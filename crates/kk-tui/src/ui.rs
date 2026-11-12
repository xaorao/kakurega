use crate::app::{App, Card, Kind, Mode, DIGEST_ACK_LEN};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

/// Which set of colours is used. `Auto` is the terminal's own named colours, which the terminal's palette decides and so cannot be
/// measured; `Dark` and `Light` are fixed colours measured against the backgrounds they are for (see `contrast` in the tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    Auto,
    Dark,
    Light,
}

impl Palette {
    pub fn parse(word: &str) -> Option<Palette> {
        match word.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Palette::Auto),
            "dark" => Some(Palette::Dark),
            "light" => Some(Palette::Light),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Palette::Auto => "auto",
            Palette::Dark => "dark",
            Palette::Light => "light",
        }
    }
}

/// What a colour means. The words always carry the meaning too; colour only helps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Accent,
    Ok,
    Warn,
    Bad,
    Code,
}

pub const ROLES: [Role; 5] = [Role::Accent, Role::Ok, Role::Warn, Role::Bad, Role::Code];

impl Palette {
    /// The colour for a role; `None` for `Auto` means the terminal's named colour is used.
    pub fn rgb(self, role: Role) -> Option<(u8, u8, u8)> {
        match (self, role) {
            (Palette::Auto, _) => None,
            (Palette::Dark, Role::Accent) => Some((0x5f, 0xd7, 0xff)),
            (Palette::Dark, Role::Ok) => Some((0x5f, 0xd7, 0x5f)),
            (Palette::Dark, Role::Warn) => Some((0xff, 0x87, 0xff)),
            (Palette::Dark, Role::Bad) => Some((0xff, 0x87, 0x87)),
            (Palette::Dark, Role::Code) => Some((0xff, 0xd7, 0x87)),
            (Palette::Light, Role::Accent) => Some((0x00, 0x5f, 0x87)),
            (Palette::Light, Role::Ok) => Some((0x00, 0x5f, 0x00)),
            (Palette::Light, Role::Warn) => Some((0x87, 0x00, 0x87)),
            (Palette::Light, Role::Bad) => Some((0xaf, 0x00, 0x00)),
            (Palette::Light, Role::Code) => Some((0x50, 0x50, 0x00)),
        }
    }

