//! Drawing: the outline, the body, the status line and the help overlay.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use leoapp::app::{App, Focus, Mode, Servers};
use leoapp::highlight::Class;
use leoapp::palette::Palette;
use leoapp::theme::{Colour, Depth, Theme};
use leoapp::view::{self, Viewport, SCOPES};

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

    let p = colours(app);
    f.buffer_mut()
        .set_style(area, Style::default().bg(p.bg).fg(p.fg));
    draw_breadcrumb(f, app, &p, chunks[0]);
    draw_outline(f, app, &p, panes[0]);
    draw_body(f, app, &p, panes[1]);
    draw_status(f, app, &p, chunks[2]);
    if app.mode == Mode::Help {
        draw_help(f, app, &p, area);
    }
    draw_menu(f, app, &p, area);
    draw_completions(f, app, &p, area);
    if app.depth == Depth::None {
        strip_colour(f.buffer_mut());
    }
}

/// Take every colour out of a finished frame, as `NO_COLOR` asks. A cell
/// that had a background, such as the selected row, is shown reversed.
fn strip_colour(buffer: &mut ratatui::buffer::Buffer) {
    for cell in buffer.content.iter_mut() {
        if cell.bg != Color::Reset {
            cell.modifier.insert(Modifier::REVERSED);
        }
        cell.fg = Color::Reset;
        cell.bg = Color::Reset;
    }
}

/// The theme's palette (`leoapp::palette`) in the terminal's colours, as
/// leogui draws the same theme.
///
/// The theme's background is painted, as Helix does, unless the theme names
/// none, colour is off, or `terminal-background` asks for the terminal's.
/// Then the panes are the terminal's, and so is a text colour the theme
/// does not name: a grey derived for a dark background is unreadable on a
/// light terminal.
pub fn colours(app: &App) -> Palette<Color> {
    let depth = app.depth;
    let mut p = Palette::of(&app.theme, depth, false).map(|c| terminal_colour(c.reduce(depth)));
    let painted = depth != Depth::None
        && app.theme.face("ui.background").bg.is_some()
        && !app.settings.terminal_background;
    if !painted {
        (p.bg, p.panel, p.gutter) = (Color::Reset, Color::Reset, Color::Reset);
        if app.theme.face("ui.text").fg.is_none() {
            p.fg = Color::Reset;
        }
    }
    p
}

/// The path to the current node, so a deep node says where it is: its
/// ancestors dim, the node itself in the text's colour.
fn draw_breadcrumb(f: &mut Frame, app: &App, p: &Palette<Color>, area: Rect) {
    let text = format!(" {}", app.breadcrumb());
    let last = app.current.h(app.outline()).len().min(text.len());
    let (path, node) = text.split_at(text.len() - last);
    let line = Line::from(vec![
        Span::styled(path.to_string(), Style::default().fg(p.dim)),
        Span::styled(node.to_string(), Style::default().fg(p.fg)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// The border of the focused pane is thick, and its title in the text's
/// colour; the other's is plain and dim. Both are the theme's border colour.
fn pane_block(p: &Palette<Color>, title: String, focused: bool) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(p.border));
    match focused {
        true => block
            .border_type(BorderType::Thick)
            .title(Span::styled(title, Style::default().fg(p.fg))),
        false => block.title(Span::styled(title, Style::default().fg(p.dim))),
    }
}

/// The cells inside a bordered pane.
fn inner(area: Rect) -> Viewport {
    Viewport {
        rows: area.height.saturating_sub(2) as usize,
        cols: area.width.saturating_sub(2) as usize,
    }
}

fn draw_outline(f: &mut Frame, app: &mut App, p: &Palette<Color>, area: Rect) {
    let tree = app.tree_view(inner(area));
    let width = inner(area).cols;
    let focused = app.focus == Focus::Tree;

    let mut lines: Vec<Line> = Vec::new();
    let current = tree.current;
    for (k, view::TreeRow { row, hits }) in tree.rows.iter().enumerate() {
        let i = tree.top + k;
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
        let prefix = format!("{flags}{indent}{marker}");
        let text = truncate(&format!("{prefix}{}", row.headline), width);
        // As leogui: the selected row filled, an `@<file>` node's kind dim.
        let row_style = if i == current && focused {
            Style::default().bg(p.selection)
        } else if i == current {
            Style::default().bg(p.line)
        } else {
            Style::default()
        };
        let kind = match row.is_file {
            true => row.headline.find(' ').map_or(row.headline.len(), |k| k + 1),
            false => 0,
        };
        let dim_to = (prefix.len() + kind).min(text.len());
        let ink = if row.marked { p.warning } else { p.fg };
        let matches: Vec<std::ops::Range<usize>> = hits
            .iter()
            .map(|r| r.start + prefix.len()..r.end + prefix.len())
            .collect();
        let mut cells: Vec<Span> = Vec::new();
        let mut at = 0;
        for (piece, hit) in view::cut(&text, &matches) {
            let split = dim_to.clamp(at, at + piece.len()) - at;
            for (part, dim) in [(&piece[..split], true), (&piece[split..], false)] {
                if part.is_empty() {
                    continue;
                }
                let mut style = row_style.fg(if dim { p.dim } else { ink });
                if hit {
                    style = style.patch(match_style());
                }
                cells.push(Span::styled(part.to_string(), style));
            }
            at += piece.len();
        }
        lines.push(Line::from(cells));
    }

    let title = format!(" outline {}/{} ", tree.current + 1, tree.total);
    f.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(p.panel))
            .block(pane_block(p, title, focused)),
        area,
    );
}

