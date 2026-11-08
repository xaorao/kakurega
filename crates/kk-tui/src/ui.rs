use crate::app::{App, Card, Kind, Mode, DIGEST_ACK_LEN};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy)]
pub struct Theme {
    pub color: bool,
}

impl Theme {
    pub fn from_env() -> Theme {
        Theme {
            color: std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn fg(&self, c: Color) -> Style {
        if self.color {
            Style::default().fg(c)
        } else {
            Style::default()
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
        Kind::User => ("›", th.fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Kind::Info => ("·", Style::default()),
        Kind::Ok => ("✓", th.fg(Color::Green)),
        Kind::Warn => ("!", th.fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Kind::Error => ("✗", th.fg(Color::Red).add_modifier(Modifier::BOLD)),
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
    if matches!(c.title.as_deref(), Some("Assistant") | Some("Aside")) {
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
                th.fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if raw.starts_with('-') {
                th.fg(Color::Red).add_modifier(Modifier::DIM)
            } else if raw.starts_with("@@") {
                th.fg(Color::Cyan)
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
                format!(
                    "… {} more line(s). Ctrl-O shows everything and folds it again.",
                    c.lines.len() - shown
                ),
                Style::default().add_modifier(Modifier::DIM),
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

pub fn draw(f: &mut Frame, app: &App, th: &Theme) {
    EXPAND.with(|e| e.set(app.expand_all));
    let area = f.area();
    let input_lines = app.input.split('\n').count().clamp(1, 6) as u16;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(input_lines + 2),
        Constraint::Length(1),
    ])
    .split(area);
    let head = Line::from(vec![
        Span::styled(
            " kakurega ",
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw(format!(" {}", app.status_line())),
    ]);
    f.render_widget(Paragraph::new(head), rows[0]);

    let width = rows[1].width as usize;
    let h = rows[1].height as usize;
    let (visible, back) = window_lines(&app.cards, width, th, h, app.scroll_back);
    f.render_widget(Paragraph::new(visible), rows[1]);
    if back > 0 {
        let note = format!(" {back} more line(s) below · PageDown ");
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
    let all: Vec<&str> = app.input.split('\n').collect();
    let (cur_row, cur_col) = {
        let before: String = app.input.chars().take(app.cursor).collect();
        let row = before.matches('\n').count();
        let col: usize = before
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .chars()
            .map(|c| c.width().unwrap_or(0))
            .sum();
        (row, col)
    };
    let h = inner.height as usize;
    let first = (cur_row + 1)
        .saturating_sub(h)
        .min(all.len().saturating_sub(h.min(all.len())));
    let shown: Vec<Line> = if app.input.is_empty() {
        vec![Line::from(Span::styled(
            hint,
            Style::default().add_modifier(Modifier::DIM),
        ))]
    } else {
        all.iter()
            .skip(first)
            .take(h)
            .map(|l| Line::raw((*l).to_string()))
            .collect()
    };
    f.render_widget(Paragraph::new(shown), inner);
    if matches!(app.mode, Mode::Normal) {
        f.set_cursor_position((
            inner.x + (cur_col as u16).min(inner.width.saturating_sub(1)),
            inner.y + (cur_row.saturating_sub(first) as u16).min(inner.height.saturating_sub(1)),
        ));
    }

    let footer = match &app.mode {
        Mode::Confirm(c) => c.prompt.clone(),
        Mode::Picker(_) => "Up/Down choose · Right previews · Enter opens · type to filter · Ctrl-D twice forgets a chat · Esc closes".to_string(),
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
    f.render_widget(
        Paragraph::new(Span::styled(
            footer,
            Style::default().add_modifier(Modifier::DIM),
        )),
        rows[3],
    );

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
            .border_style(th.fg(Color::Magenta).add_modifier(Modifier::BOLD));
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
                th.fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
        } else {
            prompt.push(Line::from(Span::styled(
                "PgUp/PgDn scroll the release. Esc refuses and records the refusal.",
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
        f.render_widget(Paragraph::new(prompt), parts[1]);
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
            .border_style(th.fg(Color::Cyan).add_modifier(Modifier::BOLD));
        let inner = block.inner(pop);
        f.render_widget(block, pop);
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let filter = if p.filter.is_empty() {
            Span::styled(
                "type to filter",
                Style::default().add_modifier(Modifier::DIM),
            )
        } else {
            Span::raw(p.filter.clone())
        };
        let mut head = vec![
            Span::styled("Filter: ", Style::default().add_modifier(Modifier::BOLD)),
            filter,
        ];
        if let Some(project) = &p.project {
            head.push(Span::styled(
                if p.only_project {
                    format!("   [project {project} · Tab: all projects]")
                } else {
                    format!("   [all projects · Tab: only {project}]")
                },
                th.fg(Color::Cyan),
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
            };
            if group != last_group {
                rows.push((
                    Line::from(Span::styled(
                        group.clone(),
                        th.fg(Color::Cyan).add_modifier(Modifier::BOLD),
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
                th.fg(Color::Red).add_modifier(Modifier::BOLD)
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
                            Line::from(Span::styled(
                                format!("      {l}"),
                                Style::default().add_modifier(Modifier::DIM),
                            )),
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
            vec![Line::from(Span::styled(
                msg,
                Style::default().add_modifier(Modifier::DIM),
            ))]
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
                    th.fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                parts[2],
            );
        } else if let Some(n) = &p.scope_note {
            f.render_widget(
                Paragraph::new(Span::styled(
                    n.clone(),
                    Style::default().add_modifier(Modifier::DIM),
                )),
                parts[2],
            );
        }
    }
}