    /// Secondary text in the fixed palettes. `Auto` uses the DIM attribute instead, which the terminal decides.
    pub fn muted_rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            Palette::Auto => None,
            Palette::Dark => Some((0xb2, 0xb2, 0xb2)),
            Palette::Light => Some((0x58, 0x58, 0x58)),
        }
    }

    fn named(role: Role) -> Color {
        match role {
            Role::Accent => Color::Cyan,
            Role::Ok => Color::Green,
            Role::Warn => Color::Magenta,
            Role::Bad => Color::Red,
            Role::Code => Color::Yellow,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub color: bool,
    pub palette: Palette,
}

impl Theme {
    pub fn from_env() -> Theme {
        Theme {
            color: std::env::var_os("NO_COLOR").is_none(),
            palette: Palette::Auto,
        }
    }

    pub fn fg(&self, role: Role) -> Style {
        if !self.color {
            return Style::default();
        }
        Style::default().fg(match self.palette.rgb(role) {
            Some((r, g, b)) => Color::Rgb(r, g, b),
            None => Palette::named(role),
        })
    }

    /// A role's colour, softened: dimmed in `Auto`, and the plain role colour in a fixed palette (dimming would lower its contrast).
    pub fn fg_soft(&self, role: Role) -> Style {
        match (self.color, self.palette) {
            (true, Palette::Auto) | (false, _) => self.fg(role).add_modifier(Modifier::DIM),
            _ => self.fg(role),
        }
    }

    /// Secondary text: dimmed in `Auto` and without colour, a measured grey in a fixed palette.
    pub fn dim(&self) -> Style {
        match (self.color, self.palette.muted_rgb()) {
            (true, Some((r, g, b))) => Style::default().fg(Color::Rgb(r, g, b)),
            _ => Style::default().add_modifier(Modifier::DIM),
        }
    }
}

thread_local! {
    static WRAPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

thread_local! {
    static EXPAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A card with more lines than this is folded unless the owner asked to see everything.
pub const FOLD_OVER: usize = 24;
/// How many lines a folded card still shows.
pub const FOLD_SHOW: usize = 10;

/// How many times `wrap` ran on this thread: a test counts it to prove a frame does work only for what is on the screen.
pub fn wrap_calls() -> usize {
    WRAPS.with(|c| c.get())
}

pub fn wrap(text: &str, width: usize) -> Vec<String> {
    WRAPS.with(|c| c.set(c.get() + 1));
    let width = width.max(8);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let indent: String = raw.chars().take_while(|c| *c == ' ').collect();
        let mut line = String::new();
        let mut w = 0usize;
        let mut word = String::new();
        let mut ww = 0usize;
        let flush_word = |line: &mut String,
                          w: &mut usize,
                          word: &mut String,
                          ww: &mut usize,
                          out: &mut Vec<String>| {
            if word.is_empty() {
                return;
            }
            if *w + *ww > width {
                if line.trim().is_empty() {
                    line.clear();
                    *w = 0;
                } else {
                    out.push(std::mem::take(line));
                    line.push_str(&indent);
                    *w = indent.chars().count();
                    if *w + *ww > width {
                        line.clear();
                        *w = 0;
                    }
                }
            }
            line.push_str(word);
            *w += *ww;
            word.clear();
            *ww = 0;
        };
        for c in raw.chars() {
            let cw = c.width().unwrap_or(0);
            if c == ' ' {
                flush_word(&mut line, &mut w, &mut word, &mut ww, &mut out);
                if w < width {
                    line.push(' ');
                    w += 1;
                }
                continue;
            }
            if ww + cw > width {
                flush_word(&mut line, &mut w, &mut word, &mut ww, &mut out);
                if w > 0 {
                    out.push(std::mem::take(&mut line));
                    w = 0;
                }
            }
            word.push(c);
            ww += cw;
        }
        flush_word(&mut line, &mut w, &mut word, &mut ww, &mut out);
        out.push(line);
    }
    out
}

fn card_lines(c: &Card, width: usize, th: &Theme) -> Vec<Line<'static>> {
    let (sym, style) = match c.kind {
        Kind::User => ("›", th.fg(Role::Accent).add_modifier(Modifier::BOLD)),
        Kind::Info => ("·", Style::default()),
        Kind::Ok => ("✓", th.fg(Role::Ok)),
        Kind::Warn => ("!", th.fg(Role::Warn).add_modifier(Modifier::BOLD)),
        Kind::Error => ("✗", th.fg(Role::Bad).add_modifier(Modifier::BOLD)),
    };
    let mut out = Vec::new();
    let mut first = true;
    if let Some(t) = &c.title {
        for l in wrap(t, width.saturating_sub(2)) {
            out.push(Line::from(vec![
                Span::styled(
                    if first {
                        format!("{sym} ")
                    } else {
                        "  ".to_string()
                    },
                    style,
                ),
                Span::styled(l, style.add_modifier(Modifier::BOLD)),
            ]));
            first = false;
        }
    }
    // an answer from the assistant is formatted text; everything else is shown as written
    if matches!(c.title.as_deref(), Some("Assistant") | Some("Aside"))
        || c.title
            .as_deref()
            .is_some_and(|t| t.starts_with(crate::app::TABLE_TITLE) || t.starts_with("Team: "))
    {
        for body in crate::markdown::render(&c.lines.join("\n"), width.saturating_sub(2), th) {
            let lead = if first {
                format!("{sym} ")
            } else {
                "  ".to_string()
            };
            let mut spans = vec![Span::styled(lead, style)];
            spans.extend(body.spans);
            out.push(Line::from(spans));
            first = false;
        }
        out.push(Line::raw(""));
        return out;
    }
    // the change inside a proposed fix: the markers + and - already tell added from removed without any colour, so colour and weight
    // only help the eye, and a plain terminal loses nothing
    let in_fix = c.title.as_deref().is_some_and(|t| t.starts_with("Fix "));
    // a long result is folded to its first lines and says how many it hides; answers from the assistant are never folded
    let fold = !EXPAND.with(|e| e.get()) && c.lines.len() > FOLD_OVER;
    let shown = if fold { FOLD_SHOW } else { c.lines.len() };
    for raw in c.lines.iter().take(shown) {
        let body_style = if in_fix {
            if raw.starts_with("+++") || raw.starts_with("---") {
                Style::default().add_modifier(Modifier::BOLD)
            } else if raw.starts_with('+') {
                th.fg(Role::Ok).add_modifier(Modifier::BOLD)
            } else if raw.starts_with('-') {
                th.fg_soft(Role::Bad)
            } else if raw.starts_with("@@") {
                th.fg(Role::Accent)
            } else {
                Style::default()
            }
        } else {
            Style::default()
        };
        for l in wrap(raw, width.saturating_sub(2)) {
            let lead = if first {
                format!("{sym} ")
            } else {
                "  ".to_string()
            };
            out.push(Line::from(vec![
                Span::styled(lead, style),
                Span::styled(l, body_style),
            ]));
            first = false;
        }
    }
    if fold {
        out.push(Line::from(vec![
            Span::styled("  ".to_string(), style),
            Span::styled(
                crate::app::counted(&format!(
                    "… {} more line(s). Ctrl-O shows everything and folds it again.",
                    c.lines.len() - shown
                )),
                th.dim(),
            ),
        ]));
    }
    if c.title.is_some() || c.lines.len() > 1 {
        out.push(Line::raw(""));
    }
    out
}

/// The `h` lines to show when the owner has scrolled `back` lines up from the newest, and how far back that really is (it cannot
/// go past the oldest line). Cards are wrapped from the newest backwards and only until enough lines exist, so a conversation of
/// thousands of cards costs what the screen shows, not what the conversation holds. The result is the same as wrapping every
/// card and cutting the window out of the whole.
pub fn window_lines(
    cards: &[Card],
    width: usize,
    th: &Theme,
    h: usize,
    back: usize,
) -> (Vec<Line<'static>>, usize) {
    let need = h.saturating_add(back);
    let mut chunks: Vec<Vec<Line<'static>>> = Vec::new();
    let mut total = 0usize;
    for c in cards.iter().rev() {
        let l = card_lines(c, width, th);
        total += l.len();
        chunks.push(l);
        if total >= need {
            break;
        }
    }
    let back = back.min(total.saturating_sub(h));
    let start = total.saturating_sub(h + back);
    let visible = chunks
        .into_iter()
        .rev()
        .flatten()
        .skip(start)
        .take(h)
        .collect();
    (visible, back)
}

/// The message as the rows it is drawn in, and the row and column of the cursor: a line break starts a row, and so does
/// text that reaches the width of the box (counted by how wide each character is drawn). `cursor` counts characters.
pub fn wrap_input(input: &str, cursor: usize, width: usize) -> (Vec<String>, usize, usize) {
    let width = width.max(1);
    let mut rows: Vec<String> = vec![String::new()];
    let mut col = 0usize;
    let (mut cur_row, mut cur_col) = (0usize, 0usize);
    for (i, c) in input.chars().enumerate() {
        if i == cursor {
            (cur_row, cur_col) = (rows.len() - 1, col);
        }
        if c == '\n' {
            rows.push(String::new());
            col = 0;
            continue;
        }
        let w = c.width().unwrap_or(0);
        if col + w > width && col > 0 {
            rows.push(String::new());
            col = 0;
            if i == cursor {
                (cur_row, cur_col) = (rows.len() - 1, 0);
            }
        }
        rows.last_mut().expect("a row").push(c);
        col += w;
    }
    if cursor >= input.chars().count() {
        // after the last character: on a new row when this one is full
        if col >= width {
            rows.push(String::new());
            col = 0;
        }
        (cur_row, cur_col) = (rows.len() - 1, col);
    }
    (rows, cur_row, cur_col)
}

pub fn draw(f: &mut Frame, app: &App, th: &Theme) {
    EXPAND.with(|e| e.set(app.expand_all));
    let area = f.area();
    // the message box grows with what is typed, counted as it is drawn: a long line takes the rows it wraps to
    let input_lines = wrap_input(
        &app.input,
        app.cursor,
        area.width.saturating_sub(2) as usize,
    )
    .0
    .len()
    .clamp(1, 6) as u16;
    let footer = match &app.mode {
        Mode::Confirm(c) => c.prompt.clone(),
        Mode::Picker(_) => "Up/Down choose · Right previews · Enter opens · Ctrl-A puts away · Ctrl-R archived · Ctrl-D twice forgets · Esc closes".to_string(),
        Mode::Approval(_) => {
            "Type the digest prefix · Enter approves · Esc or Ctrl-C refuses".to_string()
        }
        Mode::Normal if app.chat_waiting && app.esc_armed > 0 => {
            "Esc again to stop the answer · Ctrl-C quits".to_string()
        }
        Mode::Normal if app.chat_waiting => {
            "Esc twice stops the answer · new messages wait their turn · Ctrl-C quits".to_string()
        }
        Mode::Normal => {
            "Tab complete · Up/Down history · PgUp/PgDn scroll · /help · Ctrl-C quits".to_string()
        }
    };
    // a question must be read whole: it wraps onto up to three rows; the hints drop whole parts to fit one
    let footer_rows: Vec<String> = if matches!(app.mode, Mode::Confirm(_)) {
        let mut r = wrap(&footer, area.width as usize);
        r.truncate(3);
        r
    } else {
        let parts: Vec<String> = footer.split(" · ").map(str::to_string).collect();
        vec![fit_parts(&parts, area.width as usize, true)]
    };
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(input_lines + 2),
        Constraint::Length(footer_rows.len().max(1) as u16),
    ])
    .split(area);
    let status = app.status_line();
    let status_parts: Vec<String> = status.split(" · ").map(str::to_string).collect();
    let head = Line::from(vec![
        Span::styled(
            " kakurega ",
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw(format!(
            " {}",
            fit_parts(
                &status_parts,
                (area.width as usize).saturating_sub(12),
                false
            )
        )),
    ]);
    f.render_widget(Paragraph::new(head), rows[0]);

    let width = rows[1].width as usize;
    let h = rows[1].height as usize;
    let (visible, back) = window_lines(&app.cards, width, th, h, app.scroll_back);
    f.render_widget(Paragraph::new(visible), rows[1]);
    if back > 0 {
        let note = crate::app::counted(&format!(" {back} more line(s) below · PageDown "));
        let w = note.chars().count() as u16;
        let r = Rect {
            x: rows[1].x + rows[1].width.saturating_sub(w),
            y: rows[1].y + rows[1].height.saturating_sub(1),
            width: w.min(rows[1].width),
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                note,
                Style::default().add_modifier(Modifier::REVERSED),
            )),
            r,
        );
    }

    let completions = app.completions();
    if !completions.is_empty() {
        let shown = completions.len().min(8);
        let h = shown as u16 + 2;
        let r = rows[1];
        if r.height >= h && r.width >= 20 {
            let pop = Rect {
                x: r.x,
                y: r.y + r.height - h,
                width: r.width.min(72),
                height: h,
            };
            f.render_widget(Clear, pop);
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Commands · Tab completes · Enter picks · Esc hides ");
            let inner = block.inner(pop);
            f.render_widget(block, pop);
            let sel = app.popup_sel.min(completions.len() - 1);
            let first = sel.saturating_sub(shown - 1);
            let pad = completions.iter().map(|n| n.len()).max().unwrap_or(0) + 2;
            let lines: Vec<Line> = completions
                .iter()
                .enumerate()
                .skip(first)
                .take(shown)
                .map(|(i, n)| {
                    let text = format!("/{:<pad$}{}", n, crate::cmd::describe(n), pad = pad);
                    if i == sel {
                        Line::from(Span::styled(
                            text,
                            Style::default().add_modifier(Modifier::REVERSED),
                        ))
                    } else {
                        Line::raw(text)
                    }
                })
                .collect();
            f.render_widget(Paragraph::new(lines), inner);
        }
    }

    let composer_title = match &app.mode {
        Mode::Normal => " Command ",
        Mode::Approval(_) => " Approval is open: answer it first ",
        Mode::Confirm(_) => " Confirm ",
        Mode::Picker(_) => " Choose with the arrow keys; typing filters ",
    };
    let block = Block::default().borders(Borders::ALL).title(composer_title);
    let inner = block.inner(rows[2]);
    f.render_widget(block, rows[2]);
    let hint = if app.input.is_empty() && matches!(app.mode, Mode::Normal) {
        "Type / for commands, e.g. /help"
    } else {
        ""
    };
    // text wider than the box wraps to the next row, so everything typed and the place of the cursor can be seen
    let (all, cur_row, cur_col) = wrap_input(&app.input, app.cursor, inner.width as usize);
    let h = inner.height as usize;
    let first = (cur_row + 1)
        .saturating_sub(h)
        .min(all.len().saturating_sub(h.min(all.len())));
    let shown: Vec<Line> = if app.input.is_empty() {
        vec![Line::from(Span::styled(hint, th.dim()))]
    } else {
        all.iter()
            .skip(first)
            .take(h)
            .map(|l| Line::raw(l.clone()))
            .collect()
    };
    f.render_widget(Paragraph::new(shown), inner);
    if matches!(app.mode, Mode::Normal) {
        f.set_cursor_position((
            inner.x + (cur_col as u16).min(inner.width.saturating_sub(1)),
            inner.y + (cur_row.saturating_sub(first) as u16).min(inner.height.saturating_sub(1)),
        ));
    }

    let footer_lines: Vec<Line> = footer_rows
        .into_iter()
        .map(|r| Line::from(Span::styled(r, th.dim())))
        .collect();
    f.render_widget(Paragraph::new(footer_lines), rows[3]);

    if let Mode::Approval(v) = &app.mode {
        let pop = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(2),
        };
        f.render_widget(Clear, pop);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Approve release for {} · {} ", v.job, v.aid))
            .border_style(th.fg(Role::Warn).add_modifier(Modifier::BOLD));
        let inner = block.inner(pop);
        f.render_widget(block, pop);
        let parts = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(inner);
        let body: Vec<Line> = v
            .lines
            .iter()
            .flat_map(|l| wrap(l, parts[0].width as usize))
            .map(Line::raw)
            .collect();
        let max = body.len().saturating_sub(parts[0].height as usize) as u16;
        f.render_widget(
            Paragraph::new(body).scroll((v.scroll.min(max), 0)),
            parts[0],
        );
        let want: String = v.digest.chars().take(DIGEST_ACK_LEN).collect();
        let mut prompt = vec![Line::from(vec![
            Span::raw(format!(
                "To approve, type the first {DIGEST_ACK_LEN} characters of the digest ({want}): "
            )),
            Span::styled(
                format!(
                    "{}{}",
                    v.typed,
                    "_".repeat(DIGEST_ACK_LEN.saturating_sub(v.typed.len()))
                ),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ])];
        if let Some(e) = &v.error {
            prompt.push(Line::from(Span::styled(
                format!("✗ {e}"),
                th.fg(Role::Bad).add_modifier(Modifier::BOLD),
            )));
        } else {
            prompt.push(Line::from(Span::styled(
                "PgUp/PgDn scroll the release. Esc refuses and records the refusal.",
                th.dim(),
            )));
        }
        f.render_widget(Paragraph::new(prompt).wrap(Wrap { trim: false }), parts[1]);
    }

    if let Mode::Picker(p) = &app.mode {
        let pop = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(2),
        };
        f.render_widget(Clear, pop);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", p.title))
            .border_style(th.fg(Role::Accent).add_modifier(Modifier::BOLD));
        let inner = block.inner(pop);
        f.render_widget(block, pop);
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let filter = if p.filter.is_empty() {
            Span::styled("type to filter", th.dim())
        } else {
            Span::raw(p.filter.clone())
        };
        let mut head = vec![
            Span::styled("Filter: ", Style::default().add_modifier(Modifier::BOLD)),
            filter,
        ];
        if p.show_archived {
            head.push(Span::styled(
                "   [archived · Ctrl-R: back to the list in use]",
                th.fg(Role::Accent),
            ));
        } else if let Some(project) = &p.project {
            head.push(Span::styled(
                if p.only_project {
                    format!("   [project {project} · Tab: all projects]")
                } else {
                    format!("   [all projects · Tab: only {project}]")
                },
                th.fg(Role::Accent),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(head)), parts[0]);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let vis = p.visible();
        let mut rows: Vec<(Line, bool)> = Vec::new();
        let mut last_group = String::new();
        let mut selected_row = 0usize;
        for (n, i) in vis.iter().enumerate() {
            let it = &p.items[*i];
            let group = match it.kind {
                crate::picker::ItemKind::Chat => crate::picker::day_label(it.when, now),
                crate::picker::ItemKind::Review => "Reviews".to_string(),
                crate::picker::ItemKind::Message => "Your messages, newest first".to_string(),
                crate::picker::ItemKind::BranchPoint => "Messages, newest first".to_string(),
            };
            if group != last_group {
                rows.push((
                    Line::from(Span::styled(
                        group.clone(),
                        th.fg(Role::Accent).add_modifier(Modifier::BOLD),
                    )),
                    false,
                ));
                last_group = group;
            }
            let armed = p.armed.as_deref() == Some(it.id.as_str());
            let text = if armed {
                format!("  {}  [Ctrl-D again to forget]", it.label)
            } else {
                format!("  {}  · {}", it.label, it.detail)
            };
            if n == p.selected {
                selected_row = rows.len();
            }
            let style = if armed {
                th.fg(Role::Bad).add_modifier(Modifier::BOLD)
            } else if n == p.selected {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            rows.push((Line::from(Span::styled(text, style)), n == p.selected));
            if let Some((pid, lines)) = &p.preview {
                if *pid == it.id {
                    for l in lines {
                        rows.push((
                            Line::from(Span::styled(format!("      {l}"), th.dim())),
                            false,
                        ));
                    }
                }
            }
        }
        let height = parts[1].height as usize;
        // the lines of an expanded preview sit under the chosen row and stay in view with it
        let extra = p.preview.as_ref().map_or(0, |(_, l)| l.len());
        let skip = (selected_row + extra)
            .saturating_sub(height.saturating_sub(1))
            .min(selected_row);
        let shown: Vec<Line> = if rows.is_empty() {
            let msg = if p.loading {
                "Loading…"
            } else if p.filter.is_empty() {
                "Nothing here yet. /keep saves a conversation; /review starts a review."
            } else {
                "No match."
            };
            vec![Line::from(Span::styled(msg, th.dim()))]
        } else {
            rows.into_iter()
                .skip(skip)
                .take(height)
                .map(|(l, _)| l)
                .collect()
        };
        f.render_widget(Paragraph::new(shown), parts[1]);
        if let Some(e) = &p.error {
            f.render_widget(
                Paragraph::new(Span::styled(
                    e.clone(),
                    th.fg(Role::Bad).add_modifier(Modifier::BOLD),
                )),
                parts[2],
            );
        } else if let Some(n) = &p.scope_note {
            f.render_widget(Paragraph::new(Span::styled(n.clone(), th.dim())), parts[2]);
        }
    }
}