fn draw_body(f: &mut Frame, app: &mut App, p: &Palette<Color>, area: Rect) {
    // A sign column, as leogui's gutter, while a language server can fill
    // it: shown only with diagnostics, the text would shift as they change.
    // It replaces an underline, which some terminals draw only in the
    // text's colour; `diagnostic-underline` puts it back.
    // Two cells: the dot, and a blank between it and the text.
    let signs = 2 * usize::from(app.lsp.is_some());
    let mut fits = inner(area);
    fits.cols = fits.cols.saturating_sub(signs);
    let body = app.body_view(fits);
    let palette = palette(&app.theme, app.depth);
    let selected_style = Style::default().bg(p.selection);
    let linenr = Style::default().fg(p.linenr);
    let underline = app.settings.diagnostic_underline;
    let link = link_style(app);
    let mut cells: Vec<Vec<(char, Style, usize)>> = Vec::new();
    let mut shown: Vec<Line> = Vec::new();
    for screen in body.screen_lines() {
        let i = screen.line;
        if screen.first {
            let l = &body.lines[i];
            // A selected line is shown reversed. The exact columns matter less
            // than seeing what an operator would take.
            let selected = matches!(body.selection, Some((a, b, _)) if i >= a.0 && i <= b.0);
            let base = if selected {
                selected_style
            } else {
                Style::default()
            };
            let spans = body.spans.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
            let (hits, marks) = (body.hits(i), body.marks(i));
            let pieces: Vec<(&str, Style)> = view::decorate(l, spans, &hits, &marks, body.links(i))
                .into_iter()
                .map(|seg| {
                    let mut style = style_for(seg.class, base, &palette);
                    if seg.link {
                        style = style.patch(link);
                    }
                    if let Some(s) = seg.severity.filter(|_| underline) {
                        style = style
                            .underline_color(p.mark(s).underline)
                            .add_modifier(Modifier::UNDERLINED);
                    }
                    if seg.hit {
                        style = style.patch(match_style());
                    }
                    (seg.text, style)
                })
                .collect();
            cells.push(view::expand(&pieces, body.tab));
        }
        let mut row: Vec<Span> = Vec::new();
        if signs > 0 {
            row.push(sign(&body, i, screen.first, p));
        }
        if body.number_width > 0 {
            let number = match screen.first {
                true => format!("{:>w$} ", i + 1, w = body.number_width.saturating_sub(1)),
                false => " ".repeat(body.number_width),
            };
            row.push(Span::styled(number, linenr));
        }
        let line_cells = cells.last().map(|c| c.as_slice()).unwrap_or(&[]);
        row.extend(
            view::columns(line_cells, screen.from, body.text_width)
                .into_iter()
                .map(|(text, style)| Span::styled(text, style)),
        );
        shown.push(Line::from(row));
    }
    let more = body.more();
    let title = match app.mode {
        Mode::Insert => " body -- INSERT ".to_string(),
        Mode::Visual => " body -- VISUAL ".to_string(),
        _ if more > 0 => format!(" body ({more} more) "),
        _ => " body ".to_string(),
    };
    let focused = app.focus == Focus::Body;
    f.render_widget(
        Paragraph::new(shown).block(pane_block(p, title, focused)),
        area,
    );

    if let Some((y, x)) = body.cursor_screen() {
        let x = x + signs;
        f.set_cursor_position((area.x + 1 + x as u16, area.y + 1 + y as u16));
    }
}

