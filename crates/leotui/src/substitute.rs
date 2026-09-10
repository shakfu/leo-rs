//! vim's `:s`, over the lines of one body.
//!
//! `[range]s/pattern/replacement/[flags]`. The pattern is a `regex` crate
//! regex, not vim's dialect. The replacement takes vim's `&`, `\0`-`\9` and
//! `\r`. Flags: `g`, `i`, `I`, `n`.

use regex::{Captures, Regex, RegexBuilder};

/// A line address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addr {
    /// `.`, the cursor's line.
    Current,
    /// `$`.
    Last,
    /// A line number, counting from 1.
    Line(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Substitute {
    /// First and last line. None is the cursor's line.
    pub range: Option<(Addr, Addr)>,
    /// Empty means the last search.
    pub pattern: String,
    pub replacement: String,
    pub global: bool,
    /// None is smartcase, as `/` uses. `i` sets true, `I` false.
    pub ignore_case: Option<bool>,
    /// `n`: count the matches, change nothing.
    pub count_only: bool,
}

/// What a substitute did.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub count: usize,
    pub lines: usize,
    /// The last line changed, for the cursor.
    pub last_line: usize,
}

/// Parse a `:` line, without its `:`. None if it is not a substitute.
pub fn parse(line: &str) -> Option<Result<Substitute, String>> {
    let (range, rest) = parse_range(line);
    let rest = rest
        .strip_prefix("substitute")
        .or_else(|| rest.strip_prefix('s'))?;
    let delim = rest.chars().next()?;
    if delim.is_alphanumeric() || delim.is_whitespace() || matches!(delim, '\\' | '"' | '|') {
        return None;
    }
    let (pattern, after) = take_field(&rest[delim.len_utf8()..], delim);
    let (replacement, flags) = match after {
        Some(s) => {
            let (r, f) = take_field(s, delim);
            (r, f.unwrap_or(""))
        }
        None => (String::new(), ""),
    };
    let mut sub = Substitute {
        range,
        pattern,
        replacement,
        global: false,
        ignore_case: None,
        count_only: false,
    };
    for c in flags.trim_end().chars() {
        match c {
            'g' => sub.global = true,
            'i' => sub.ignore_case = Some(true),
            'I' => sub.ignore_case = Some(false),
            'n' => sub.count_only = true,
            'c' => return Some(Err("substitute: the c flag is not supported".into())),
            other => return Some(Err(format!("substitute: unknown flag {other}"))),
        }
    }
    Some(Ok(sub))
}

/// `%`, `N`, `N,M`, with `.` and `$` for N or M.
fn parse_range(s: &str) -> (Option<(Addr, Addr)>, &str) {
    if let Some(rest) = s.strip_prefix('%') {
        return (Some((Addr::Line(1), Addr::Last)), rest);
    }
    let Some((first, rest)) = parse_addr(s) else {
        return (None, s);
    };
    match rest.strip_prefix(',') {
        Some(after) => match parse_addr(after) {
            Some((second, rest)) => (Some((first, second)), rest),
            None => (Some((first, Addr::Current)), after),
        },
        None => (Some((first, first)), rest),
    }
}

fn parse_addr(s: &str) -> Option<(Addr, &str)> {
    if let Some(rest) = s.strip_prefix('.') {
        return Some((Addr::Current, rest));
    }
    if let Some(rest) = s.strip_prefix('$') {
        return Some((Addr::Last, rest));
    }
    let digits = s.len() - s.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let n = s[..digits].parse().ok()?;
    Some((Addr::Line(n), &s[digits..]))
}

