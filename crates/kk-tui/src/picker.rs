//! A searchable list in a full-screen overlay, used to choose between kept conversations and reviews.
//!
//! The behaviour follows what OpenCode's session picker does (filter as you type, newest first under date headings,
//! a failure shown inside the list instead of closing it, delete only on a second press); the logic here is written
//! for this project and runs on a list that is already in memory, so there is no search delay to hide.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Chat,
    Review,
    /// One of the owner's own messages in the open conversation, chosen to go back to it (`/rewind`).
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub when: i64,
    pub kind: ItemKind,
    /// The project a kept conversation belongs to; reviews have none.
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delete {
    /// First press: the item is marked and the next press removes it.
    Armed(String),
    /// Second press on the same item.
    Confirmed(String),
    /// Reviews are not removed here.
    NotAllowed,
    Nothing,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub title: String,
    pub items: Vec<Item>,
    pub filter: String,
    pub selected: usize,
    pub error: Option<String>,
    pub armed: Option<String>,
    pub loading: bool,
    /// The project of the folder the workspace was opened in. While `only_project` is on, conversations of other projects are hidden.
    pub project: Option<String>,
    pub only_project: bool,
    /// Said once when the list widened by itself.
    pub scope_note: Option<String>,
    /// The last messages of the conversation the owner expanded with the Right arrow: its id and the lines shown under its row.
    pub preview: Option<(String, Vec<String>)>,
}

