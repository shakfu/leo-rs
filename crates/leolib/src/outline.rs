//! The Outline: one open Leo document, independent of any view.
//!
//! Leo's commander was both the document and the window. An `Outline` is only
//! the document: the vnode arena, the gnx index, the file name, the dirty
//! flag, and the directive scanning that must give one answer for the whole
//! document rather than one per window.

use std::collections::{HashMap, HashSet};

use crate::gnx::NodeIndices;
use crate::langdata;
use crate::node::{self, Vnode, VnodeId};
use crate::position::Position;
use crate::util;

/// The handful of settings the model consults.
///
/// Leo reads these from `leoSettings.leo`; there is no settings file here, so
/// these are the code defaults Leo falls back to, spelled out. They are named
/// after the settings they stand for so a caller can override one.
#[derive(Debug, Clone)]
pub struct Config {
    pub default_derived_file_encoding: String,
    pub output_newline: String,
    pub page_width: i32,
    pub tab_width: i32,
    pub target_language: String,
    pub create_nonexistent_directories: bool,
    pub force_newlines_in_at_nosent_bodies: bool,
    pub body_pane_wraps: bool,
    /// The .leo file's own encoding. Leo spells it lowercase; the XML prolog copies it.
    pub leo_file_encoding: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_derived_file_encoding: "utf-8".to_string(),
            output_newline: "nl".to_string(),
            // Leo's *code* default. leoSettings.leo ships 80; nothing in the
            // writer depends on it, but check before adding anything that does.
            page_width: 132,
            tab_width: -4,
            target_language: "python".to_string(),
            create_nonexistent_directories: false,
            force_newlines_in_at_nosent_bodies: true,
            body_pane_wraps: true,
            leo_file_encoding: "utf-8".to_string(),
        }
    }
}

/// The window geometry a .leo file records. Data, not something applied here.
#[derive(Debug, Clone, Default)]
pub struct WindowGeometry {
    pub width: i32,
    pub height: i32,
    pub left: i32,
    pub top: i32,
    pub r1: f64,
    pub r2: f64,
}

#[derive(Debug)]
pub struct Outline {
    nodes: Vec<Vnode>,
    pub hidden_root: VnodeId,
    pub gnx_dict: HashMap<String, VnodeId>,
    pub file_name: String,
    pub changed: bool,
    pub ni: NodeIndices,
    pub config: Config,
    /// Bumped on every structural change, so a view can tell whether to redraw.
    pub generation: u64,
    /// Which nodes are unfolded, by gnx. Per document here; Leo keeps a copy per view.
    pub expanded: HashSet<String>,
    pub window_geometry: WindowGeometry,
    /// Last-seen mtime per @clean node, so an unchanged file is not re-read.
    pub mod_time_cache: HashMap<String, u64>,
    /// `(gnx, path, headline)` for every external file this outline has read
    /// or written. See [`Outline::may_overwrite`].
    pub read_paths: HashSet<(String, String, String)>,
}

pub const HIDDEN_ROOT_GNX: &str = "hidden-root-vnode-gnx";

impl Outline {
    /// An outline with a hidden root and nothing else. Not a valid document yet.
    pub fn new(file_name: &str) -> Self {
        let mut o = Self {
            nodes: Vec::new(),
            hidden_root: VnodeId(0),
            gnx_dict: HashMap::new(),
            file_name: file_name.to_string(),
            changed: false,
            ni: NodeIndices::new(&default_user_id()),
            config: Config::default(),
            generation: 0,
            expanded: HashSet::new(),
            window_geometry: WindowGeometry::default(),
            mod_time_cache: HashMap::new(),
            read_paths: HashSet::new(),
        };
        let hidden = o.new_vnode(Some(HIDDEN_ROOT_GNX));
        o.node_mut(hidden).h = "<hidden root vnode>".to_string();
        o.hidden_root = hidden;
        o
    }

