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

pub fn wrap(text: &str, width: usize) -> Vec<String> {
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
    for raw in &c.lines {
        for l in wrap(raw, width.saturating_sub(2)) {
            let lead = if first {
                format!("{sym} ")
            } else {
                "  ".to_string()
            };
            out.push(Line::from(vec![Span::styled(lead, style), Span::raw(l)]));
            first = false;
        }
    }
    if c.title.is_some() || c.lines.len() > 1 {
        out.push(Line::raw(""));
    }
    out
}

pub fn draw(f: &mut Frame, app: &App, th: &Theme) {
    let area = f.area();
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(3),
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
    let mut lines: Vec<Line> = Vec::new();
    for c in &app.cards {
        lines.extend(card_lines(c, width, th));
    }
    let h = rows[1].height as usize;
    let total = lines.len();
    let max_back = total.saturating_sub(h);
    let back = app.scroll_back.min(max_back);
    let start = total.saturating_sub(h + back);
    let visible: Vec<Line> = lines.into_iter().skip(start).take(h).collect();
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

    let composer_title = match &app.mode {
        Mode::Normal => " Command ",
        Mode::Approval(_) => " Approval is open: answer it first ",
        Mode::Confirm(_) => " Confirm ",
    };
    let block = Block::default().borders(Borders::ALL).title(composer_title);
    let inner = block.inner(rows[2]);
    f.render_widget(block, rows[2]);
    let hint = if app.input.is_empty() && matches!(app.mode, Mode::Normal) {
        "Type / for commands, e.g. /help"
    } else {
        ""
    };
    let shown = if app.input.is_empty() {
        Line::from(Span::styled(
            hint,
            Style::default().add_modifier(Modifier::DIM),
        ))
    } else {
        Line::raw(app.input.clone())
    };
    f.render_widget(Paragraph::new(shown), inner);
    if matches!(app.mode, Mode::Normal) {
        let col: usize = app
            .input
            .chars()
            .take(app.cursor)
            .map(|c| c.width().unwrap_or(0))
            .sum();
        f.set_cursor_position((
            inner.x + (col as u16).min(inner.width.saturating_sub(1)),
            inner.y,
        ));
    }

    let footer = match &app.mode {
        Mode::Confirm(c) => c.prompt.clone(),
        Mode::Approval(_) => {
            "Type the digest prefix · Enter approves · Esc or Ctrl-C refuses".to_string()
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
}
