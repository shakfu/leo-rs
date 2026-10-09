//! What leogui's rendered view shows for a node, as Leo's `viewrendered`
//! chooses it: a headline directive first (`@image`, `@md`), then the
//! language in effect. Kept here, with no renderer, so it can be tested.

use std::path::{Path, PathBuf};

use leolib::{Outline, Position};

/// What to show for a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rendered {
    /// Markdown, and the directory its relative image paths start from.
    Markdown { text: String, base: PathBuf },
    /// An `@image` node's file, or why it cannot be shown.
    Image(Result<PathBuf, String>),
    /// Text in a language there is no renderer for here, shown as it is.
    Text { language: String, text: String },
    /// Nothing to render, and why.
    Nothing(String),
}

/// What the rendered view shows for p, whose body is `body`: the text being
/// typed, which the outline does not hold until INSERT ends.
pub fn rendered(o: &Outline, p: &Position, body: &str) -> Rendered {
    let changed = crate::plugins::rendered(o, p, body);
    let body = changed.as_deref().unwrap_or(body);
    let base = base_dir(o);
    let h = p.h(o);
    if let Some(rest) = headline_directive(h, "@image") {
        return Rendered::Image(image_path(&base, rest, body));
    }
    if headline_directive(h, "@md").is_some() {
        return Rendered::Markdown {
            text: plain_fences(&without_directives(body)),
            base,
        };
    }
    // A kind's markdown, as an `@entangled` heading with its code in place.
    let markdown = leolib::ext::kind_at(o, p).and_then(|k| k.node_markdown(o, p));
    if let Some(text) = markdown {
        return Rendered::Markdown {
            text: plain_fences(&text),
            base,
        };
    }
    match o.language_at(p).as_deref() {
        Some("md" | "markdown") => Rendered::Markdown {
            text: plain_fences(&without_directives(body)),
            base,
        },
        Some(lang @ ("rest" | "rst" | "restructuredtext")) => Rendered::Text {
            language: lang.to_string(),
            text: without_directives(body),
        },
        Some(lang) => Rendered::Nothing(format!(
            "nothing to render for {lang}: markdown, @md and @image nodes are rendered"
        )),
        None => Rendered::Nothing(
            "nothing to render: markdown, @md and @image nodes are rendered".to_string(),
        ),
    }
}

/// The rest of `h` after `word`, if `h` starts with it as a whole word.
fn headline_directive<'a>(h: &'a str, word: &str) -> Option<&'a str> {
    let rest = h.strip_prefix(word)?;
    match rest.chars().next() {
        None => Some(""),
        Some(c) if c.is_whitespace() => Some(rest.trim()),
        Some(_) => None,
    }
}

/// The directory of the outline's file, where Leo resolves a relative path;
/// the current directory for an unsaved outline.
fn base_dir(o: &Outline) -> PathBuf {
    let dir = match o.file_name.is_empty() {
        true => None,
        false => std::path::absolute(&o.file_name)
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf)),
    };
    dir.or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into())
}

/// An `@image` node's file: the first line of its body, else its headline
/// after `@image`, as Leo takes it.
fn image_path(base: &Path, headline_rest: &str, body: &str) -> Result<PathBuf, String> {
    let named = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or(headline_rest);
    if named.is_empty() {
        return Err("@image: no file named: put its path on the body's first line".to_string());
    }
    let path = PathBuf::from(shellexpand_home(named));
    let path = match path.is_absolute() {
        true => path,
        false => base.join(path),
    };
    match path.is_file() {
        true => Ok(path),
        false => Err(format!("@image: file not found: {}", path.display())),
    }
}

/// `~/x` as a path under the home directory.
fn shellexpand_home(s: &str) -> String {
    match (s.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest).to_string_lossy().to_string(),
        _ => s.to_string(),
    }
}

/// `body` without Leo's directive lines, as `viewrendered` shows it.
fn without_directives(body: &str) -> String {
    leolib::util::split_lines(body)
        .into_iter()
        .filter(|l| crate::highlight::directive_at(l).is_none())
        .collect()
}

