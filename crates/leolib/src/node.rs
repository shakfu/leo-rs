//! Vnodes: the data of one outline node.
//!
//! A vnode may have several parents -- that is what a clone is -- so the tree
//! is a DAG and cannot be expressed as owned children. Vnodes therefore live
//! in an arena on the [`Outline`](crate::Outline) and refer to each other by
//! [`VnodeId`]. Leo's `VNode.context` pointer becomes the arena that holds it.

use std::collections::BTreeMap;

/// An index into the outline's vnode arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VnodeId(pub u32);

/// Status bits, with the same values Leo writes into .leo files.
pub mod status {
    /// The node is marked. Leo's `markedBit`, the one bit saved in `.leo` files.
    pub const MARKED: u32 = 0x08;
    /// Scratch bit for traversals. Leo's `visitedBit`.
    pub const VISITED: u32 = 0x100;
    /// The node has unsaved changes. Leo's `dirtyBit`.
    pub const DIRTY: u32 = 0x200;
    /// The node's body goes in the `.leo` file's `<t>` elements. Leo's `writeBit`.
    pub const WRITE: u32 = 0x400;
    /// An error in the node's `@<file>` tree. Leo's `orphanBit`.
    pub const ORPHAN: u32 = 0x800;
}

/// An unknown attribute: a value Leo does not interpret, read from a .leo file.
///
/// Leo pickles these. Nothing here unpickles them, so a value arrives as the
/// raw hex string it was written as and is written back unchanged. That keeps
/// a round trip lossless without pulling a pickle interpreter into the model;
/// the price is that a uA's *value* is opaque to Rust callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ua {
    /// A `str_`-prefixed attribute: plain text, both in the file and here.
    Text(String),
    /// Anything else: the hexlified pickle exactly as the file spells it.
    Opaque(String),
}

