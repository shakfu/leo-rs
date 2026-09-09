//! Key handling for each mode.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Mode};

pub fn handle(app: &mut App, key: KeyEvent) {
    app.message.clear();
    match &mut app.mode {
        Mode::Normal => normal(app, key),
        Mode::Prompt(_) => prompt(app, key),
        Mode::EditBody(_) => body(app, key),
    }
}

fn normal(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('q') => app.request_quit(),
        KeyCode::Char('c') if ctrl => app.request_quit(),
        KeyCode::Char('j') | KeyCode::Down => app.move_by(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_by(-1),
        KeyCode::PageDown => app.move_by(10),
        KeyCode::PageUp => app.move_by(-10),
        KeyCode::Home => app.move_to_row(0),
        KeyCode::End => app.move_to_row(usize::MAX),
        KeyCode::Char(' ') | KeyCode::Enter => app.toggle_fold(),
        KeyCode::Right => app.expand_or_descend(),
        KeyCode::Left => app.collapse_or_ascend(),
        KeyCode::Char('e') => app.begin_headline_prompt(),
        KeyCode::Char('i') => app.begin_body_edit(),
        KeyCode::Char('o') => app.insert_node(),
        KeyCode::Char('D') => app.delete_node(),
        KeyCode::Char('u') => app.undo(),
        KeyCode::Char('r') => app.redo(),
        KeyCode::Char('K') => app.move_node('u'),
        KeyCode::Char('J') => app.move_node('d'),
        KeyCode::Char('<') => app.move_node('l'),
        KeyCode::Char('>') => app.move_node('r'),
        KeyCode::Char('m') => app.toggle_mark(),
        KeyCode::Char('c') => app.clone_node(),
        KeyCode::Char('y') => app.copy_node(),
        KeyCode::Char('P') => app.paste_node(),
        KeyCode::Char('s') => app.save(),
        KeyCode::Char('w') => app.write_external(),
        KeyCode::Char('n') => app.body_scroll += 1,
        KeyCode::Char('p') => app.body_scroll = app.body_scroll.saturating_sub(1),
        _ => {}
    }
}

fn prompt(app: &mut App, key: KeyEvent) {
    let Mode::Prompt(prompt) = &mut app.mode else {
        return;
    };
    match key.code {
        KeyCode::Esc => app.finish_prompt(false),
        KeyCode::Enter => app.finish_prompt(true),
        KeyCode::Backspace => {
            if prompt.cursor > 0 {
                let i = char_index(&prompt.buffer, prompt.cursor - 1);
                prompt.buffer.remove(i);
                prompt.cursor -= 1;
            }
        }
        KeyCode::Delete => {
            if prompt.cursor < prompt.buffer.chars().count() {
                let i = char_index(&prompt.buffer, prompt.cursor);
                prompt.buffer.remove(i);
            }
        }
        KeyCode::Left => prompt.cursor = prompt.cursor.saturating_sub(1),
        KeyCode::Right => prompt.cursor = (prompt.cursor + 1).min(prompt.buffer.chars().count()),
        KeyCode::Home => prompt.cursor = 0,
        KeyCode::End => prompt.cursor = prompt.buffer.chars().count(),
        KeyCode::Char(ch) => {
            let i = char_index(&prompt.buffer, prompt.cursor);
            prompt.buffer.insert(i, ch);
            prompt.cursor += 1;
        }
        _ => {}
    }
}

