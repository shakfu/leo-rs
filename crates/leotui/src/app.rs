//! The terminal view's state and its key bindings.
//!
//! Everything that changes the outline goes through `leolib::Document`, so an
//! edit lands in the model and its undo history, never in a widget that the
//! model then has to be told about. That is the whole reason the model was
//! separated from the view: this file holds no copy of the outline.

use leolib::{Document, Outline, Position};

/// A one-line prompt at the bottom of the screen.
pub struct Prompt {
    pub label: String,
    pub buffer: String,
    pub cursor: usize,
    pub kind: PromptKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    Headline,
    SaveAs,
    ConfirmQuit,
}

/// A minimal multi-line editor for one node's body.
pub struct BodyEditor {
    pub lines: Vec<String>,
    pub row: usize,
    pub col: usize,
    pub position: Position,
}

pub enum Mode {
    Normal,
    Prompt(Prompt),
    EditBody(BodyEditor),
}

pub struct App {
    pub doc: Document,
    pub current: Position,
    pub mode: Mode,
    /// First visible row of the outline pane.
    pub top: usize,
    pub body_scroll: usize,
    pub message: String,
    pub quit: bool,
    /// Total positions, and the outline generation it was counted at.
    /// Counting is O(outline), and the status line asks on every keystroke.
    position_count: (u64, usize),
}

/// One row of the outline pane.
pub struct Row {
    pub position: Position,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub marked: bool,
    pub dirty: bool,
    pub cloned: bool,
    pub headline: String,
}

impl App {
    pub fn new(doc: Document) -> Self {
        let current = doc
            .outline
            .root_position()
            .expect("an outline always has a root");
        let mut app = Self {
            doc,
            current,
            mode: Mode::Normal,
            top: 0,
            body_scroll: 0,
            message: String::new(),
            quit: false,
            position_count: (u64::MAX, 0),
        };
        app.expand_ancestors();
        app
    }

    pub fn outline(&self) -> &Outline {
        &self.doc.outline
    }

    /// The rows the outline pane shows: the tree with folded subtrees skipped.
    pub fn rows(&self) -> Vec<Row> {
        let o = self.outline();
        let mut rows = Vec::new();
        let mut p = o.root_position();
        while let Some(cur) = p {
            let has_children = cur.has_children(o);
            let expanded = o.is_expanded(&cur);
            rows.push(Row {
                depth: cur.level(),
                has_children,
                expanded,
                marked: cur.is_marked(o),
                dirty: cur.is_dirty(o),
                cloned: cur.is_cloned(o),
                headline: cur.h(o).to_string(),
                position: cur.clone(),
            });
            p = if has_children && expanded {
                cur.thread_next(o)
            } else {
                cur.node_after_tree(o)
            };
        }
        rows
    }

    pub fn current_row(&self) -> usize {
        row_of(&self.rows(), &self.current)
    }

    /// The visible rows and the index of the selected one, in one walk.
    pub fn rows_and_current(&self) -> (Vec<Row>, usize) {
        let rows = self.rows();
        let current = row_of(&rows, &self.current);
        (rows, current)
    }

    fn position_count(&mut self) -> usize {
        let generation = self.outline().generation;
        if self.position_count.0 != generation {
            self.position_count = (generation, self.outline().all_positions().len());
        }
        self.position_count.1
    }

    pub fn body_lines(&self) -> Vec<String> {
        self.current
            .b(self.outline())
            .split('\n')
            .map(String::from)
            .collect()
    }

    /// Unfold everything above the current node, so it can be seen.
    pub fn expand_ancestors(&mut self) {
        for p in self.current.clone().parents(self.outline()) {
            self.doc.outline.expand(&p);
        }
    }

    fn select(&mut self, p: Position) {
        self.current = p;
        self.body_scroll = 0;
        self.expand_ancestors();
    }