/// Line `i`'s sign: a dot in the colour of its worst diagnostic, on the
/// line's first screen row.
fn sign(body: &view::BodyView, i: usize, first: bool, p: &Palette<Color>) -> Span<'static> {
    let worst = body
        .diagnostics
        .iter()
        .filter(|d| first && d.row <= i && i <= d.end_row)
        .map(|d| d.severity)
        .min();
    let style = Style::default().bg(p.gutter);
    match worst {
        Some(s) => Span::styled("\u{25cf} ", style.fg(p.mark(s).gutter)),
        None => Span::styled("  ", style),
    }
}

fn draw_status(f: &mut Frame, app: &mut App, p: &Palette<Color>, area: Rect) {
    let mode = app.mode.label();
    let (badge, badge_text) = p.mode(app.mode);
    let mode_style = Style::default()
        .bg(badge)
        .fg(badge_text)
        .add_modifier(Modifier::BOLD);
    let base = Style::default().bg(p.statusbar).fg(p.statusbar_text);

    // The minibuffer owns the whole line, so the answer is where the eye is.
    //
    // It is drawn plain, with no background: this is a line being typed into,
    // not a status bar, and the status bar's colours make an input look like a
    // readout. vim draws its command line the same way.
    if let Some(mini) = &app.mini {
        let label = app.mini_label();
        let text = truncate(&format!("{label}{}", mini.buffer), area.width as usize);
        // A selected text is shown reversed: typing replaces it.
        let line = match mini.selected_all {
            true => {
                let n = label.chars().count().min(text.chars().count());
                let (head, tail) =
                    text.split_at(text.char_indices().nth(n).map_or(text.len(), |(i, _)| i));
                Line::from(vec![
                    Span::raw(head.to_string()),
                    Span::styled(
                        tail.to_string(),
                        Style::default().add_modifier(Modifier::REVERSED),
                    ),
                ])
            }
            false => Line::from(Span::raw(text)),
        };
        f.render_widget(Paragraph::new(line), area);
        let x = area.x + (label.chars().count() + mini.cursor) as u16;
        f.set_cursor_position((x.min(area.x + area.width.saturating_sub(1)), area.y));
        return;
    }

    let right = app.status_text();
    let mode_text = format!(" {mode} ");
    let width = area.width as usize;
    let servers = lsp_spans(app, p, base);
    let servers_width: usize = servers.iter().map(|s| s.content.chars().count()).sum();
    let room = width.saturating_sub(mode_text.chars().count() + servers_width);
    let rest = truncate(&format!(" {right}"), room);
    let mut spans = vec![
        Span::styled(mode_text, mode_style),
        Span::styled(format!("{rest:<room$}"), base),
    ];
    spans.extend(servers);
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The right end of the status bar while a language server is attached, as
/// leogui's: the body's error and warning counts, then "LSP" and a dot lit
/// while a server runs, in the error colour if one failed.
fn lsp_spans(app: &App, p: &Palette<Color>, base: Style) -> Vec<Span<'static>> {
    let Some(state) = app.servers() else {
        return Vec::new();
    };
    let (errors, warnings) = app.diagnostic_counts();
    let error = p.mark(view::Severity::Error).gutter;
    let counts = base.fg(if errors > 0 { error } else { p.dim });
    let dot = match state {
        Servers::Failed => error,
        Servers::Running => p.accent,
        Servers::Idle => p.dim,
    };
    vec![
        Span::styled(format!(" E {errors}  W {warnings}  "), counts),
        Span::styled("LSP ", base.fg(p.dim)),
        Span::styled("\u{25cf} ", base.fg(dot)),
    ]
}

