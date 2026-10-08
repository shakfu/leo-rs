//! Extensions: `@<file>` kinds Leo does not have, registered per outline.
//!
//! leolib is a port of Leo. A kind such as `@entangled` (crate
//! `leo-entangled`) implements
//! [`FileKind`]; an outline reads, writes and saves it only if the kind is in
//! the outline's [`Kinds`]. With [`Kinds::empty`], such a headline is an
//! ordinary node, as in Leo. The design is in `docs/dev/plugins.md`.

use std::sync::Arc;

use crate::error::Result;
use crate::outline::Outline;
use crate::position::Position;

/// The in-memory attribute holding a node's language, from the file it was
/// read from: a markdown fence's info string. `Outline::language_at` reads
/// it, since an `@language` line in the body would be written to the file.
pub const LANGUAGE: &str = "leo-rs-language";

/// An `@<file>` kind Leo does not have.
pub trait FileKind: Send + Sync {
    /// The headline directive, `@entangled`.
    fn directive(&self) -> &'static str;
    /// Read p's file into p's tree. True if it was read.
    fn read(&self, o: &mut Outline, p: &Position) -> Result<bool>;
    /// The text p's tree writes to its file.
    fn write(&self, o: &Outline, p: &Position) -> Result<String>;
    /// Before the `.leo` file is written: set what p saves in it.
    fn before_save(&self, _o: &mut Outline, _p: &Position) {}
    /// Whether the `.leo` file keeps p's body. Its children it never keeps.
    fn stores_body(&self) -> bool {
        true
    }
    /// Read before Leo's kinds, as clones shared with an `@clean` tree need.
    fn read_first(&self) -> bool {
        false
    }
    /// Plan renaming the node p under this kind to `headline`. None if the
    /// kind does not rename p; an error says why the rename is refused.
    fn plan_rename(
        &self,
        _o: &Outline,
        _p: &Position,
        _headline: &str,
    ) -> Option<std::result::Result<Rename, String>> {
        None
    }
    /// Whether a language server serves each node as a document of its own.
    fn nodes_are_documents(&self) -> bool {
        false
    }
    /// Why the node p under this kind may not be edited, if it may not.
    fn read_only(&self, _o: &Outline, _p: &Position) -> Option<String> {
        None
    }
    /// The markdown the node p under this kind stands for, for a rendered
    /// view; None if it is not markdown.
    fn node_markdown(&self, _o: &Outline, _p: &Position) -> Option<String> {
        None
    }
}

/// A tree kind Leo does not have: a headline directive whose subtree is not
/// a file, as `@wiki`. Unlike a [`FileKind`], the `.leo` file stores its
/// tree as any other, and nothing reads or writes a file for it.
pub trait TreeKind: Send + Sync {
    /// The headline directive, `@wiki`.
    fn directive(&self) -> &'static str;
    /// The language of every node in the tree, the root's included, over
    /// what directives and headlines say.
    fn language(&self) -> Option<&'static str> {
        None
    }
    /// Plan renaming the node p, in the tree rooted at `root`, to
    /// `headline`. None if the kind does not rename p; an error says why the
    /// rename is refused.
    fn plan_rename(
        &self,
        _o: &Outline,
        _root: &Position,
        _p: &Position,
        _headline: &str,
    ) -> Option<std::result::Result<Rename, String>> {
        None
    }
}

/// The registered tree kind p is under, and its root: the nearest ancestor,
/// or p itself, whose headline starts with the kind's directive.
pub fn tree_at(o: &Outline, p: &Position) -> Option<(Arc<dyn TreeKind>, Position)> {
    let kinds = o.kinds.clone();
    p.self_and_parents(o).into_iter().find_map(|q| {
        let h = q.h(o);
        kinds
            .trees
            .iter()
            .find(|k| crate::util::match_word(h, 0, k.directive()))
            .map(|k| (Arc::clone(k), q.clone()))
    })
}

/// The registered kind p is under: the nearest ancestor, or p itself, whose
/// headline names one.
pub fn kind_at(o: &Outline, p: &Position) -> Option<Arc<dyn FileKind>> {
    p.self_and_parents(o)
        .into_iter()
        .find_map(|a| o.kinds().find(a.h(o)).map(|(k, _)| k.clone()))
}

/// What renaming a node changes, worked out before anything is changed.
#[derive(Debug, Clone)]
pub struct Rename {
    /// The name before.
    pub old: String,
    /// The new name.
    pub new: String,
    /// Fences renamed.
    pub fences: usize,
    /// References rewritten.
    pub references: usize,
    /// What else to say about it, from the kind: what was not checked.
    pub note: Option<String>,
    /// Each node's new headline or body, in outline order.
    pub edits: Vec<(Position, Option<String>, Option<String>)>,
}

/// The kinds an outline reads, writes and saves beyond Leo's.
#[derive(Clone, Default)]
pub struct Kinds {
    kinds: Vec<Arc<dyn FileKind>>,
    trees: Vec<Arc<dyn TreeKind>>,
}

impl std::fmt::Debug for Kinds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.kinds.iter().map(|k| k.directive()).collect();
        f.debug_tuple("Kinds").field(&names).finish()
    }
}

impl Kinds {
    /// No kinds: Leo.
    pub fn empty() -> Self {
        Self::default()
    }

    /// These kinds and `kind`. Refused for a directive Leo has, or one
    /// already registered: a kind adds to Leo and never replaces it.
    pub fn with(mut self, kind: impl FileKind + 'static) -> std::result::Result<Self, String> {
        self.check_new(kind.directive())?;
        self.kinds.push(Arc::new(kind));
        Ok(self)
    }

    /// These kinds and the tree kind `kind`, refused as [`Kinds::with`] refuses.
    pub fn with_tree(mut self, kind: impl TreeKind + 'static) -> std::result::Result<Self, String> {
        self.check_new(kind.directive())?;
        self.trees.push(Arc::new(kind));
        Ok(self)
    }

    /// Why `d` cannot be registered, if it cannot.
    fn check_new(&self, d: &str) -> std::result::Result<(), String> {
        let leo = crate::node::AT_AUTO_NAMES
            .iter()
            .chain(crate::node::AT_FILE_NAMES)
            .chain(&["@leo"])
            .any(|n| *n == d);
        if leo {
            return Err(format!("{d} is Leo's"));
        }
        if !d.starts_with('@') || d.contains(char::is_whitespace) {
            return Err(format!("{d:?} is not a directive"));
        }
        let taken = self
            .kinds
            .iter()
            .map(|k| k.directive())
            .chain(self.trees.iter().map(|k| k.directive()));
        if taken.into_iter().any(|t| t == d) {
            return Err(format!("{d} is registered twice"));
        }
        Ok(())
    }

    /// The registered tree directives, in order.
    pub fn tree_directives(&self) -> Vec<&'static str> {
        self.trees.iter().map(|k| k.directive()).collect()
    }

    /// The kind headline `h` names, and the file name after its directive.
    pub fn find(&self, h: &str) -> Option<(&Arc<dyn FileKind>, String)> {
        self.kinds.iter().find_map(|k| {
            let name = crate::node::find_at_file_name(h, &[k.directive()]);
            (!name.is_empty()).then_some((k, name))
        })
    }

    /// The registered directives, in order.
    pub fn directives(&self) -> Vec<&'static str> {
        self.kinds.iter().map(|k| k.directive()).collect()
    }
}
