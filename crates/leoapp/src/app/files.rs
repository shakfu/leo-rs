//! Saving, writing, reading and quitting: everything that touches disk.

use super::*;

impl App {
    /// Leo's `save`: the `.leo` file, then every dirty external file.
    ///
    /// The `.leo` file goes first, so the outline's edits reach disk however
    /// the files fare. A file that fails stays dirty for the next save and
    /// does not stop the others.
    pub fn save(&mut self) {
        self.request_save(true);
    }

    /// Leo's `write-outline-only`: the `.leo` file and nothing else.
    pub fn write_outline_only(&mut self) {
        if self.outline().file_name.is_empty() {
            self.message = "write-outline-only: no file name; use :saveas path".to_string();
            return;
        }
        self.request_save(false);
    }

    fn request_save(&mut self, files: bool) {
        if self.outline().file_name.is_empty() {
            self.open_mini(MiniKind::SaveAs, String::new());
            return;
        }
        self.save_files = files;
        if self.outline().changed_on_disk(&self.outline().file_name) {
            self.open_mini(MiniKind::ConfirmSave, String::new());
            return;
        }
        self.save_now(true);
    }

    /// Save the `.leo` file unless the user declined to overwrite it, then the
    /// external files unless this is `write-outline-only`.
    ///
    /// A `.leo` file that is not saved, by failure or by refusal, holds back
    /// every external file, as `leolib::save_all` does.
    pub(super) fn save_now(&mut self, leo: bool) {
        let name = leolib::util::short_file_name(&self.outline().file_name);
        let leo = match leo {
            true if self.save_files => {
                let result = self.doc.save_all("");
                match result.leo {
                    Ok(_) => {
                        self.report_save(format!("saved {name}"), result.files);
                        return self.note_dropped_uas(result.dropped_descendent_uas);
                    }
                    Err(e) => format!("save failed: {e}"),
                }
            }
            true => match self.doc.save("") {
                Ok(_) => return self.message = format!("saved {name}"),
                Err(e) => format!("save failed: {e}"),
            },
            false => format!("not saved: {name}"),
        };
        self.message = match self.save_files {
            true => self.held_back(leo),
            false => leo,
        };
    }

    /// `message`, naming the dirty external files a failed save did not write.
    pub(super) fn held_back(&self, message: String) -> String {
        match leolib::external::find_files_to_write(self.outline(), true)
            .0
            .len()
        {
            0 => message,
            n => format!("{message}; {} not written", plural(n, "external file")),
        }
    }

    /// Name the trees whose descendants' unknown attributes the save dropped.
    ///
    /// The blob that held them is keyed by position and only Leo can rebuild
    /// it, so restructuring such a tree loses them. The save is the one place
    /// that can say so: nothing in the outline records it afterwards.
    pub(super) fn note_dropped_uas(&mut self, dropped: Vec<String>) {
        let note = match dropped.len() {
            0 => return,
            1 => format!("dropped the descendant uAs under {}", dropped[0]),
            n => format!("dropped the descendant uAs under {n} trees"),
        };
        self.message = format!("{}; {note}", self.message);
    }

    /// Say what saving the `.leo` file did, then what writing the files did.
    pub(super) fn report_save(&mut self, leo: String, files: leolib::external::WriteResult) {
        let none = files.written.is_empty() && files.unchanged == 0 && files.errors.is_empty();
        self.report_write(files);
        self.message = match none {
            true => leo,
            false => format!("{leo}; {}", self.message),
        };
    }

    /// Leo's `write-dirty-at-file-nodes`: write the outline's dirty external
    /// files.
    ///
    /// A file that exists but was never read is refused, then offered on a
    /// y/n prompt, as Leo asks before overwriting it (issue #50).
    pub fn write_dirty_at_file_nodes(&mut self) {
        let result = self.doc.write_external_files(true);
        self.report_write(result);
    }