/// Text up to an unescaped `delim`, and what follows it. `\delim` is `delim`.
fn take_field(s: &str, delim: char) -> (String, Option<&str>) {
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == delim {
            return (out, Some(&s[i + c.len_utf8()..]));
        }
        if c == '\\' {
            match chars.next() {
                Some((_, d)) if d == delim => out.push(d),
                Some((_, d)) => {
                    out.push('\\');
                    out.push(d);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    (out, None)
}

/// Apply `sub` to `lines`, with the cursor on line `cursor`, counting from 0.
///
/// `last_search` is the `/` pattern, used when the pattern is empty. A
/// replacement containing `\r` splits its line.
pub fn apply(
    sub: &Substitute,
    lines: &mut Vec<String>,
    cursor: usize,
    last_search: Option<&str>,
) -> Result<Outcome, String> {
    let re = compile(sub, last_search)?;
    let (a, b) = match sub.range {
        None => (cursor, cursor),
        Some((x, y)) => (
            resolve(x, cursor, lines.len())?,
            resolve(y, cursor, lines.len())?,
        ),
    };
    let (first, last) = (a.min(b), a.max(b));
    let template = parse_template(&sub.replacement);
    let (mut count, mut changed, mut last_line) = (0, 0, first);
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let n = match (first..=last).contains(&i) {
            true if sub.global => re.find_iter(line).count(),
            true => usize::from(re.is_match(line)),
            false => 0,
        };
        if n == 0 || sub.count_only {
            if n > 0 {
                last_line = out.len();
            }
            out.push(line.clone());
        } else {
            let limit = if sub.global { 0 } else { 1 };
            let new = re.replacen(line, limit, |caps: &Captures| expand(&template, caps));
            out.extend(new.split('\n').map(str::to_string));
            last_line = out.len() - 1;
        }
        count += n;
        changed += usize::from(n > 0);
    }
    if count == 0 {
        return Err(format!("pattern not found: {}", re.as_str()));
    }
    if !sub.count_only {
        *lines = out;
    }
    Ok(Outcome {
        count,
        lines: changed,
        last_line,
    })
}

fn compile(sub: &Substitute, last_search: Option<&str>) -> Result<Regex, String> {
    let pattern = match sub.pattern.is_empty() {
        true => last_search
            .ok_or("substitute: no previous search")?
            .to_string(),
        false => sub.pattern.clone(),
    };
    let ignore = sub
        .ignore_case
        .unwrap_or_else(|| !crate::search::has_capital(&pattern));
    RegexBuilder::new(&pattern)
        .case_insensitive(ignore)
        .build()
        .map_err(|e| {
            let detail = e.to_string();
            format!(
                "substitute: {}",
                detail.lines().last().unwrap_or("bad pattern")
            )
        })
}

fn resolve(addr: Addr, cursor: usize, len: usize) -> Result<usize, String> {
    match addr {
        Addr::Current => Ok(cursor),
        Addr::Last => Ok(len.saturating_sub(1)),
        Addr::Line(n) if (1..=len).contains(&n) => Ok(n - 1),
        Addr::Line(n) => Err(format!("substitute: no line {n}; the body has {len}")),
    }
}

enum Piece {
    Text(String),
    Group(usize),
}

/// vim's replacement: `&` and `\0` are the match, `\1`-`\9` its groups, `\r`
/// and `\n` a new line, `\t` a tab, and a backslash before anything else
/// makes it literal.
fn parse_template(s: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        let group = match c {
            '&' => Some(0),
            '\\' => match chars.next() {
                Some(d) if d.is_ascii_digit() => Some(d as usize - '0' as usize),
                Some('r' | 'n') => {
                    text.push('\n');
                    None
                }
                Some('t') => {
                    text.push('\t');
                    None
                }
                Some(other) => {
                    text.push(other);
                    None
                }
                None => {
                    text.push('\\');
                    None
                }
            },
            c => {
                text.push(c);
                None
            }
        };
        if let Some(n) = group {
            if !text.is_empty() {
                out.push(Piece::Text(std::mem::take(&mut text)));
            }
            out.push(Piece::Group(n));
        }
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    out
}

fn expand(template: &[Piece], caps: &Captures) -> String {
    let mut out = String::new();
    for piece in template {
        match piece {
            Piece::Text(t) => out.push_str(t),
            Piece::Group(n) => out.push_str(caps.get(*n).map_or("", |m| m.as_str())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(line: &str) -> Substitute {
        parse(line)
            .expect("not a substitute")
            .expect("did not parse")
    }

    fn run(line: &str, text: &[&str], cursor: usize) -> Result<(Vec<String>, Outcome), String> {
        let mut lines: Vec<String> = text.iter().map(|s| s.to_string()).collect();
        let outcome = apply(&sub(line), &mut lines, cursor, Some("a.b"))?;
        Ok((lines, outcome))
    }

    #[test]
    fn the_parts_are_read_as_vim_reads_them() {
        let s = sub("s/a/b/");
        assert_eq!(
            (s.range, s.pattern.as_str(), s.replacement.as_str()),
            (None, "a", "b")
        );
        assert!(!s.global);
        let s = sub("%s/a/b/gi");
        assert_eq!(s.range, Some((Addr::Line(1), Addr::Last)));
        assert!(s.global && s.ignore_case == Some(true));
        let s = sub("2,$substitute#x/y#z#n");
        assert_eq!(s.range, Some((Addr::Line(2), Addr::Last)));
        assert_eq!(s.pattern, "x/y");
        assert!(s.count_only);
        assert_eq!(sub(r"s/a\/b/c").pattern, "a/b");
        assert_eq!(sub("s/a").replacement, "");
    }

    #[test]
    fn other_commands_are_not_substitutes() {
        for line in [
            "set wrap",
            "save",
            "search-forward",
            "12",
            "e path",
            "s",
            "theme x",
        ] {
            assert!(parse(line).is_none(), "{line}");
        }
    }

    #[test]
    fn an_unsupported_flag_is_an_error() {
        assert!(parse("s/a/b/c").unwrap().is_err());
        assert!(parse("s/a/b/x").unwrap().is_err());
    }

    #[test]
    fn without_a_range_only_the_cursor_line_changes() {
        let (lines, o) = run("s/x/y/", &["x x", "x x"], 1).unwrap();
        assert_eq!(lines, ["x x", "y x"]);
        assert_eq!((o.count, o.lines, o.last_line), (1, 1, 1));
    }

    #[test]
    fn a_range_and_g_change_every_match_in_those_lines() {
        let (lines, o) = run("%s/x/y/g", &["x x", "z", "x"], 0).unwrap();
        assert_eq!(lines, ["y y", "z", "y"]);
        assert_eq!((o.count, o.lines, o.last_line), (3, 2, 2));
        let (lines, _) = run("2,3s/x/y/", &["x", "x", "x"], 0).unwrap();
        assert_eq!(lines, ["x", "y", "y"]);
    }

    #[test]
    fn the_replacement_takes_groups_the_match_and_new_lines() {
        let (lines, _) = run(r"s/(\w+) (\w+)/\2 \1 [&]/", &["hello world"], 0).unwrap();
        assert_eq!(lines, ["world hello [hello world]"]);
        let (lines, o) = run(r"s/, /,\r/g", &["a, b, c", "d"], 0).unwrap();
        assert_eq!(lines, ["a,", "b,", "c", "d"]);
        assert_eq!(o.last_line, 2);
        let (lines, _) = run(r"s/a/\&\$1/", &["a"], 0).unwrap();
        assert_eq!(lines, ["&$1"]);
    }

    #[test]
    fn case_follows_smartcase_unless_a_flag_says_otherwise() {
        assert!(run("s/foo/x/", &["Foo"], 0).is_ok());
        assert!(run("s/Foo/x/", &["foo"], 0).is_err());
        assert!(run("s/foo/x/I", &["Foo"], 0).is_err());
        assert!(run("s/Foo/x/i", &["foo"], 0).is_ok());
        // An escape such as \S is not a capital.
        assert!(run(r"s/\Sx/y/", &["AX"], 0).is_ok());
    }

    #[test]
    fn an_empty_pattern_is_the_last_search() {
        // The last search is a regex, as `/` reads it: `a.b` matches `axb`.
        let (lines, _) = run("s//X/", &["axb a.b"], 0).unwrap();
        assert_eq!(lines, ["X a.b"]);
    }

    #[test]
    fn counting_changes_nothing() {
        let (lines, o) = run("%s/x//gn", &["x x", "x"], 0).unwrap();
        assert_eq!(lines, ["x x", "x"]);
        assert_eq!((o.count, o.lines), (3, 2));
    }

    #[test]
    fn a_missing_match_or_line_or_bad_pattern_is_an_error() {
        assert!(run("s/q/x/", &["x"], 0)
            .unwrap_err()
            .contains("pattern not found"));
        assert!(run("5s/x/y/", &["x"], 0).unwrap_err().contains("no line 5"));
        assert!(run("s/(/y/", &["x"], 0)
            .unwrap_err()
            .starts_with("substitute:"));
    }
}
