//! Drawing the outline, the body and the status line.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};

pub const HELP: &str =
    " j/k move  space fold  e head  i body  o ins  D del  u/r undo  KJ<> move  m mark  c clone  y/P copy  s save  w write  q quit ";

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[0]);

    draw_outline(f, app, panes[0]);
    draw_body(f, app, panes[1]);
    draw_status(f, app, chunks[1]);
}

fn draw_outline(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_height = area.height.saturating_sub(2) as usize;
    let (rows, current) = app.rows_and_current();
    // Keep the selected row on screen without recentring on every keypress.
    if current < app.top {
        app.top = current;
    } else if inner_height > 0 && current >= app.top + inner_height {
        app.top = current + 1 - inner_height;
    }
    let width = area.width.saturating_sub(2) as usize;

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
        let text = format!("{flags}{indent}{marker}{}", row.headline);
        let text = truncate(&text, width);
        let style = if i == current {
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else if row.marked {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(text, style)));
    }

    let title = format!(" outline ({} nodes) ", rows.len());
    let block = Block::default().borders(Borders::ALL).title(title);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_body(f: &mut Frame, app: &mut App, area: Rect) {
    let (lines, cursor, title) = match &app.mode {
        Mode::EditBody(editor) => (
            editor.lines.clone(),
            Some((editor.row, editor.col)),
            " body (^S save, ESC cancel) ".to_string(),
        ),
        _ => (app.body_lines(), None, " body ".to_string()),
    };
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;

    // Scroll to the cursor while editing, and by hand otherwise.
    let top = match cursor {
        Some((row, _)) => {
            if inner_height == 0 {
                0
            } else if row < app.body_scroll {
                row
            } else if row >= app.body_scroll + inner_height {
                row + 1 - inner_height
            } else {
                app.body_scroll
            }
        }
        None => app.body_scroll.min(lines.len().saturating_sub(1)),
    };
    app.body_scroll = top;

    let shown: Vec<Line> = lines
        .iter()
        .skip(top)
        .take(inner_height)
        .map(|l| Line::from(truncate(&l.replace('\t', "    "), inner_width)))
        .collect();
    let block = Block::default().borders(Borders::ALL).title(title);
    f.render_widget(Paragraph::new(shown).block(block), area);

    if let Some((row, col)) = cursor {
        let x = area.x + 1 + col.min(inner_width.saturating_sub(1)) as u16;
        let y = area.y + 1 + (row.saturating_sub(top)) as u16;
        f.set_cursor_position((x, y));
    }
}

fn draw_status(f: &mut Frame, app: &mut App, area: Rect) {
    let style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let text = match &app.mode {
        Mode::Prompt(prompt) => format!("{}{}", prompt.label, prompt.buffer),
        _ if !app.message.is_empty() => format!(" {} ", app.message),
        _ => format!("{}{HELP}", app.status()),
    };
    let text = truncate(&text, area.width as usize);
    f.render_widget(Paragraph::new(Line::from(Span::styled(text, style))), area);
    if let Mode::Prompt(prompt) = &app.mode {
        let x = area.x + (prompt.label.chars().count() + prompt.cursor) as u16;
        f.set_cursor_position((x.min(area.x + area.width - 1), area.y));
    }
}

/// Cut a line to `width` display cells, counting characters, not bytes.
fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= width {
            break;
        }
        out.push(ch);
    }
    out
}