    /// Move `delta` visible rows.
    pub fn move_by(&mut self, delta: i32) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let i = self.current_row() as i32 + delta;
        let i = i.clamp(0, rows.len() as i32 - 1) as usize;
        self.select(rows[i].position.clone());
    }

    pub fn move_to_row(&mut self, i: usize) {
        let rows = self.rows();
        if let Some(row) = rows.get(i.min(rows.len().saturating_sub(1))) {
            self.select(row.position.clone());
        }
    }

    pub fn toggle_fold(&mut self) {
        let p = self.current.clone();
        if !p.has_children(self.outline()) {
            return;
        }
        if self.outline().is_expanded(&p) {
            self.doc.outline.contract(&p);
        } else {
            self.doc.outline.expand(&p);
        }
    }

    /// Right arrow: unfold, or step into the first child.
    pub fn expand_or_descend(&mut self) {
        let p = self.current.clone();
        if p.has_children(self.outline()) {
            if self.outline().is_expanded(&p) {
                if let Some(child) = p.first_child(self.outline()) {
                    self.select(child);
                }
            } else {
                self.doc.outline.expand(&p);
            }
        }
    }

    /// Left arrow: fold, or step out to the parent.
    pub fn collapse_or_ascend(&mut self) {
        let p = self.current.clone();
        if p.has_children(self.outline()) && self.outline().is_expanded(&p) {
            self.doc.outline.contract(&p);
        } else if let Some(parent) = p.parent(self.outline()) {
            self.select(parent);
        }
    }

    // --- Commands ---------------------------------------------------------

    pub fn insert_node(&mut self) {
        let p = self.current.clone();
        let new = self.doc.insert_node(&p);
        self.select(new);
        self.begin_headline_prompt();
    }

    pub fn delete_node(&mut self) {
        let p = self.current.clone();
        match self.doc.delete_node(&p) {
            Some(next) => self.select(next),
            None => self.message = "cannot delete the last node".to_string(),
        }
    }

    pub fn clone_node(&mut self) {
        let p = self.current.clone();
        let new = self.doc.clone_node(&p);
        self.select(new);
    }

    pub fn copy_node(&mut self) {
        let p = self.current.clone();
        self.doc.copy_node(&p);
        self.message = format!("copied: {}", p.h(self.outline()));
    }

    pub fn paste_node(&mut self) {
        let p = self.current.clone();
        match self.doc.paste_node(&p) {
            Some(new) => self.select(new),
            None => self.message = "nothing copied".to_string(),
        }
    }

    pub fn toggle_mark(&mut self) {
        let p = self.current.clone();
        self.doc.toggle_marked(&p);
    }

    pub fn move_node(&mut self, direction: char) {
        let p = self.current.clone();
        let moved = match direction {
            'u' => self.doc.move_up(&p),
            'd' => self.doc.move_down(&p),
            'l' => self.doc.move_left(&p),
            'r' => self.doc.move_right(&p),
            _ => None,
        };
        match moved {
            Some(new) => self.select(new),
            None => self.message = "cannot move that way".to_string(),
        }
    }

    pub fn undo(&mut self) {
        let name = self.doc.undoer.undo_name().unwrap_or("nothing").to_string();
        match self.doc.undo() {
            Some(p) => {
                self.select(p);
                self.message = format!("undo: {name}");
            }
            None => self.message = "nothing to undo".to_string(),
        }
        self.clamp_current();
    }

    pub fn redo(&mut self) {
        let name = self.doc.undoer.redo_name().unwrap_or("nothing").to_string();
        match self.doc.redo() {
            Some(p) => {
                self.select(p);
                self.message = format!("redo: {name}");
            }
            None => self.message = "nothing to redo".to_string(),
        }
        self.clamp_current();
    }

    /// After an undo the current position may no longer exist.
    fn clamp_current(&mut self) {
        if !self.outline().position_exists(&self.current) {
            if let Some(root) = self.outline().root_position() {
                self.current = root;
            }
        }
    }

    pub fn save(&mut self) {
        if self.outline().file_name.is_empty() {
            self.mode = Mode::Prompt(Prompt {
                label: "save as: ".to_string(),
                buffer: String::new(),
                cursor: 0,
                kind: PromptKind::SaveAs,
            });
            return;
        }
        match self.doc.save("") {
            Ok(path) => self.message = format!("saved: {path}"),
            Err(e) => self.message = format!("save failed: {e}"),
        }
    }

    /// Write the outline's external files. Only dirty trees, as Leo does.
    pub fn write_external(&mut self) {
        let result = self.doc.write_external_files(true);
        let mut parts = vec![format!("wrote {}", result.written.len())];
        if result.unchanged > 0 {
            parts.push(format!("{} unchanged", result.unchanged));
        }
        if !result.errors.is_empty() {
            parts.push(format!(
                "{} failed: {}",
                result.errors.len(),
                result.errors[0].message
            ));
        }
        self.message = parts.join(", ");
    }

    // --- Prompts ----------------------------------------------------------

    pub fn begin_headline_prompt(&mut self) {
        let text = self.current.h(self.outline()).to_string();
        self.mode = Mode::Prompt(Prompt {
            label: "headline: ".to_string(),
            cursor: text.chars().count(),
            buffer: text,
            kind: PromptKind::Headline,
        });
    }

    pub fn begin_body_edit(&mut self) {
        let mut lines = self.body_lines();
        if lines.is_empty() {
            lines.push(String::new());
        }
        self.mode = Mode::EditBody(BodyEditor {
            lines,
            row: 0,
            col: 0,
            position: self.current.clone(),
        });
    }

    pub fn finish_prompt(&mut self, accepted: bool) {
        let Mode::Prompt(prompt) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        if !accepted {
            return;
        }
        match prompt.kind {
            PromptKind::Headline => {
                let p = self.current.clone();
                self.doc.set_headline(&p, &prompt.buffer);
            }
            PromptKind::SaveAs => match self.doc.save(&prompt.buffer) {
                Ok(path) => self.message = format!("saved: {path}"),
                Err(e) => self.message = format!("save failed: {e}"),
            },
            PromptKind::ConfirmQuit => {
                if prompt.buffer.trim().eq_ignore_ascii_case("y") {
                    self.quit = true;
                }
            }
        }
    }

    pub fn finish_body_edit(&mut self, accepted: bool) {
        let Mode::EditBody(editor) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return;
        };
        if !accepted {
            return;
        }
        let text = editor.lines.join("\n");
        self.doc.set_body(&editor.position, &text);
    }

    pub fn request_quit(&mut self) {
        if !self.outline().changed {
            self.quit = true;
            return;
        }
        self.mode = Mode::Prompt(Prompt {
            label: "unsaved changes. quit anyway? (y/n) ".to_string(),
            buffer: String::new(),
            cursor: 0,
            kind: PromptKind::ConfirmQuit,
        });
    }

    /// The status line: what the outline is and what state it is in.
    pub fn status(&mut self) -> String {
        let positions = self.position_count();
        let (rows, current) = self.rows_and_current();
        let o = self.outline();
        let name = if o.file_name.is_empty() {
            "<unsaved>".to_string()
        } else {
            leolib::util::short_file_name(&o.file_name)
        };
        let changed = if o.changed { " *" } else { "" };
        format!(
            " {name}{changed}  row {}/{}  {positions} positions ",
            current + 1,
            rows.len()
        )
    }
}

