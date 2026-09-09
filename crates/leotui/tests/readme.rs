//! The README's cheatsheet must not drift from the binding table.
//!
//! `docs/dev/tui-design.md` makes the table the single source for the
//! dispatcher, `F1` and `--keys`. The README is the fourth reader, and the
//! only one a compiler cannot check, so it is checked here: a binding added,
//! renamed or removed without touching the README fails this test.
//!
//! The bindings come from running the binary, not from a copy of the table.
//! An integration test cannot import a binary crate's modules, and a second
//! copy of a hundred key specs is a thing to forget to update -- which is
//! exactly what this test exists to prevent.

use std::path::Path;
use std::process::Command;

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

/// Every key spec bound in NORMAL mode, straight from the binary.
fn bindings() -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_leotui"))
        .arg("--key-specs")
        .output()
        .expect("could not run leotui --key-specs");
    assert!(out.status.success(), "leotui --key-specs failed");
    String::from_utf8(out.stdout)
        .expect("--key-specs wrote invalid UTF-8")
        .lines()
        .map(|l| l.to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Keys markdown cannot show inside inline code, with the reason.
///
/// A lone backtick cannot appear between backticks. The README writes them as
/// ``` `` ` `` ``` and ``` ``Ctrl-` `` ```, which no simple search can match;
/// the test below checks their descriptions instead.
const UNCHECKABLE: &[&str] = &["`", "Ctrl-`"];

#[test]
fn the_readme_documents_every_binding() {
    let sheet = cheatsheet();
    let all = bindings();
    assert!(
        all.len() > 50,
        "only {} bindings: is --key-specs right?",
        all.len()
    );
    let missing: Vec<&String> = all
        .iter()
        .filter(|spec| !UNCHECKABLE.contains(&spec.as_str()))
        .filter(|spec| !sheet.contains(&format!("`{spec}`")))
        .collect();
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