    /// An outline with one empty node, as `File > New` produces.
    pub fn new_empty() -> Self {
        let mut o = Self::new("");
        let root = o.new_vnode(None);
        o.node_mut(root).h = "newHeadline".to_string();
        o.link_child(o.hidden_root, 0, root);
        o
    }

    // --- The arena --------------------------------------------------------

    pub fn node(&self, v: VnodeId) -> &Vnode {
        &self.nodes[v.0 as usize]
    }

    pub fn node_mut(&mut self, v: VnodeId) -> &mut Vnode {
        &mut self.nodes[v.0 as usize]
    }

    pub fn gnx(&self, v: VnodeId) -> &str {
        &self.node(v).gnx
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Allocate a vnode. With no gnx, mint one; with a gnx, index it.
    pub fn new_vnode(&mut self, gnx: Option<&str>) -> VnodeId {
        let gnx = match gnx {
            Some(g) => g.to_string(),
            None => self.ni.new_gnx(),
        };
        let id = VnodeId(self.nodes.len() as u32);
        self.nodes.push(Vnode::new(gnx.clone()));
        self.gnx_dict.insert(gnx, id);
        id
    }

    pub fn find_gnx(&self, gnx: &str) -> Option<VnodeId> {
        self.gnx_dict.get(gnx).copied()
    }

    // --- Positions --------------------------------------------------------

    pub fn root_position(&self) -> Option<Position> {
        let first = *self.node(self.hidden_root).children.first()?;
        Some(Position::new(first, 0, Vec::new()))
    }

    /// Every position in the outline, in outline order. Clones appear once per place.
    pub fn all_positions(&self) -> Vec<Position> {
        let mut out = Vec::new();
        let mut p = self.root_position();
        while let Some(cur) = p {
            out.push(cur.clone());
            p = cur.thread_next(self);
        }
        out
    }

    /// One position per distinct vnode, skipping the other places a clone appears.
    pub fn all_unique_positions(&self) -> Vec<Position> {
        let mut seen: HashSet<VnodeId> = HashSet::new();
        let mut out = Vec::new();
        let mut p = self.root_position();
        while let Some(cur) = p {
            if seen.insert(cur.v) {
                out.push(cur.clone());
                p = cur.thread_next(self);
            } else {
                p = cur.node_after_tree(self);
            }
        }
        out
    }

    pub fn all_unique_nodes(&self) -> Vec<VnodeId> {
        self.all_unique_positions().iter().map(|p| p.v).collect()
    }

    pub fn clear_all_visited(&mut self) {
        for v in self.nodes.iter_mut() {
            v.clear_bit(node::status::VISITED);
        }
    }

    pub fn short_file_name(&self) -> String {
        util::short_file_name(&self.file_name)
    }

    // --- Links ------------------------------------------------------------

    /// Insert `child` as parent's nth child, keeping the parent lists correct.
    fn link_child(&mut self, parent_v: VnodeId, n: usize, child: VnodeId) {
        self.generation += 1;
        self.node_mut(parent_v).children.insert(n, child);
        self.node_mut(child).parents.push(parent_v);
        // A vnode that just gained its first parent re-enters the tree, so its
        // descendants gain their parent links back. Undoing a delete needs this.
        if self.node(child).parents.len() == 1 {
            let kids = self.node(child).children.clone();
            for k in kids {
                self.add_parent_links(k, child);
            }
        }
    }

    fn add_parent_links(&mut self, v: VnodeId, parent: VnodeId) {
        self.node_mut(v).parents.push(parent);
        if self.node(v).parents.len() == 1 {
            let kids = self.node(v).children.clone();
            for k in kids {
                self.add_parent_links(k, v);
            }
        }
    }

    /// Insert `child` as parent's nth child without touching descendant links.
    fn link_copied_child(&mut self, parent_v: VnodeId, n: usize, child: VnodeId) {
        self.generation += 1;
        self.node_mut(parent_v).children.insert(n, child);
        self.node_mut(child).parents.push(parent_v);
    }

    fn cut_link(&mut self, parent_v: VnodeId, n: usize, child: VnodeId) {
        self.generation += 1;
        debug_assert_eq!(self.node(parent_v).children[n], child);
        self.node_mut(parent_v).children.remove(n);
        if let Some(i) = self.node(child).parents.iter().position(|x| *x == parent_v) {
            self.node_mut(child).parents.remove(i);
        }
        if self.node(child).parents.is_empty() {
            let kids = self.node(child).children.clone();
            for k in kids {
                self.cut_parent_links(k, child);
            }
        }
    }

    fn cut_parent_links(&mut self, v: VnodeId, parent: VnodeId) {
        if let Some(i) = self.node(v).parents.iter().position(|x| *x == parent) {
            self.node_mut(v).parents.remove(i);
        }
        if self.node(v).parents.is_empty() {
            let kids = self.node(v).children.clone();
            for k in kids {
                self.cut_parent_links(k, v);
            }
        }
    }

    /// Detach v's whole subtree, so a reader can rebuild every link from scratch.
    ///
    /// Leo detaches only the root's children, which leaves a re-read appending
    /// a second parent link to every node below and making them all look
    /// cloned. Each vnode is visited once, so a clone inside the tree is safe.
    pub fn detach_subtree(&mut self, v: VnodeId) {
        let mut seen: HashSet<VnodeId> = HashSet::new();
        let mut todo = vec![v];
        while let Some(cur) = todo.pop() {
            if !seen.insert(cur) {
                continue;
            }
            let kids = self.node(cur).children.clone();
            for k in &kids {
                if let Some(i) = self.node(*k).parents.iter().position(|x| *x == cur) {
                    self.node_mut(*k).parents.remove(i);
                }
            }
            self.node_mut(cur).children.clear();
            todo.extend(kids);
        }
        self.generation += 1;
    }

    /// Detach every child of v. Used by the readers before rebuilding a tree.
    pub fn delete_all_children(&mut self, v: VnodeId) {
        let kids = self.node(v).children.clone();
        for k in kids {
            if let Some(i) = self.node(k).parents.iter().position(|x| *x == v) {
                self.node_mut(k).parents.remove(i);
            }
        }
        self.node_mut(v).children.clear();
        self.generation += 1;
    }

    // --- Editing the tree -------------------------------------------------

    pub fn insert_as_nth_child(&mut self, parent: &Position, n: usize) -> Position {
        let v = self.new_vnode(None);
        self.link_child(parent.v, n, v);
        let mut stack = parent.stack.clone();
        stack.push((parent.v, parent.child_index));
        Position::new(v, n, stack)
    }

    pub fn insert_as_first_child(&mut self, parent: &Position) -> Position {
        self.insert_as_nth_child(parent, 0)
    }

    pub fn insert_as_last_child(&mut self, parent: &Position) -> Position {
        let n = parent.num_children(self);
        self.insert_as_nth_child(parent, n)
    }

    pub fn insert_after(&mut self, p: &Position) -> Position {
        let v = self.new_vnode(None);
        let parent_v = p.parent_vnode(self);
        let n = p.child_index + 1;
        self.link_child(parent_v, n, v);
        Position::new(v, n, p.stack.clone())
    }

    pub fn insert_before(&mut self, p: &Position) -> Position {
        if let Some(back) = p.back(self) {
            return self.insert_after(&back);
        }
        if let Some(parent) = p.parent(self) {
            return self.insert_as_nth_child(&parent, 0);
        }
        let v = self.new_vnode(None);
        self.link_child(self.hidden_root, 0, v);
        Position::new(v, 0, Vec::new())
    }

    /// Link p's vnode again, right after p: a clone.
    pub fn clone_node(&mut self, p: &Position) -> Position {
        let parent_v = p.parent_vnode(self);
        let n = p.child_index + 1;
        self.link_child(parent_v, n, p.v);
        Position::new(p.v, n, p.stack.clone())
    }

    /// Unlink p from the outline. The vnode survives, so undo can relink it.
    pub fn delete_position(&mut self, p: &Position) {
        self.set_dirty(p);
        let parent_v = p.parent_vnode(self);
        self.cut_link(parent_v, p.child_index, p.v);
    }

    /// Move p to be the nth child of `parent`. Returns p's new position.
    pub fn move_to_nth_child_of(&mut self, p: &Position, parent: &Position, n: usize) -> Position {
        let parent = adjust_before_unlink(self, parent, p);
        let parent_v = p.parent_vnode(self);
        self.cut_link(parent_v, p.child_index, p.v);
        self.link_child(parent.v, n, p.v);
        let mut stack = parent.stack.clone();
        stack.push((parent.v, parent.child_index));
        Position::new(p.v, n, stack)
    }

    /// Move p to just after `a`. Returns p's new position.
    pub fn move_after(&mut self, p: &Position, a: &Position) -> Position {
        let a = adjust_before_unlink(self, a, p);
        let parent_v = p.parent_vnode(self);
        self.cut_link(parent_v, p.child_index, p.v);
        let a_parent_v = a.parent_vnode(self);
        let n = a.child_index + 1;
        self.link_child(a_parent_v, n, p.v);
        Position::new(p.v, n, a.stack.clone())
    }

    pub fn move_to_root(&mut self, p: &Position) -> Position {
        let parent_v = p.parent_vnode(self);
        self.cut_link(parent_v, p.child_index, p.v);
        self.link_child(self.hidden_root, 0, p.v);
        Position::new(p.v, 0, Vec::new())
    }

    /// Splice p's children into p's parent, just after p.
    pub fn promote(&mut self, p: &Position) {
        let parent_v = p.parent_vnode(self);
        let children = self.node(p.v).children.clone();
        if children.is_empty() {
            return;
        }
        let n = p.child_index + 1;
        let mut z = self.node(parent_v).children.clone();
        let tail = z.split_off(n);
        z.extend(children.iter().copied());
        z.extend(tail);
        self.node_mut(parent_v).children = z;
        self.node_mut(p.v).children.clear();
        for child in children {
            if let Some(i) = self.node(child).parents.iter().position(|x| *x == p.v) {
                self.node_mut(child).parents.remove(i);
            }
            self.node_mut(child).parents.push(parent_v);
        }
        self.generation += 1;
    }

    /// True if p still names a real place in the outline.
    pub fn position_exists(&self, p: &Position) -> bool {
        let parent_v = p.parent_vnode(self);
        self.node(parent_v).children.get(p.child_index) == Some(&p.v)
    }

    /// An unlinked copy of p's tree with fresh gnxs. Paste links it back in.
    pub fn copy_tree(&mut self, p: &Position) -> VnodeId {
        self.copy_tree_helper(p.v)
    }

    /// An unlinked copy of one vnode's tree, for a second paste of a clipboard.
    pub fn copy_tree_of_vnode(&mut self, v: VnodeId) -> VnodeId {
        self.copy_tree_helper(v)
    }

    fn copy_tree_helper(&mut self, v: VnodeId) -> VnodeId {
        let v2 = self.new_vnode(None);
        self.node_mut(v2).h = self.node(v).h.clone();
        self.node_mut(v2).b = self.node(v).b.clone();
        self.node_mut(v2).uas = self.node(v).uas.clone();
        let kids = self.node(v).children.clone();
        for k in kids {
            let k2 = self.copy_tree_helper(k);
            self.node_mut(v2).children.push(k2);
        }
        v2
    }

    /// Link an unlinked tree (from [`copy_tree`](Self::copy_tree)) after `p`.
    pub fn paste_after(&mut self, p: &Position, v: VnodeId) -> Position {
        let parent_v = p.parent_vnode(self);
        let n = p.child_index + 1;
        self.link_copied_child(parent_v, n, v);
        self.add_descendant_parent_links(v);
        Position::new(v, n, p.stack.clone())
    }

    fn add_descendant_parent_links(&mut self, v: VnodeId) {
        let kids = self.node(v).children.clone();
        for k in kids {
            if !self.node(k).parents.contains(&v) {
                self.node_mut(k).parents.push(v);
            }
            self.add_descendant_parent_links(k);
        }
    }

    // --- Content ----------------------------------------------------------

    pub fn set_headline(&mut self, p: &Position, s: &str) {
        let s = s.replace('\n', "");
        self.node_mut(p.v).h = s;
        let gnx = self.gnx(p.v).to_string();
        self.mod_time_cache.remove(&gnx);
        self.set_dirty(p);
        self.changed = true;
        self.generation += 1;
    }

    pub fn set_body(&mut self, p: &Position, s: &str) {
        self.node_mut(p.v).b = s.to_string();
        self.set_dirty(p);
        self.changed = true;
        self.generation += 1;
    }

    pub fn toggle_marked(&mut self, p: &Position) {
        let marked = self.node(p.v).is_marked();
        if marked {
            self.node_mut(p.v).clear_bit(node::status::MARKED);
        } else {
            self.node_mut(p.v).set_bit(node::status::MARKED);
        }
        self.changed = true;
    }

    /// Mark p and every ancestor @<file> node dirty, following clone links.
    pub fn set_dirty(&mut self, p: &Position) {
        self.node_mut(p.v).set_bit(node::status::DIRTY);
        for v in self.all_ancestor_at_file_nodes(p.v) {
            self.node_mut(v).set_bit(node::status::DIRTY);
        }
    }

    /// Every @<file> node at or above `v`, through all of `v`'s parents.
    fn all_ancestor_at_file_nodes(&self, v: VnodeId) -> Vec<VnodeId> {
        let mut seen: HashSet<VnodeId> = HashSet::new();
        let mut todo = vec![v];
        let mut out = Vec::new();
        while let Some(cur) = todo.pop() {
            if !seen.insert(cur) {
                continue;
            }
            if node::is_any_at_file_node(&self.node(cur).h) {
                out.push(cur);
            }
            todo.extend(self.node(cur).parents.iter().copied());
        }
        out
    }

    pub fn clear_dirty_in_tree(&mut self, p: &Position) {
        for p2 in p.self_and_subtree(self) {
            self.node_mut(p2.v).clear_bit(node::status::DIRTY);
        }
    }

    // --- Overwrite safety -------------------------------------------------

    /// Record that p's external file at `path` has been read or written.
    pub fn remember_read_path(&mut self, p: &Position, path: &str) {
        let key = (
            self.gnx(p.v).to_string(),
            path.to_string(),
            p.h(self).to_string(),
        );
        self.read_paths.insert(key);
    }

    /// True if writing p's file cannot destroy work this outline never saw.
    ///
    /// An `@<file>` node whose file exists but was never read is the case Leo
    /// warns about (issue #50): the outline holds no copy of what is in that
    /// file, so writing it discards the file. With no one to ask, refusing is
    /// the only safe answer. `@nosent` is exempt because its file is never
    /// read, and `@clean` because its reader runs on every open.
    pub fn may_overwrite(&self, p: &Position) -> bool {
        if p.is_at_nosent_node(self) || p.is_at_clean_node(self) {
            return true;
        }
        let path = self.full_path(p);
        if !std::path::Path::new(&path).exists() {
            return true;
        }
        let key = (self.gnx(p.v).to_string(), path, p.h(self).to_string());
        self.read_paths.contains(&key)
    }

    // --- Folds ------------------------------------------------------------

    pub fn is_expanded(&self, p: &Position) -> bool {
        self.expanded.contains(self.gnx(p.v))
    }

    pub fn expand(&mut self, p: &Position) {
        let gnx = self.gnx(p.v).to_string();
        self.expanded.insert(gnx);
    }

    pub fn contract(&mut self, p: &Position) {
        let gnx = self.gnx(p.v).to_string();
        self.expanded.remove(&gnx);
    }

    // --- Directive scanners ----------------------------------------------
    //
    // Asked of the document, never of a window: two views of one outline must
    // not disagree about what language a node is written in.

    /// The path in effect at p, from @path directives and the .leo file's directory.
    pub fn get_path(&self, p: &Position) -> String {
        let mut paths: Vec<String> = Vec::new();
        for p2 in p.self_and_parents(self) {
            if let Some(path) = self.get_path_from_node(&p2) {
                paths.push(path);
            }
        }
        let absbase = if self.file_name.is_empty() {
            util::home_dir()
        } else {
            util::os_path_dirname(&self.file_name)
        };
        paths.push(absbase);
        paths.reverse();
        let refs: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
        util::finalize_join(&refs)
    }

    fn get_path_from_node(&self, p: &Position) -> Option<String> {
        // The headline wins over the body because it is more visible.
        for (is_body, s) in [(false, p.h(self)), (true, p.b(self))] {
            let mut found: Option<String> = None;
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("@path") {
                    if rest.starts_with(char::is_whitespace) {
                        // @path in an @file body would name a path for the file
                        // that already has one. Leo warns; here it is ignored.
                        if is_body && p.is_any_at_file_node(self) {
                            continue;
                        }
                        let path = util::strip_path_cruft(rest.trim());
                        if !path.is_empty() && found.is_none() {
                            found = Some(path);
                        }
                    }
                }
            }
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// The absolute path of p's external file, or of its enclosing directory.
    pub fn full_path(&self, p: &Position) -> String {
        let name = p.any_at_file_node_name(self);
        util::finalize_join(&[&self.get_path(p), &name])
    }

    /// The first directive matching `pattern` at or above p, headline before body.
    fn scan_directive(&self, p: &Position, name: &str) -> Option<String> {
        for p2 in p.self_and_parents(self) {
            for s in [p2.h(self), p2.b(self)] {
                for line in s.lines() {
                    if let Some(rest) = line.strip_prefix(name) {
                        if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                            return Some(rest.trim().to_string());
                        }
                    }
                }
            }
        }
        None
    }

    pub fn get_encoding(&self, p: &Position) -> String {
        if let Some(s) = self.scan_directive(p, "@encoding") {
            let enc = s.split_whitespace().next().unwrap_or("");
            if is_valid_encoding(enc) {
                return enc.to_string();
            }
        }
        self.config.default_derived_file_encoding.clone()
    }

    pub fn get_line_ending(&self, p: &Position) -> String {
        if let Some(s) = self.scan_directive(p, "@lineending") {
            let name = s.split_whitespace().next().unwrap_or("");
            if ["cr", "crlf", "lf", "nl", "platform"].contains(&name) {
                return util::get_output_newline(name);
            }
        }
        String::new()
    }

    pub fn get_page_width(&self, p: &Position) -> i32 {
        if let Some(s) = self.scan_directive(p, "@pagewidth") {
            if let Ok(n) = s.split_whitespace().next().unwrap_or("").parse::<i32>() {
                return n;
            }
        }
        self.config.page_width
    }

    pub fn get_tab_width(&self, p: &Position) -> i32 {
        if let Some(s) = self.scan_directive(p, "@tabwidth") {
            if let Ok(n) = s.split_whitespace().next().unwrap_or("").parse::<i32>() {
                return n;
            }
        }
        self.config.tab_width
    }

    /// The comment delimiters in effect at p: (single, block-start, block-end).
    pub fn get_delims(&self, p: &Position) -> (String, String, String) {
        for p2 in p.self_and_parents(self) {
            for s in [p2.h(self), p2.b(self)] {
                for line in s.lines() {
                    if let Some(rest) = line.strip_prefix("@comment") {
                        if rest.starts_with(char::is_whitespace) {
                            return util::set_delims_from_string(rest);
                        }
                    }
                }
            }
        }
        let mut language = self.get_language(p);
        if language.is_empty() {
            language = self.config.target_language.clone();
        }
        if language.is_empty() {
            language = "python".to_string();
        }
        set_delims_from_language(&language)
    }

    /// The language in effect at p.
    ///
    /// Four passes, in Leo's order: an unambiguous @language directive in p's
    /// body, then in its direct ancestors, then in ancestors reached through
    /// clone links, then the file extension of the nearest @<file> headline.
    pub fn get_language(&self, p: &Position) -> String {
        if let Some(lang) = find_first_valid_at_language(p.b(self)) {
            return lang;
        }
        for p2 in p.self_and_parents(self) {
            let langs = find_all_valid_languages(p2.b(self));
            if langs.len() == 1 {
                return langs[0].clone();
            }
        }
        for v in self.v_and_parents(p.v) {
            let langs = find_all_valid_languages(&self.node(v).b);
            if langs.len() == 1 {
                return langs[0].clone();
            }
        }
        for p2 in p.self_and_parents(self) {
            if let Some(lang) = self.language_from_headline(p2.v) {
                return lang;
            }
        }
        for v in self.v_and_parents(p.v) {
            if let Some(lang) = self.language_from_headline(v) {
                return lang;
            }
        }
        self.config.target_language.clone()
    }

    fn language_from_headline(&self, v: VnodeId) -> Option<String> {
        let h = &self.node(v).h;
        if !node::is_any_at_file_node(h) {
            return None;
        }
        let name = node::any_at_file_node_name(h);
        let (_, ext) = util::os_path_splitext(&name);
        let ext = ext.strip_prefix('.').unwrap_or(&ext);
        let lang = langdata::extension_dict().get(ext)?;
        if is_valid_language(lang) {
            Some(lang.to_string())
        } else {
            None
        }
    }

    /// v and every ancestor reachable through parent links, breadth-first, once each.
    fn v_and_parents(&self, v: VnodeId) -> Vec<VnodeId> {
        let mut seen: HashSet<VnodeId> = HashSet::new();
        seen.insert(self.hidden_root);
        let mut out = Vec::new();
        let mut todo = vec![v];
        while let Some(cur) = todo.pop() {
            if !seen.insert(cur) {
                continue;
            }
            out.push(cur);
            todo.extend(self.node(cur).parents.iter().copied());
        }
        out
    }
}

/// Adjust `p` for the imminent unlinking of `p2`, as `p._adjustPositionBeforeUnlink`.
///
/// Removing an earlier sibling shifts the child index of everything after it,
/// including in the stack of a deeper position. Skipping this leaves a stale
/// index that names the wrong node rather than failing.
fn adjust_before_unlink(o: &Outline, p: &Position, p2: &Position) -> Position {
    let mut p = p.clone();
    // A previous sibling of p itself.
    let mut sib = p.clone();
    while let Some(back) = sib.back(o) {
        sib = back;
        if sib == *p2 {
            p.child_index -= 1;
            return p;
        }
    }
    // A previous sibling of one of p's ancestors.
    let mut stack: Vec<(VnodeId, usize)> = Vec::new();
    let mut changed = false;
    for i in 0..p.stack.len() {
        let (v, child_index) = p.stack[i];
        let mut p3 = Some(Position::new(v, child_index, stack[..i].to_vec()));
        let mut matched = false;
        while let Some(cur) = p3 {
            if *p2 == cur {
                stack.push((v, child_index - 1));
                changed = true;
                matched = true;
                break;
            }
            p3 = cur.back(o);
        }
        if !matched {
            stack.push((v, child_index));
        }
    }
    if changed {
        p.stack = stack;
    }
    p
}

fn default_user_id() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "leo-rs".to_string())
}