/// Parts joined by " · " that fit in `width` columns. With `keep_last`, the first and the last part stay (the last is the
/// way out) and those between are kept in order while they fit; otherwise parts are dropped from the end. A part is never
/// cut, except a lone one that cannot fit, which is shortened at a word with an ellipsis.
pub fn fit_parts(parts: &[String], width: usize, keep_last: bool) -> String {
    use unicode_width::UnicodeWidthStr;
    let join = |v: &[&String]| v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" · ");
    let fits = |v: &[&String]| join(v).width() <= width;
    let all: Vec<&String> = parts.iter().collect();
    if fits(&all) || parts.is_empty() {
        return join(&all);
    }
    let mut chosen: Vec<&String> = Vec::new();
    if keep_last && parts.len() >= 2 {
        let (first, last) = (&parts[0], &parts[parts.len() - 1]);
        let mut middle: Vec<&String> = Vec::new();
        for p in &parts[1..parts.len() - 1] {
            let mut trial = vec![first];
            trial.extend(middle.iter().copied());
            trial.push(p);
            trial.push(last);
            if fits(&trial) {
                middle.push(p);
            }
        }
        let mut trial = vec![first];
        trial.extend(middle);
        trial.push(last);
        if fits(&trial) {
            chosen = trial;
        } else if fits(&[last]) {
            chosen = vec![last];
        }
    } else {
        for p in parts {
            let mut trial = chosen.clone();
            trial.push(p);
            if !fits(&trial) {
                break;
            }
            chosen = trial;
        }
    }
    if !chosen.is_empty() {
        return join(&chosen);
    }
    // not even one part fits: the first is shortened at a word
    let mut out = String::new();
    for word in parts[0].split(' ') {
        let next = if out.is_empty() {
            word.to_string()
        } else {
            format!("{out} {word}")
        };
        if (next.width() + 1) > width {
            break;
        }
        out = next;
    }
    // one word longer than the room: as much of it as fits
    if out.is_empty() {
        out = parts[0].chars().take(width.saturating_sub(1)).collect();
    }
    format!("{out}…")
}