/// The help overlay, generated from the binding table.
fn draw_help(f: &mut Frame, app: &mut App, p: &Palette<Color>, area: Rect) {
    // Never wider than 80, never narrower than the terminal allows.
    let w = area.width.saturating_sub(8).clamp(20, 80);
    let h = area.height.saturating_sub(4).max(6);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup = Rect::new(x, y, w, h);
    f.render_widget(Clear, popup);

    let inner = h.saturating_sub(2) as usize;
    let help = app.help_view(inner, w.saturating_sub(2) as usize);
    let shown: Vec<Line> = help
        .lines
        .iter()
        .map(|l| Line::from(truncate(l, w.saturating_sub(2) as usize)))
        .collect();
    let title = format!(
        " {}  {}-{}/{}  q closes ",
        help.name,
        help.first + 1,
        (help.first + inner).min(help.total),
        help.total
    );
    f.render_widget(
        Paragraph::new(shown)
            .style(Style::default().bg(p.help).fg(p.fg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Thick)
                    .border_style(Style::default().fg(p.border))
                    .title(title),
            ),
        popup,
    );
}

/// A search match: vim's default `Search` colours.
/// A plugin's link: the theme's link colour, else light blue, underlined.
fn link_style(app: &App) -> Style {
    let fg = app.theme.face(view::LINK_SCOPE).fg;
    Style::default()
        .fg(fg.map_or(Color::LightBlue, |c| terminal_colour(c.reduce(app.depth))))
        .add_modifier(Modifier::UNDERLINED)
}

fn match_style() -> Style {
    Style::default().bg(Color::Yellow).fg(Color::Black)
}

/// The completion drop-down, above the `:` line.
///
/// See `Menu::is_open` for when it shows.
fn draw_menu(f: &mut Frame, app: &App, p: &Palette<Color>, area: Rect) {
    let Some(mini) = &app.mini else {
        return;
    };
    let menu = mini.menu(&app.theme_names);
    if !menu.is_open() || area.height < 3 {
        return;
    }

    // Columns wide enough for the longest name, as many as the width allows.
    let width = menu
        .items
        .iter()
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(1)
        + 2;
    let inner_width = area.width.saturating_sub(2) as usize;
    let columns = (inner_width / width).max(1);
    let rows = menu.items.len().div_ceil(columns);
    let height = (rows as u16 + 2).min(area.height.saturating_sub(1)).min(12);
    let visible = height.saturating_sub(2) as usize;

    // Scroll by whole columns, so the selected name is on screen.
    let first = match menu.selected {
        Some(i) if visible > 0 => (i % rows) / visible * visible,
        _ => 0,
    };

    let lines: Vec<Line> = (first..(first + visible).min(rows))
        .map(|row| {
            let mut cells: Vec<Span> = Vec::new();
            for column in 0..columns {
                let Some(name) = menu.items.get(column * rows + row) else {
                    continue;
                };
                let text = format!("{name:<width$}");
                let style = match menu.selected == Some(column * rows + row) {
                    true => Style::default()
                        .bg(p.menu_selected)
                        .fg(p.menu_selected_text),
                    false => Style::default(),
                };
                cells.push(Span::styled(text, style));
            }
            Line::from(cells)
        })
        .collect();

    let popup = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1 + height),
        width: area.width,
        height,
    };
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).style(menu_style(p)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(p.border))
                .title(match menu.at {
                    0 => format!(" {} commands ", menu.items.len()),
                    _ => format!(" {} themes ", menu.items.len()),
                }),
        ),
        popup,
    );
}