    /// Leo's `write-at-file-nodes`: write every `@<file>` node at or under the
    /// selection, dirty or not.
    pub fn write_at_file_nodes(&mut self) {
        let (files, _) = leolib::external::find_files_to_write_under(self.outline(), &self.current);
        if files.is_empty() {
            self.message = "write-at-file-nodes: no external files here".to_string();
            return;
        }
        let result = self.doc.write_files(files);
        self.report_write(result);
    }

    /// Say what a write did, and ask about any file it refused.
    pub(super) fn report_write(&mut self, result: leolib::external::WriteResult) {
        let mut parts = vec![format!("wrote {}", result.written.len())];
        if result.unchanged > 0 {
            parts.push(format!("{} unchanged", result.unchanged));
        }
        if !result.errors.is_empty() {
            parts.push(format!(
                "{} failed: {}",
                result.errors.len(),
                result.errors[0].error
            ));
        }
        self.message = parts.join(", ");
        let mut refused = result.refused;
        refused.extend(result.changed_on_disk);
        if !refused.is_empty() {
            self.pending_overwrite = refused;
            self.open_mini(MiniKind::ConfirmOverwrite, String::new());
        }
    }

    /// Say that external files changed on disk, when any this outline read did.
    ///
    /// Run when the terminal regains focus: the likeliest moment another
    /// program has written them.
    pub fn check_disk(&mut self) {
        self.changed_on_disk = self.outline().changed_files();
        let mut changed: Vec<String> = self
            .changed_on_disk
            .iter()
            .map(|path| leolib::util::short_file_name(path))
            .collect();
        if changed.is_empty() {
            return;
        }
        changed.sort();
        self.message = format!(
            "changed on disk: {}. :refresh-from-disk or :read-at-file-nodes reads them, :e! the .leo file",
            changed.join(", ")
        );
    }

    /// The files `check_disk` found changed that are still changed: a read,
    /// a write or `keep_changed_files` since may have settled one.
    pub fn files_changed_on_disk(&self) -> Vec<String> {
        let o = self.outline();
        self.changed_on_disk
            .iter()
            .filter(|path| o.changed_on_disk(path))
            .cloned()
            .collect()
    }

    /// What needs attention in p's external file, if p is an `@<file>` node.
    pub fn file_state(&self, p: &Position) -> Option<FileState> {
        let o = self.outline();
        if !p.is_any_at_file_node(o) {
            return None;
        }
        let path = o.full_path(p);
        if self.unread.contains_key(&path) {
            Some(FileState::Unread)
        } else if self.changed_on_disk.contains(&path) && o.changed_on_disk(&path) {
            Some(FileState::ChangedOnDisk)
        } else if !o.may_overwrite(p) {
            Some(FileState::Refused)
        } else if p.is_dirty(o) {
            Some(FileState::Unwritten)
        } else {
            None
        }
    }

    /// Read again the files changed on disk, asking first if that would
    /// discard edits not yet written.
    pub fn reload_changed_files(&mut self) {
        let changed = self.files_changed_on_disk();
        let o = self.outline();
        let Some(root) = o.root_position() else {
            return;
        };
        // `@asis` and `@nosent` files are never read; only Keep settles one.
        let (mut files, _) = leolib::external::find_files_to_read(o, &root, true);
        files.retain(|p| changed.contains(&o.full_path(p)));
        if files.is_empty() {
            self.message = match changed.is_empty() {
                true => "no external file has changed on disk".to_string(),
                false => "the changed files are never read; Keep settles them".to_string(),
            };
            return;
        }
        self.read_or_ask(files, false);
    }

    /// Keep the outline's text of the files changed on disk: the next write
    /// overwrites them without asking.
    pub fn keep_changed_files(&mut self) {
        let changed = self.files_changed_on_disk();
        let o = self.doc.outline_mut_untracked();
        for path in &changed {
            o.record_file_stamp(path, leolib::util::file_stamp(path));
        }
        self.changed_on_disk.clear();
        self.message = format!(
            "kept the outline's text of {}",
            plural(changed.len(), "file")
        );
    }