fn body(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if key.code == KeyCode::Esc {
        app.finish_body_edit(false);
        return;
    }
    if ctrl && key.code == KeyCode::Char('s') {
        app.finish_body_edit(true);
        return;
    }
    let Mode::EditBody(ed) = &mut app.mode else {
        return;
    };
    match key.code {
        KeyCode::Enter => {
            let i = char_index(&ed.lines[ed.row], ed.col);
            let rest = ed.lines[ed.row].split_off(i);
            ed.lines.insert(ed.row + 1, rest);
            ed.row += 1;
            ed.col = 0;
        }
        KeyCode::Backspace => {
            if ed.col > 0 {
                let i = char_index(&ed.lines[ed.row], ed.col - 1);
                ed.lines[ed.row].remove(i);
                ed.col -= 1;
            } else if ed.row > 0 {
                let line = ed.lines.remove(ed.row);
                ed.row -= 1;
                ed.col = ed.lines[ed.row].chars().count();
                ed.lines[ed.row].push_str(&line);
            }
        }
        KeyCode::Delete => {
            let len = ed.lines[ed.row].chars().count();
            if ed.col < len {
                let i = char_index(&ed.lines[ed.row], ed.col);
                ed.lines[ed.row].remove(i);
            } else if ed.row + 1 < ed.lines.len() {
                let next = ed.lines.remove(ed.row + 1);
                ed.lines[ed.row].push_str(&next);
            }
        }
        KeyCode::Up => {
            ed.row = ed.row.saturating_sub(1);
            ed.col = ed.col.min(ed.lines[ed.row].chars().count());
        }
        KeyCode::Down => {
            ed.row = (ed.row + 1).min(ed.lines.len() - 1);
            ed.col = ed.col.min(ed.lines[ed.row].chars().count());
        }
        KeyCode::Left => ed.col = ed.col.saturating_sub(1),
        KeyCode::Right => ed.col = (ed.col + 1).min(ed.lines[ed.row].chars().count()),
        KeyCode::Home => ed.col = 0,
        KeyCode::End => ed.col = ed.lines[ed.row].chars().count(),
        KeyCode::Tab => {
            let i = char_index(&ed.lines[ed.row], ed.col);
            ed.lines[ed.row].insert_str(i, "    ");
            ed.col += 4;
        }
        KeyCode::Char(ch) => {
            let i = char_index(&ed.lines[ed.row], ed.col);
            ed.lines[ed.row].insert(i, ch);
            ed.col += 1;
        }
        _ => {}
    }
}

/// The byte offset of character `n`, so editing works on non-ASCII text.
fn char_index(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use leolib::Document;

    fn press(app: &mut App, code: KeyCode) {
        handle(app, KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn type_text(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch));
        }
    }

    fn app() -> App {
        let mut doc = Document::new_empty("");
        let root = doc.outline.root_position().unwrap();
        doc.set_headline(&root, "root");
        doc.undoer.clear();
        App::new(doc)
    }

    #[test]
    fn typing_a_headline_lands_in_the_model() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));
        for _ in 0..4 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "renamed");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.current.h(app.outline()), "renamed");
        assert!(app.doc.undoer.can_undo());
    }

    #[test]
    fn escaping_a_prompt_changes_nothing() {
        let mut app = app();
        press(&mut app, KeyCode::Char('e'));
        type_text(&mut app, "xyz");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.current.h(app.outline()), "root");
        assert!(!app.doc.undoer.can_undo());
    }

    #[test]
    fn the_body_editor_commits_only_on_ctrl_s() {
        let mut app = app();
        press(&mut app, KeyCode::Char('i'));
        type_text(&mut app, "line one");
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "line two");
        assert_eq!(app.current.b(app.outline()), "");
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.current.b(app.outline()), "line one\nline two");
    }

    #[test]
    fn the_body_editor_edits_non_ascii_text_by_character() {
        let mut app = app();
        press(&mut app, KeyCode::Char('i'));
        type_text(&mut app, "na\u{ef}ve");
        press(&mut app, KeyCode::Backspace);
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.current.b(app.outline()), "na\u{ef}v");
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_first() {
        let mut app = app();
        press(&mut app, KeyCode::Char('q'));
        assert!(!app.quit);
        press(&mut app, KeyCode::Char('n'));
        press(&mut app, KeyCode::Enter);
        assert!(!app.quit);
        press(&mut app, KeyCode::Char('q'));
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Enter);
        assert!(app.quit);
    }

    #[test]
    fn inserting_a_node_opens_the_headline_prompt() {
        let mut app = app();
        press(&mut app, KeyCode::Char('o'));
        type_text(&mut app, "new node");
        press(&mut app, KeyCode::Enter);
        let heads: Vec<String> = app.rows().iter().map(|r| r.headline.clone()).collect();
        assert_eq!(heads, vec!["root", "new node"]);
    }
}