/// The index of `current` among `rows`, or 0 if it is not shown.
fn row_of(rows: &[Row], current: &Position) -> usize {
    rows.iter()
        .position(|r| r.position == *current)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut doc = Document::new_empty("");
        let root = doc.outline.root_position().unwrap();
        doc.set_headline(&root, "root");
        let a = doc.outline.insert_as_last_child(&root);
        doc.set_headline(&a, "a");
        let a1 = doc.outline.insert_as_last_child(&a);
        doc.set_headline(&a1, "a1");
        let b = doc.outline.insert_after(&root);
        doc.set_headline(&b, "b");
        doc.undoer.clear();
        App::new(doc)
    }

    #[test]
    fn a_folded_subtree_is_not_shown() {
        let mut app = app();
        let heads: Vec<String> = app.rows().iter().map(|r| r.headline.clone()).collect();
        assert_eq!(heads, vec!["root", "b"]);
        app.expand_or_descend();
        let heads: Vec<String> = app.rows().iter().map(|r| r.headline.clone()).collect();
        assert_eq!(heads, vec!["root", "a", "b"]);
    }

    #[test]
    fn navigation_skips_folded_nodes() {
        let mut app = app();
        app.move_by(1);
        assert_eq!(app.current.h(app.outline()), "b");
    }

    #[test]
    fn selecting_a_node_unfolds_its_ancestors() {
        let mut app = app();
        let a1 = app.outline().all_positions()[2].clone();
        app.select(a1);
        let heads: Vec<String> = app.rows().iter().map(|r| r.headline.clone()).collect();
        assert_eq!(heads, vec!["root", "a", "a1", "b"]);
    }

    #[test]
    fn undo_after_a_delete_restores_the_selection() {
        let mut app = app();
        app.move_by(1);
        app.delete_node();
        assert_eq!(app.rows().len(), 1);
        app.undo();
        assert_eq!(app.rows().len(), 2);
        assert_eq!(app.current.h(app.outline()), "b");
    }
}