#[cfg(test)]
mod fit_tests {
    use super::fit_parts;

    fn v(s: &str) -> Vec<String> {
        s.split(" · ").map(str::to_string).collect()
    }

    #[test]
    fn whole_parts_are_dropped_never_cut_and_the_way_out_stays() {
        let footer = v("Tab complete · Up/Down history · PgUp/PgDn scroll · /help · Ctrl-C quits");
        assert_eq!(fit_parts(&footer, 200, true), footer.join(" · "));
        let narrow = fit_parts(&footer, 50, true);
        assert!(
            narrow.starts_with("Tab complete")
                && narrow.ends_with("Ctrl-C quits")
                && narrow.chars().count() <= 50,
            "{narrow}"
        );
        assert_eq!(fit_parts(&footer, 14, true), "Ctrl-C quits");
        let head =
            v("qwen2.5-coder:7b · setup needed (/status) · 3 reviews · 1 running · project acme");
        let h = fit_parts(&head, 45, false);
        assert_eq!(h, "qwen2.5-coder:7b · setup needed (/status)");
        let tiny = fit_parts(&head, 12, false);
        assert_eq!(tiny, "qwen2.5-cod…");
    }
}

#[cfg(test)]
mod wrap_tests {
    use super::wrap_input;

    #[test]
    fn long_text_wraps_and_the_cursor_follows() {
        let (rows, r, c) = wrap_input("abcdefghij", 10, 4);
        assert_eq!(rows, vec!["abcd", "efgh", "ij"]);
        assert_eq!((r, c), (2, 2));
        let (rows, r, c) = wrap_input("abcd", 4, 4);
        assert_eq!(
            (rows.len(), r, c),
            (2, 1, 0),
            "a full row puts the cursor on the next one"
        );
        let (rows, r, c) = wrap_input("ab\ncdefg", 5, 4);
        assert_eq!(rows, vec!["ab", "cdef", "g"]);
        assert_eq!((r, c), (1, 2));
        // a wide character takes two columns and is never cut in half
        let (rows, _, _) = wrap_input("a\u{65e5}\u{672c}b", 0, 3);
        assert_eq!(rows, vec!["a\u{65e5}", "\u{672c}b"]);
        assert_eq!(wrap_input("", 0, 10), (vec![String::new()], 0, 0));
    }
}
