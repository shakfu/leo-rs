//! Drawing: the outline, the body, the status line and the help overlay.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Focus, Mode};
use crate::bindings::{self, BINDINGS};
use crate::commands;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(app.tree_percent),
            Constraint::Percentage(100 - app.tree_percent),
        ])
        .split(chunks[1]);

    draw_breadcrumb(f, app, chunks[0]);
    draw_outline(f, app, panes[0]);
    draw_body(f, app, panes[1]);
    draw_status(f, app, chunks[2]);
    if app.mode == Mode::Help {
        draw_help(f, app, area);
    }
}

/// The path to the current node, so a deep node says where it is.
fn draw_breadcrumb(f: &mut Frame, app: &App, area: Rect) {
    let text = truncate(&format!(" {}", app.breadcrumb()), area.width as usize);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().fg(Color::Cyan),
        ))),
        area,
    );
}

/// The border of the focused pane is thick; the other is plain.
fn pane_block(title: String, focused: bool) -> Block<'static> {
    let block = Block::default().borders(Borders::ALL).title(title);
    if focused {
        block
            .border_type(BorderType::Thick)
            .border_style(Style::default().fg(Color::Cyan))
    } else {
        block.border_style(Style::default().fg(Color::DarkGray))
    }
}

fn draw_outline(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    app.tree_height = inner_height.max(1);
    let (rows, current) = app.rows_and_current();
    // Keep the selected row on screen without recentring on every keypress.
    if current < app.top {
        app.top = current;
    } else if inner_height > 0 && current >= app.top + inner_height {
        app.top = current + 1 - inner_height;
    }
    let width = area.width.saturating_sub(2) as usize;
    let focused = app.focus == Focus::Tree;

    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in rows.iter().enumerate().skip(app.top).take(inner_height) {
        // A fold marker Leo users know, and a cursor column so the view is
        // still readable where reverse video is not, as in --dump.
        let marker = if !row.has_children {
            "  "
        } else if row.expanded {
            "- "
        } else {
            "+ "
        };
        let flags = format!(
            "{}{}{}{}",
            if i == current { ">" } else { " " },
            if row.marked { "*" } else { " " },
            if row.cloned { "C" } else { " " },
            if row.dirty { "~" } else { " " },
        );
        let indent = "  ".repeat(row.depth);
        let text = truncate(&format!("{flags}{indent}{marker}{}", row.headline), width);
        let style = if i == current {
            let bg = if focused {
                Color::Blue
            } else {
                Color::DarkGray
            };
            Style::default()
                .bg(bg)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if row.marked {
            Style::default().fg(Color::Yellow)
        } else if row.is_file {
            Style::default().fg(Color::Green)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    let title = format!(" outline {}/{} ", current + 1, rows.len());
    f.render_widget(
        Paragraph::new(lines).block(pane_block(title, focused)),
        area,
    );
}

fn draw_body(f: &mut Frame, app: &mut App, area: Rect) {
    // The body is the model's text, unless a change is being typed.
    let lines = app.body_buffer();
    let editing = app.focus == Focus::Body;
    let cursor = editing.then_some(app.editor.cursor);
    let selection = app.editor.visual_range(&lines);
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    app.body_height = inner_height.max(1);

    // Scroll to the cursor while editing, and by hand otherwise.
    let max_top = lines.len().saturating_sub(1);
    let top = match cursor {
        Some((row, _)) if inner_height > 0 => {
            if row < app.body_scroll {
                row
            } else if row >= app.body_scroll + inner_height {
                row + 1 - inner_height
            } else {
                app.body_scroll
            }
        }
        _ => app.body_scroll.min(max_top),
    };
    app.body_scroll = top;

    let number_width = if app.options.number {
        format!("{} ", lines.len()).len()
    } else {
        0
    };
    let text_width = inner_width.saturating_sub(number_width);
    let shown: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(top)
        .take(inner_height)
        .map(|(i, l)| {
            let text = truncate(&l.replace('\t', "    "), text_width);
            // A selected line is shown reversed. The exact columns matter less
            // than seeing what an operator would take.
            let selected = matches!(selection, Some((a, b, _)) if i >= a.0 && i <= b.0);
            let body = if selected {
                Span::styled(text, Style::default().bg(Color::DarkGray).fg(Color::White))
            } else {
                Span::raw(text)
            };
            if number_width > 0 {
                Line::from(vec![
                    Span::styled(
                        format!("{:>w$} ", i + 1, w = number_width.saturating_sub(1)),
                        Style::default().fg(Color::DarkGray),
                    ),
                    body,
                ])
            } else {
                Line::from(body)
            }
        })
        .collect();
    let more = lines.len().saturating_sub(top + inner_height);
    let title = match app.mode {
        Mode::Insert => " body -- INSERT ".to_string(),
        Mode::Visual => " body -- VISUAL ".to_string(),
        _ if more > 0 => format!(" body ({more} more) "),
        _ => " body ".to_string(),
    };
    let focused = app.focus == Focus::Body;
    let paragraph = Paragraph::new(shown).block(pane_block(title, focused));
    let paragraph = if app.options.wrap {
        paragraph.wrap(ratatui::widgets::Wrap { trim: false })
    } else {
        paragraph
    };
    f.render_widget(paragraph, area);

    if let Some((row, col)) = cursor {
        let x = area.x + 1 + number_width as u16 + col.min(text_width.saturating_sub(1)) as u16;
        let y = area.y + 1 + (row.saturating_sub(top)) as u16;
        f.set_cursor_position((x, y));
    }
}

fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    let mode = app.mode.label();
    let mode_style = Style::default()
        .bg(match app.mode {
            Mode::Normal => Color::Blue,
            Mode::Insert => Color::Green,
            Mode::Visual => Color::Magenta,
            Mode::Headline => Color::Magenta,
            Mode::Help => Color::Cyan,
            Mode::Confirm => Color::Red,
            Mode::Command | Mode::Search => Color::Yellow,
        })
        .fg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let base = Style::default().bg(Color::DarkGray).fg(Color::White);

    // The minibuffer owns the whole line, so the answer is where the eye is.
    //
    // It is drawn plain, with no background: this is a line being typed into,
    // not a status bar, and the status bar's colours make an input look like a
    // readout. vim draws its command line the same way.
    if let Some(mini) = &app.mini {
        let label = mini.kind.label();
        let text = truncate(&format!("{label}{}", mini.buffer), area.width as usize);
        f.render_widget(Paragraph::new(Line::from(Span::raw(text))), area);
        let x = area.x + (label.chars().count() + mini.cursor) as u16;
        f.set_cursor_position((x.min(area.x + area.width.saturating_sub(1)), area.y));
        return;
    }

    let right = if !app.message.is_empty() {
        app.message.clone()
    } else {
        let pending = app.pending_keys();
        let hint = match (pending.is_empty(), app.mode) {
            (false, _) => pending,
            (true, Mode::Help) => "q closes".to_string(),
            (true, Mode::Insert) => "Esc commits, Ctrl-c abandons".to_string(),
            (true, Mode::Visual) => "d c y > < to operate, Esc cancels".to_string(),
            _ => "F1 help".to_string(),
        };
        format!("{}  {hint}", app.status())
    };
    let mode_text = format!(" {mode} ");
    let width = area.width as usize;
    let rest = truncate(&format!(" {right}"), width.saturating_sub(mode_text.len()));
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(mode_text, mode_style),
            Span::styled(
                format!("{rest:<w$}", w = width.saturating_sub(mode.len() + 2)),
                base,
            ),
        ])),
        area,
    );
}