impl Ua {
    /// The value as the `.leo` file spells it.
    pub fn as_file_text(&self) -> &str {
        match self {
            Ua::Text(s) | Ua::Opaque(s) => s,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// One node's data, shared by all its clones. Leo's `VNode`.
pub struct Vnode {
    /// The node's global id. See [`crate::gnx`].
    pub gnx: String,
    /// The headline.
    pub h: String,
    /// The body text.
    pub b: String,
    /// Child links, in order. A vnode may appear more than once.
    pub children: Vec<VnodeId>,
    /// Every vnode that has this one as a child, once per link.
    pub parents: Vec<VnodeId>,
    /// Bits from [`status`].
    pub status: u32,
    /// Unknown attributes, by name.
    pub uas: BTreeMap<String, Ua>,
}

impl Vnode {
    /// An empty node with id `gnx`.
    pub fn new(gnx: String) -> Self {
        Self {
            gnx,
            ..Default::default()
        }
    }

    /// True if [`status::MARKED`] is set.
    pub fn is_marked(&self) -> bool {
        self.status & status::MARKED != 0
    }
    /// True if [`status::DIRTY`] is set.
    pub fn is_dirty(&self) -> bool {
        self.status & status::DIRTY != 0
    }
    /// True if [`status::VISITED`] is set.
    pub fn is_visited(&self) -> bool {
        self.status & status::VISITED != 0
    }
    /// True if [`status::ORPHAN`] is set.
    pub fn is_orphan(&self) -> bool {
        self.status & status::ORPHAN != 0
    }
    /// Set the `status` bits in `bit`.
    pub fn set_bit(&mut self, bit: u32) {
        self.status |= bit;
    }
    /// Clear the `status` bits in `bit`.
    pub fn clear_bit(&mut self, bit: u32) {
        self.status &= !bit;
    }

    /// The headline with leading whitespace removed, as the `@<file>` tests want it.
    pub fn head_stripped(&self) -> &str {
        self.h.trim_start()
    }
}

/// The `@<file>` spellings Leo understands, grouped as `VNode.at*NodeName` groups them.
///
/// Leo adds an importer's `@auto-<name>` spellings to `app.atAutoNames` when
/// it loads the importer, so these are its full set.
pub const AT_AUTO_NAMES: &[&str] = &[
    "@auto",
    "@auto-markdown",
    "@auto-md",
    "@auto-org",
    "@auto-org-mode",
    "@auto-otl",
    "@auto-rst",
    "@auto-vim-outline",
];
/// The other `@<file>` spellings: Leo's `app.atFileNames`.
pub const AT_FILE_NAMES: &[&str] = &[
    "@asis",
    "@clean",
    "@edit",
    "@file",
    "@file-asis",
    "@file-nosent",
    "@file-thin",
    "@jupytext",
    "@nosent",
    "@shadow",
    "@thin",
];

/// Skip an identifier, also accepting the characters in `extra`.
fn skip_id(s: &str, mut i: usize, extra: &str) -> usize {
    let b = s.as_bytes();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphanumeric() || c == '_' || extra.contains(c) {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// The file name following one of `names` in `h`, or "" if `h` is not such a node.
///
/// A port of `VNode.findAtFileName`. Two rules matter and are easy to lose:
/// the `@` must be at column 0, and a directive with no file name after it
/// answers "", so a bare `@file` is not an @file node.
pub fn find_at_file_name(h: &str, names: &[&str]) -> String {
    if !h.starts_with('@') {
        return String::new();
    }
    let i = skip_id(h, 1, "-");
    let word = &h[..i];
    if names.contains(&word) {
        return h[i..].trim().to_string();
    }
    String::new()
}

/// The name following any `@<file>` directive, as `v.anyAtFileNodeName`.
pub fn any_at_file_node_name(h: &str) -> String {
    let name = find_at_file_name(h, AT_AUTO_NAMES);
    if !name.is_empty() {
        return name;
    }
    let name = find_at_file_name(h, AT_FILE_NAMES);
    if !name.is_empty() {
        return name;
    }
    let name = at_entangled_node_name(h);
    if !name.is_empty() {
        return name;
    }
    find_at_file_name(h, &["@leo"])
}

/// The file name after `@entangled`, a leo-rs kind Leo does not know, so it
/// is in neither of Leo's lists (`crate::entangled`).
pub fn at_entangled_node_name(h: &str) -> String {
    find_at_file_name(h, &["@entangled"])
}

/// True if `h` names any `@<file>` or `@leo` node, as `v.isAnyAtFileNode`.
pub fn is_any_at_file_node(h: &str) -> bool {
    !any_at_file_node_name(h).is_empty()
}

/// As `v.atAutoNodeName`: the file name after any `@auto` spelling.
pub fn at_auto_node_name(h: &str) -> String {
    find_at_file_name(h, AT_AUTO_NAMES)
}
/// As `v.atCleanNodeName`: the file name after `@clean`.
pub fn at_clean_node_name(h: &str) -> String {
    find_at_file_name(h, &["@clean"])
}
/// As `v.atEditNodeName`: the file name after `@edit`.
pub fn at_edit_node_name(h: &str) -> String {
    find_at_file_name(h, &["@edit"])
}
/// As `v.atFileNodeName`: the file name after `@file` or `@thin`.
pub fn at_file_node_name(h: &str) -> String {
    find_at_file_name(h, &["@file", "@thin"])
}
/// As `v.atJupytextNodeName`: the file name after `@jupytext`.
pub fn at_jupytext_node_name(h: &str) -> String {
    find_at_file_name(h, &["@jupytext"])
}
/// As `v.atNoSentFileNodeName`: the file name after `@nosent` or `@file-nosent`.
pub fn at_nosent_node_name(h: &str) -> String {
    find_at_file_name(h, &["@nosent", "@file-nosent"])
}
/// As `v.atAsisFileNodeName`: the file name after `@asis` or `@file-asis`.
pub fn at_asis_node_name(h: &str) -> String {
    find_at_file_name(h, &["@asis", "@file-asis"])
}
/// As `v.atShadowFileNodeName`: the file name after `@shadow`.
pub fn at_shadow_node_name(h: &str) -> String {
    find_at_file_name(h, &["@shadow"])
}
/// As `v.atThinFileNodeName`: the file name after `@thin` or `@file-thin`.
pub fn at_thin_node_name(h: &str) -> String {
    find_at_file_name(h, &["@thin", "@file-thin"])
}

/// True if `directive` starts a line of `body`, as `g.is_special`.
pub fn is_special(body: &str, directive: &str) -> bool {
    let mut i = 0usize;
    while i < body.len() {
        if crate::util::match_word(body, i, directive) {
            return true;
        }
        i = crate::util::skip_line(body, i);
    }
    false
}

/// True if the headline starts with @ignore, or a body line does.
pub fn is_at_ignore_node(h: &str, b: &str) -> bool {
    crate::util::match_word(h, 0, "@ignore") || is_special(b, "@ignore")
}

/// Compare a headline against a section name, ignoring case, whitespace and
/// leading periods, as `v.matchHeadline`.
pub fn match_headline(h: &str, pattern: &str) -> bool {
    let norm = |s: &str| -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| *c != ' ' && *c != '\t')
            .collect()
    };
    let h = norm(h);
    let h = h.trim_start_matches('.');
    h.starts_with(&norm(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directive_names_are_whole_words() {
        assert_eq!(any_at_file_node_name("@file x.py"), "x.py");
        assert_eq!(any_at_file_node_name("@file-thin x.py"), "x.py");
        assert_eq!(any_at_file_node_name("@filex x.py"), "");
        assert_eq!(any_at_file_node_name("not a file node"), "");
    }

    #[test]
    fn a_bare_directive_is_not_a_file_node() {
        // No file name, so nothing to write: Leo answers "" here, and code
        // that tests `is_any_at_file_node` depends on it.
        assert_eq!(any_at_file_node_name("@file"), "");
        assert!(!is_any_at_file_node("@clean  "));
    }

    #[test]
    fn indented_directives_do_not_count() {
        assert_eq!(any_at_file_node_name("  @clean a/b.c"), "");
    }

    #[test]
    fn section_names_match_loosely() {
        assert!(match_headline("<< My Section >>", "<<mysection>>"));
        assert!(match_headline(".<< s >>", "<<s>>"));
        assert!(!match_headline("<< other >>", "<<s>>"));
    }
}