/// `text` with each opening fence's info string cut to its language:
/// `python #add file=add.py` to `python`, `{.rust #main}` to `rust`. A
/// renderer takes the whole info string as the language and colours none.
fn plain_fences(text: &str) -> String {
    let mut out = String::new();
    let mut open: Option<String> = None;
    for line in leolib::util::split_lines(text) {
        let trimmed = line.trim_start_matches(' ');
        let indent = &line[..line.len() - trimmed.len()];
        let run: String = trimmed
            .chars()
            .take_while(|c| *c == '`' || *c == '~')
            .collect();
        let fence = indent.len() <= 3
            && run.len() >= 3
            && run.chars().all(|c| c == run.chars().next().unwrap());
        match (&open, fence) {
            (None, true) => {
                let info = trimmed[run.len()..].trim();
                let tokens: Vec<&str> = info
                    .split(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '}'))
                    .filter(|t| !t.is_empty())
                    .collect();
                let language = tokens
                    .iter()
                    .find_map(|t| t.strip_prefix('.'))
                    .or_else(|| {
                        tokens
                            .first()
                            .copied()
                            .filter(|t| !t.starts_with('#') && !t.contains('='))
                    })
                    .unwrap_or("");
                out.push_str(&format!("{indent}{run}{language}\n"));
                open = Some(run);
            }
            (Some(marker), true)
                if trimmed.trim_end().chars().all(|c| marker.starts_with(c))
                    && trimmed.trim_end().len() >= marker.len() =>
            {
                out.push_str(&line);
                open = None;
            }
            _ => out.push_str(&line),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Document;

    /// An outline in `dir` with one node headlined `h` with body `b`.
    fn node(dir: &Path, h: &str, b: &str) -> (Document, Position) {
        let mut doc = Document::new_empty(&dir.join("x.leo").to_string_lossy());
        let p = doc.outline().root_position().unwrap();
        doc.set_headline(&p, h);
        doc.set_body(&p, b);
        (doc, p)
    }

    fn shown(doc: &Document, p: &Position) -> Rendered {
        rendered(doc.outline(), p, p.b(doc.outline()))
    }

    #[test]
    fn markdown_is_rendered_without_leos_directives() {
        let dir = std::env::temp_dir();
        let (doc, p) = node(&dir, "notes", "@language md\n# Title\n\nText.\n");
        let Rendered::Markdown { text, base } = shown(&doc, &p) else {
            panic!("{:?}", shown(&doc, &p))
        };
        assert_eq!(text, "# Title\n\nText.\n");
        assert_eq!(base, std::path::absolute(&dir).unwrap());
        // `@md` in the headline is markdown whatever the language.
        let (doc, p) = node(&dir, "@md readme", "@language python\n*x*\n");
        assert!(matches!(shown(&doc, &p), Rendered::Markdown { text, .. } if text == "*x*\n"));
    }

    #[test]
    fn the_text_being_typed_is_what_is_rendered() {
        let (doc, p) = node(&std::env::temp_dir(), "notes", "@language md\nold\n");
        let r = rendered(doc.outline(), &p, "@language md\nnew\n");
        assert!(matches!(r, Rendered::Markdown { text, .. } if text == "new\n"));
    }

    #[test]
    fn an_image_node_names_its_file_on_its_first_body_line() {
        let dir = std::env::temp_dir().join(format!("leoapp-rendered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pic.png"), b"not really a png").unwrap();
        let (doc, p) = node(&dir, "@image", "pic.png\nignored\n");
        assert_eq!(shown(&doc, &p), Rendered::Image(Ok(dir.join("pic.png"))));
        let (doc, p) = node(&dir, "@image pic.png", "");
        assert_eq!(shown(&doc, &p), Rendered::Image(Ok(dir.join("pic.png"))));
        let (doc, p) = node(&dir, "@image", "missing.png\n");
        assert!(matches!(shown(&doc, &p), Rendered::Image(Err(e)) if e.contains("not found")));
        let (doc, p) = node(&dir, "@image", "");
        assert!(matches!(shown(&doc, &p), Rendered::Image(Err(e)) if e.contains("no file named")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fences_info_string_is_cut_to_its_language_for_colouring() {
        let text = "```python #add file=add.py\n```not a fence inside\n```\n\n\
                    ```text {.rust #main}\nfn main() {}\n```\n\n~~~{python, label=k}\nk\n~~~\n";
        assert_eq!(
            plain_fences(text),
            "```python\n```not a fence inside\n```\n\n```rust\nfn main() {}\n```\n\n~~~python\nk\n~~~\n"
        );
    }

    #[test]
    fn rst_is_text_and_code_is_nothing() {
        let dir = std::env::temp_dir();
        let (doc, p) = node(&dir, "doc", "@language rest\nTitle\n=====\n");
        assert_eq!(
            shown(&doc, &p),
            Rendered::Text {
                language: "rest".into(),
                text: "Title\n=====\n".into()
            }
        );
        let (doc, p) = node(&dir, "code", "@language python\nx = 1\n");
        assert!(matches!(shown(&doc, &p), Rendered::Nothing(why) if why.contains("python")));
    }
}