    /// Leo's `refresh-from-disk`: read the `@<file>` node at or above the
    /// selection from disk again.
    pub fn refresh_from_disk(&mut self) {
        let o = self.outline();
        let Some(root) = self
            .current
            .self_and_parents(o)
            .into_iter()
            .find(|p| p.is_any_at_file_node(o))
        else {
            self.message = "refresh-from-disk: not in an @<file> tree".to_string();
            return;
        };
        let (files, _) = leolib::external::find_files_to_read(o, &root, false);
        if files.is_empty() {
            self.message = format!("refresh-from-disk: {} is never read", root.h(o));
            return;
        }
        self.read_or_ask(files, true);
    }

    /// Leo's `read-at-file-nodes`: read every `@<file>` node at or under the
    /// selection from disk again.
    pub fn read_at_file_nodes(&mut self) {
        let (files, _) = leolib::external::find_files_to_read(self.outline(), &self.current, false);
        if files.is_empty() {
            self.message = "read-at-file-nodes: no external files to read here".to_string();
            return;
        }
        self.read_or_ask(files, false);
    }

    /// Read `files`, asking first if that would discard edits not yet written.
    /// `refresh` reads an `@clean` file whose mtime says it is unchanged.
    fn read_or_ask(&mut self, files: Vec<Position>, refresh: bool) {
        if files.iter().any(|p| p.is_dirty(self.outline())) {
            self.pending_read = (files, refresh);
            self.open_mini(MiniKind::ConfirmRead, String::new());
            return;
        }
        self.read_files(files, refresh);
    }

    pub(super) fn read_files(&mut self, files: Vec<Position>, refresh: bool) {
        // A selection inside a tree being rebuilt names nodes about to go, so
        // it moves to that tree's root, which the read keeps.
        for p in &files {
            self.unread.remove(&self.doc.outline().full_path(p));
        }
        let o = self.outline();
        let inside = self
            .current
            .self_and_parents(o)
            .into_iter()
            .find(|p| files.iter().any(|f| f.v == p.v));
        let result = if refresh {
            self.doc.refresh_files(files)
        } else {
            self.doc.read_files(files)
        };
        match inside {
            Some(root) => self.select(root),
            None => self.clamp_current(),
        }
        self.buffer = None;
        for e in &result.errors {
            self.unread.insert(e.path.clone(), e.error.to_string());
        }
        for line in read_report_lines(&result) {
            self.log(line);
        }
        self.message = read_report_message(&result).unwrap_or_else(|| {
            format!("read {}; undo history cleared", plural(result.read, "file"))
        });
    }

    /// Leo's `revert`, vim's `:e!`: open the `.leo` file again, dropping
    /// every change since it was last saved.
    pub fn revert(&mut self) {
        let path = self.outline().file_name.clone();
        if path.is_empty() {
            self.message = "revert: the outline has never been saved".to_string();
            return;
        }
        self.open_file_now(&path);
    }

    pub fn request_quit(&mut self) {
        if self.unsaved_work().is_empty() {
            self.quit = true;
            return;
        }
        self.open_mini(MiniKind::ConfirmQuit, String::new());
    }

    /// What quitting now would lose, or "" if nothing.
    ///
    /// Saving the `.leo` file clears `changed`, but an `@file` tree's text is
    /// not in the `.leo` file: only `w` puts it on disk.
    pub fn unsaved_work(&self) -> String {
        let o = self.outline();
        let files = leolib::external::find_files_to_write(o, true).0.len();
        let mut parts = Vec::new();
        if o.changed {
            parts.push("unsaved changes".to_string());
        }
        if files > 0 {
            parts.push(format!("{} not written", plural(files, "external file")));
        }
        parts.join(", ")
    }
}
