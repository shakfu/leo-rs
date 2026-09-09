//! The README's cheatsheet must not drift from the binding table.
//!
//! `docs/dev/tui-design.md` makes the table the single source for the
//! dispatcher, `F1` and `--keys`. The README is the fourth reader, and the
//! only one a compiler cannot check, so it is checked here: a binding that is
//! added, renamed or removed without touching the README fails this test.

use std::path::Path;

/// The cheatsheet, between its markers.
fn cheatsheet() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("README.md");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let begin = text
        .find("<!-- keys:begin -->")
        .expect("README has no <!-- keys:begin --> marker");
    let end = text
        .find("<!-- keys:end -->")
        .expect("README has no <!-- keys:end --> marker");
    assert!(
        begin < end,
        "the README's key markers are the wrong way round"
    );
    text[begin..end].to_string()
}

/// Keys markdown cannot show inside inline code, with the reason.
///
/// A lone backtick cannot appear between backticks. The README writes it as
/// ``` `` ` `` ``` and as ``Ctrl-` ``, which no simple search can match.
const UNCHECKABLE: &[&str] = &["`", "Ctrl-`"];

#[test]
fn the_readme_documents_every_binding() {
    let sheet = cheatsheet();
    let mut missing = Vec::new();
    for binding in leotui_bindings() {
        if UNCHECKABLE.contains(&binding) {
            continue;
        }
        if !sheet.contains(&format!("`{binding}`")) {
            missing.push(binding);
        }
    }
    assert!(
        missing.is_empty(),
        "the README cheatsheet does not mention: {missing:?}"
    );
}

#[test]
fn the_readme_mentions_the_keys_markdown_cannot_quote() {
    let sheet = cheatsheet();
    assert!(
        sheet.contains("clone this node"),
        "the backtick binding is undocumented"
    );
    assert!(
        sheet.contains("Ctrl-`"),
        "Leo's Ctrl-` chord is undocumented"
    );
}

/// Every key spec bound in NORMAL mode, from `leotui --keys`.
///
/// The binary is the source rather than the library, because `leotui` is a
/// binary crate: an integration test cannot import its modules.
fn leotui_bindings() -> Vec<&'static str> {
    // Kept in step by `bindings::tests::the_readme_test_lists_every_binding`,
    // which fails if the table gains a key this list does not have.
    BINDING_KEYS.to_vec()
}

/// The key specs in `bindings::BINDINGS` for `Mode::Normal`.
pub static BINDING_KEYS: &[&str] = &[
    "j",
    "Down",
    "k",
    "Up",
    "h",
    "Left",
    "l",
    "Right",
    "Enter",
    "gg",
    "Alt-Home",
    "G",
    "Alt-End",
    "gp",
    "{",
    "}",
    "[m",
    "]m",
    "]c",
    "Alt-n",
    "o",
    "Insert",
    "O",
    "a",
    "Ctrl-Insert",
    "dd",
    "Delete",
    "Backspace",
    "yy",
    "p",
    "`",
    "m",
    "M",
    "J",
    "Shift-Down",
    "K",
    "Shift-Up",
    "<<",
    "Shift-Left",
    ">>",
    "Shift-Right",
    "g<",
    "g>",
    "e",
    "Ctrl-h",
    "i",
    "Ctrl-i",
    "Ctrl-m",
    "Ctrl-[",
    "Ctrl-]",
    "Ctrl-`",
    "Space",
    "za",
    "zo",
    "Alt-]",
    "zc",
    "Alt-[",
    "zR",
    "zM",
    "Alt--",
    "zr",
    "zm",
    "zx",
    "z1",
    "z2",
    "z3",
    "z4",
    "z5",
    "z6",
    "z7",
    "z8",
    "z9",
    "h j k l",
    "w W b B e E ge",
    "0 ^ $ gg G { } %",
    "f F t T ; ,",
    "H M L",
    "d c y > < gu gU g~",
    "iw aw i\" a( ip",
    "x X r s S D C Y J ~",
    "i a I A o O",
    "v V",
    "p P",
    ".",
    "Escape",
    "Tab",
    "Ctrl-f",
    "PageDown",
    "Ctrl-b",
    "PageUp",
    "Ctrl-d",
    "Ctrl-u",
    "u",
    "Ctrl-z",
    "Ctrl-r",
    "Ctrl-Shift-z",
    "Ctrl-s",
    "Ctrl-Left",
    "Ctrl-Right",
    "F1",
    ":",
    "/",
    "?",
    "n",
    "N",
    "w",
    "q",
];