pub fn is_valid_language(language: &str) -> bool {
    !language.is_empty()
        && (langdata::language_delims_dict().contains_key(language)
            || langdata::delegate_language_dict().contains_key(language))
}

/// Encodings the model can actually decode. Leo asks Python's codec registry.
pub fn is_valid_encoding(encoding: &str) -> bool {
    matches!(
        encoding.to_lowercase().replace('_', "-").as_str(),
        "utf-8" | "utf8" | "ascii" | "us-ascii" | "latin-1" | "latin1" | "iso-8859-1"
    )
}

pub fn set_delims_from_language(language: &str) -> (String, String, String) {
    match langdata::language_delims_dict().get(language) {
        Some(val) => {
            let (d1, d2, d3) = util::set_delims_from_string(val);
            if !d2.is_empty() && d3.is_empty() {
                (String::new(), d1, d2)
            } else {
                (d1, d2, d3)
            }
        }
        None => (String::new(), String::new(), String::new()),
    }
}

fn find_first_valid_at_language(s: &str) -> Option<String> {
    at_language_directives(s).into_iter().next()
}

fn find_all_valid_languages(s: &str) -> Vec<String> {
    let mut v = at_language_directives(s);
    v.sort();
    v.dedup();
    v
}

/// Every valid language named by an `@language` directive at the start of a line.
fn at_language_directives(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("@language") {
            if !rest.starts_with(char::is_whitespace) {
                continue;
            }
            // `\w+`: the directive names one word, and stops at the first non-word.
            let word: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if is_valid_language(&word) {
                out.push(word);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_outline_has_one_node() {
        let o = Outline::new_empty();
        let all = o.all_positions();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].h(&o), "newHeadline");
    }

    #[test]
    fn clones_share_a_vnode_and_appear_twice() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "shared");
        let clone = o.clone_node(&child);
        assert_eq!(clone.v, child.v);
        assert_eq!(o.all_positions().len(), 3);
        assert_eq!(o.all_unique_positions().len(), 2);
        o.set_headline(&clone, "renamed");
        assert_eq!(child.h(&o), "renamed");
    }

    #[test]
    fn deleting_an_earlier_sibling_adjusts_the_mover() {
        // move_after must survive the unlink shifting its own target's index.
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "a");
        let b = o.insert_as_last_child(&root);
        o.set_headline(&b, "b");
        let c = o.insert_as_last_child(&root);
        o.set_headline(&c, "c");
        // Move a after c: a is before c, so c's index shifts down by one.
        o.move_after(&a, &c);
        let heads: Vec<&str> = root.children(&o).iter().map(|p| p.h(&o)).collect();
        assert_eq!(heads, vec!["b", "c", "a"]);
    }

    #[test]
    fn promote_splices_children_in_place() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "a");
        let a1 = o.insert_as_last_child(&a);
        o.set_headline(&a1, "a1");
        let b = o.insert_as_last_child(&root);
        o.set_headline(&b, "b");
        o.promote(&a);
        let heads: Vec<&str> = root.children(&o).iter().map(|p| p.h(&o)).collect();
        assert_eq!(heads, vec!["a", "a1", "b"]);
    }

    #[test]
    fn language_comes_from_the_at_file_extension() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        let child = o.insert_as_last_child(&root);
        assert_eq!(o.get_language(&child), "python");
        assert_eq!(o.get_delims(&child).0, "#");
    }

    #[test]
    fn an_at_language_directive_wins_over_the_extension() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        let child = o.insert_as_last_child(&root);
        o.set_body(&child, "@language c\nint main(void);\n");
        assert_eq!(o.get_language(&child), "c");
    }

    #[test]
    fn editing_a_node_dirties_its_ancestor_file_node() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        let child = o.insert_as_last_child(&root);
        o.node_mut(root.v).clear_bit(node::status::DIRTY);
        o.set_body(&child, "x = 1\n");
        assert!(o.node(root.v).is_dirty());
    }
}