/// Days since 1970-01-01 to a calendar date (proleptic Gregorian), after Howard Hinnant's published algorithm.
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// "Today", "Yesterday" or the UTC date. Items from the future count as today.
pub fn day_label(when: i64, now: i64) -> String {
    let (dw, dn) = (when.div_euclid(86_400), now.div_euclid(86_400));
    if dw >= dn {
        return "Today".into();
    }
    if dw == dn - 1 {
        return "Yesterday".into();
    }
    let (y, m, d) = civil(dw);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Picker {
    pub fn new(title: &str) -> Picker {
        Picker {
            title: title.into(),
            items: Vec::new(),
            filter: String::new(),
            selected: 0,
            error: None,
            armed: None,
            loading: true,
            project: None,
            only_project: false,
            scope_note: None,
            preview: None,
        }
    }

    /// Indices of items that contain every word of the filter, newest first, ties by id.
    pub fn visible(&self) -> Vec<usize> {
        let words: Vec<String> = self
            .filter
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let mut idx: Vec<usize> = (0..self.items.len())
            .filter(|i| {
                let it = &self.items[*i];
                if !self.in_scope(it) {
                    return false;
                }
                let hay = format!("{} {} {}", it.label, it.detail, it.id).to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        idx.sort_by(|a, b| {
            self.items[*b]
                .when
                .cmp(&self.items[*a].when)
                .then(self.items[*a].id.cmp(&self.items[*b].id))
        });
        idx
    }

    /// Whether an item shows under the current scope: reviews always do; a conversation does when all projects are shown or it is this project's.
    fn in_scope(&self, it: &Item) -> bool {
        match (&self.project, self.only_project, it.kind) {
            (_, false, _) | (_, _, ItemKind::Review | ItemKind::Message) | (None, _, _) => true,
            (Some(p), true, ItemKind::Chat) => it.project.as_deref() == Some(p.as_str()),
        }
    }

    /// Starts the list on this project's conversations when the folder has a project; Tab widens it to every project.
    pub fn scope_to(&mut self, project: Option<String>) {
        self.only_project = project.is_some();
        self.project = project;
    }

    /// Called once the conversations are loaded: a project with none kept shows everything instead of an empty list.
    pub fn settle_scope(&mut self) {
        if self.only_project
            && !self
                .items
                .iter()
                .any(|it| it.kind == ItemKind::Chat && self.in_scope(it))
        {
            self.only_project = false;
            self.scope_note = Some(format!(
                "No kept conversation for project {} yet: showing every project.",
                self.project.as_deref().unwrap_or("")
            ));
        }
    }

    /// Tab: this project's conversations, or every project's. Nothing to switch when the folder has no project.
    pub fn toggle_scope(&mut self) {
        if self.project.is_some() {
            self.only_project = !self.only_project;
            self.scope_note = None;
            self.selected = 0;
            self.armed = None;
            self.preview = None;
        }
    }

    fn clamp(&mut self) {
        let n = self.visible().len();
        self.selected = if n == 0 { 0 } else { self.selected.min(n - 1) };
    }

    pub fn current(&self) -> Option<&Item> {
        self.visible().get(self.selected).map(|i| &self.items[*i])
    }

    pub fn move_by(&mut self, delta: i64) {
        let n = self.visible().len() as i64;
        if n == 0 {
            return;
        }
        self.selected = (self.selected as i64 + delta).clamp(0, n - 1) as usize;
        self.armed = None;
        self.preview = None;
    }

    pub fn type_char(&mut self, c: char) {
        if self.filter.chars().count() < 80 && !c.is_control() {
            self.filter.push(c);
            self.selected = 0;
            self.armed = None;
            self.preview = None;
        }
    }

    pub fn backspace(&mut self) {
        self.filter.pop();
        self.preview = None;
        self.selected = 0;
        self.armed = None;
    }

    pub fn delete_press(&mut self) -> Delete {
        let Some(it) = self.current() else {
            return Delete::Nothing;
        };
        if it.kind != ItemKind::Chat {
            return Delete::NotAllowed;
        }
        let id = it.id.clone();
        if self.armed.as_deref() == Some(id.as_str()) {
            self.armed = None;
            Delete::Confirmed(id)
        } else {
            self.armed = Some(id.clone());
            Delete::Armed(id)
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.items.retain(|i| i.id != id);
        self.armed = None;
        self.clamp();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, label: &str, when: i64, kind: ItemKind) -> Item {
        Item {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            when,
            kind,
            project: None,
        }
    }

    #[test]
    fn pk01_civil_dates_match_known_days() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(19_723), (2024, 1, 1));
        assert_eq!(civil(-1), (1969, 12, 31));
        assert_eq!(civil(20_397), (2025, 11, 5));
    }

    #[test]
    fn pk02_day_labels_group_today_yesterday_and_older() {
        let now = 20_397 * 86_400 + 3600;
        assert_eq!(day_label(now - 10, now), "Today");
        assert_eq!(
            day_label(now + 99_999, now),
            "Today",
            "a clock that runs ahead still reads as today"
        );
        assert_eq!(day_label(20_396 * 86_400 + 5, now), "Yesterday");
        assert_eq!(day_label(20_390 * 86_400, now), "2025-10-29");
        assert_eq!(day_label(0, now), "1970-01-01");
    }

    #[test]
    fn pk03_filtering_needs_every_word_and_the_order_is_newest_first() {
        let mut p = Picker::new("t");
        p.items = vec![
            item("a", "Invoice questions", 10, ItemKind::Chat),
            item("b", "Quarterly invoice review", 30, ItemKind::Review),
            item("c", "Garden notes", 20, ItemKind::Chat),
        ];
        let order: Vec<&str> = p
            .visible()
            .iter()
            .map(|i| p.items[*i].id.as_str())
            .collect();
        assert_eq!(order, ["b", "c", "a"]);
        "invoice".chars().for_each(|c| p.type_char(c));
        let order: Vec<&str> = p
            .visible()
            .iter()
            .map(|i| p.items[*i].id.as_str())
            .collect();
        assert_eq!(order, ["b", "a"]);
        " review".chars().for_each(|c| p.type_char(c));
        assert_eq!(p.current().unwrap().id, "b");
        " zzz".chars().for_each(|c| p.type_char(c));
        assert!(p.current().is_none() && p.visible().is_empty());
        for _ in 0..4 {
            p.backspace();
        }
        assert_eq!(p.current().unwrap().id, "b");
    }

    #[test]
    fn pk04_selection_stays_inside_the_list() {
        let mut p = Picker::new("t");
        p.items = vec![
            item("a", "x", 1, ItemKind::Chat),
            item("b", "y", 2, ItemKind::Chat),
        ];
        p.move_by(5);
        assert_eq!(p.selected, 1);
        p.move_by(-9);
        assert_eq!(p.selected, 0);
        let mut e = Picker::new("t");
        e.move_by(1);
        assert!(e.current().is_none());
    }

    #[test]
    fn pk05_delete_needs_a_second_press_on_the_same_chat_and_never_applies_to_reviews() {
        let mut p = Picker::new("t");
        p.items = vec![
            item("a", "x", 2, ItemKind::Chat),
            item("b", "y", 1, ItemKind::Chat),
            item("r", "z", 0, ItemKind::Review),
        ];
        assert_eq!(p.delete_press(), Delete::Armed("a".into()));
        p.move_by(1);
        assert_eq!(
            p.delete_press(),
            Delete::Armed("b".into()),
            "moving disarms the first"
        );
        assert_eq!(p.delete_press(), Delete::Confirmed("b".into()));
        p.remove("b");
        assert_eq!(p.items.len(), 2);
        p.move_by(1);
        assert_eq!(p.current().unwrap().id, "r");
        assert_eq!(p.delete_press(), Delete::NotAllowed);
        p.type_char('q');
        assert_eq!(p.delete_press(), Delete::Nothing);
        p.type_char('\u{7}');
        assert_eq!(
            p.filter, "q",
            "control characters are not typed into the filter"
        );
    }
}