/// The language server's completions, a drop-down above the status line,
/// the selected one lit and kept on screen.
fn draw_completions(f: &mut Frame, app: &App, p: &Palette<Color>, area: Rect) {
    let Some(menu) = &app.completion else {
        return;
    };
    if app.mode != Mode::Insert || area.height < 4 {
        return;
    }
    let height = (menu.shown.len() as u16 + 2)
        .min(12)
        .min(area.height.saturating_sub(1));
    let visible = height.saturating_sub(2) as usize;
    let first = (menu.selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = (first..menu.shown.len().min(first + visible))
        .filter_map(|i| {
            let c = menu.shown_item(i)?;
            let style = match i == menu.selected {
                true => Style::default()
                    .bg(p.menu_selected)
                    .fg(p.menu_selected_text),
                false => Style::default(),
            };
            let mut spans = vec![Span::styled(c.label.clone(), style)];
            if let Some(detail) = &c.detail {
                let detail: String = detail.chars().take(60).collect();
                spans.push(Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(p.dim),
                ));
            }
            Some(Line::from(spans))
        })
        .collect();
    let popup = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1 + height),
        width: area.width,
        height,
    };
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).style(menu_style(p)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(p.border))
                .title(format!(" {} completions; Tab takes one ", menu.shown.len())),
        ),
        popup,
    );
}

/// A drop-down's colours: `ui.menu`, as leogui's.
fn menu_style(p: &Palette<Color>) -> Style {
    Style::default().bg(p.menu).fg(p.fg)
}

/// Every class's style, resolved once for the frame.
///
/// Resolving a scope walks its prefixes, and reducing a colour searches 240
/// candidates in CIELAB. A body holds thousands of runs and neither answer
/// changes between them.
fn palette(theme: &Theme, depth: Depth) -> Vec<(Class, Style)> {
    SCOPES
        .iter()
        .map(|&(class, scope)| {
            let face = theme.face(scope);
            let mut style = Style::default();
            if let Some(colour) = face.fg {
                style = style.fg(terminal_colour(colour.reduce(depth)));
            }
            if face.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if face.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            (class, style)
        })
        .collect()
}

/// The style for one class, over whatever the line's own style is.
///
/// A class the theme says nothing about is left plain rather than guessed at.
fn style_for(class: Class, base: Style, palette: &[(Class, Style)]) -> Style {
    match palette.iter().find(|(c, _)| *c == class) {
        Some((_, style)) => base.patch(*style),
        None => base,
    }
}

/// A reduced colour as ratatui names it.
///
/// The sixteen keep their names rather than becoming `Indexed`, so the
/// terminal's own palette decides them.
fn terminal_colour(colour: Colour) -> Color {
    let n = match colour {
        Colour::Rgb(r, g, b) => return Color::Rgb(r, g, b),
        Colour::Ansi(n) => n,
    };
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        n => Color::Indexed(n),
    }
}

/// Cut a line to `width` display cells, counting characters, not bytes.
fn truncate(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    s.chars().take(width).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_terminals_own_sixteen_keep_their_names() {
        // `Indexed(3)` and `Yellow` paint the same cell, but only the name
        // follows a terminal whose palette has been changed.
        assert_eq!(terminal_colour(Colour::Ansi(3)), Color::Yellow);
        assert_eq!(terminal_colour(Colour::Ansi(8)), Color::DarkGray);
        assert_eq!(terminal_colour(Colour::Ansi(200)), Color::Indexed(200));
        assert_eq!(terminal_colour(Colour::Rgb(1, 2, 3)), Color::Rgb(1, 2, 3));
    }

    #[test]
    fn a_reduced_theme_reaches_the_style() {
        let palette = palette(&Theme::builtin(), Depth::Ansi16);
        let base = Style::default();
        assert_eq!(
            style_for(Class::Keyword, base, &palette).fg,
            Some(Color::Yellow)
        );
        assert_eq!(style_for(Class::Plain, base, &palette).fg, None);
        assert!(style_for(Class::Directive, base, &palette)
            .add_modifier
            .contains(Modifier::BOLD));
    }
}