/// The help overlay, generated from the binding table.
fn draw_help(f: &mut Frame, app: &mut App, area: Rect) {
    let lines = help_lines(app.focus);
    // Never wider than 80, never narrower than the terminal allows.
    let w = area.width.saturating_sub(8).clamp(20, 80);
    let h = area.height.saturating_sub(4).max(6);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);
    f.render_widget(Clear, popup);

    let inner = h.saturating_sub(2) as usize;
    let max_top = lines.len().saturating_sub(inner);
    app.help_scroll = app.help_scroll.min(max_top);
    let shown: Vec<Line> = lines
        .iter()
        .skip(app.help_scroll)
        .take(inner)
        .map(|l| Line::from(truncate(l, w.saturating_sub(2) as usize)))
        .collect();
    let title = format!(
        " keys: {} pane  {}-{}/{}  q closes ",
        if app.focus == Focus::Tree {
            "outline"
        } else {
            "body"
        },
        app.help_scroll + 1,
        (app.help_scroll + inner).min(lines.len()),
        lines.len()
    );
    f.render_widget(
        Paragraph::new(shown).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Thick)
                .border_style(Style::default().fg(Color::Cyan))
                .title(title),
        ),
        popup,
    );
}

/// One line per command, with every key bound to it. The single source is the
/// binding table, so the help cannot drift from what the keys do.
pub fn help_lines(focus: Focus) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for binding in bindings::for_context(Mode::Normal, focus) {
        if seen.contains(&binding.command) {
            continue;
        }
        seen.push(binding.command);
        let keys = bindings::keys_for(binding.command);
        let keys: Vec<&&str> = keys
            .iter()
            .filter(|k| {
                BINDINGS.iter().any(|x| {
                    x.keys == **k
                        && x.command == binding.command
                        && x.mode == Mode::Normal
                        && (x.focus.is_none() || x.focus == Some(focus))
                })
            })
            .collect();
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        let summary = commands::find(binding.command)
            .map(|x| x.summary)
            .unwrap_or("");
        out.push(format!(
            "{:22} {:24} {}",
            keys.join(" "),
            binding.command,
            summary
        ));
    }
    out
}

/// Cut a line to `width` display cells, counting characters, not bytes.
fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    s.chars().take(width).collect()
}
