//! The YAML of a jupytext header: PyYAML's `safe_dump` and `safe_load`,
//! for the block mappings and sequences jupytext writes there.
//!
//! What PyYAML would write in a style not ported here -- double quotes,
//! a line folded at 80 columns, a key over 128 characters -- is refused.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Number, Value};

/// PyYAML's best width: a scalar is folded at a single space past it.
const WIDTH: usize = 80;

/// `yaml.safe_dump(v, default_flow_style=False)` for a mapping, as lines.
pub fn dump(v: &Map<String, Value>) -> Result<Vec<String>, String> {
    let mut lines = Vec::new();
    mapping(v, 0, &mut lines)?;
    Ok(lines)
}

fn mapping(m: &Map<String, Value>, indent: usize, lines: &mut Vec<String>) -> Result<(), String> {
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    for k in keys {
        if k.chars().count() > 128 {
            return Err(format!("the header key {k:?} is too long"));
        }
        let head = format!("{}{}:", " ".repeat(indent), scalar_str(k, indent, true)?);
        node(&m[k.as_str()], head, indent, true, lines)?;
    }
    Ok(())
}

/// Write `v` after `head`, a key or a `-` already on the line.
fn node(
    v: &Value,
    head: String,
    indent: usize,
    in_mapping: bool,
    lines: &mut Vec<String>,
) -> Result<(), String> {
    match v {
        Value::Object(m) if !m.is_empty() => {
            lines.push(head);
            mapping(m, indent + 2, lines)
        }
        Value::Array(a) if !a.is_empty() => {
            lines.push(head);
            // A sequence in a mapping is not indented; in a sequence it is.
            let inner = if in_mapping { indent } else { indent + 2 };
            for x in a {
                match x {
                    Value::Array(b) if !b.is_empty() => {
                        return Err("a sequence in a sequence in the header is not ported".into())
                    }
                    Value::Object(m) if !m.is_empty() => {
                        let mut sub = Vec::new();
                        mapping(m, inner + 2, &mut sub)?;
                        let first = sub.remove(0);
                        lines.push(format!("{}- {}", " ".repeat(inner), &first[inner + 2..]));
                        lines.extend(sub);
                    }
                    _ => node(x, format!("{}-", " ".repeat(inner)), inner, false, lines)?,
                }
            }
            Ok(())
        }
        _ => {
            let column = head.chars().count() + 1;
            lines.push(format!("{head} {}", scalar(v, column)?));
            Ok(())
        }
    }
}

fn scalar(v: &Value, column: usize) -> Result<String, String> {
    Ok(match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) if n.is_f64() => {
            let r = super::json::float_repr(n.as_f64().unwrap_or(0.0)).to_lowercase();
            match r.as_str() {
                "nan" => ".nan".to_string(),
                "infinity" => ".inf".to_string(),
                "-infinity" => "-.inf".to_string(),
                _ if !r.contains('.') && r.contains('e') => r.replacen('e', ".0e", 1),
                _ => r,
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => scalar_str(s, column, false)?,
        Value::Array(_) => "[]".to_string(),
        Value::Object(_) => "{}".to_string(),
    })
}

/// A string as PyYAML's `choose_scalar_style` writes it: plain if it reads
/// back as the same string, else in single quotes.
fn scalar_str(s: &str, column: usize, key: bool) -> Result<String, String> {
    let a = analyze(s);
    let refuse = || {
        Err(format!(
            "the header value {s:?} needs a YAML style not ported"
        ))
    };
    if a.special || a.line_breaks {
        return refuse();
    }
    let plain = !(key && a.empty) && a.block_plain && resolve(s).is_none();
    if !plain && !a.single_quoted {
        return refuse();
    }
    if folds(s, column + usize::from(!plain), !plain) && !key {
        return refuse();
    }
    Ok(match plain {
        true => s.to_string(),
        false => format!("'{}'", s.replace('\'', "''")),
    })
}

/// Whether PyYAML would break the line at a single space past the width.
fn folds(s: &str, start: usize, quoted: bool) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let mut column = start;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ' ' {
            let j = (i..chars.len())
                .find(|&j| chars[j] != ' ')
                .unwrap_or(chars.len());
            let inner = !quoted || (i != 0 && j != chars.len());
            if j == i + 1 && column > WIDTH && inner {
                return true;
            }
            column += j - i;
            i = j;
        } else {
            column += 1;
            i += 1;
        }
    }
    false
}

struct Analysis {
    empty: bool,
    line_breaks: bool,
    special: bool,
    block_plain: bool,
    single_quoted: bool,
}

/// PyYAML's `Emitter.analyze_scalar`, with `allow_unicode` off.
fn analyze(s: &str) -> Analysis {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return Analysis {
            empty: true,
            line_breaks: false,
            special: false,
            block_plain: true,
            single_quoted: true,
        };
    }
    let ws = |c: char| "\0 \t\r\n\u{85}\u{2028}\u{2029}".contains(c);
    let brk = |c: char| "\n\u{85}\u{2028}\u{2029}".contains(c);
    let mut block_indicators = s.starts_with("---") || s.starts_with("...");
    let (mut line_breaks, mut special) = (false, false);
    let (mut leading, mut trailing, mut break_space, mut space_break) =
        (false, false, false, false);
    let (mut previous_space, mut previous_break) = (false, false);
    let n = chars.len();
    for (i, &ch) in chars.iter().enumerate() {
        let preceded = i == 0 || ws(chars[i - 1]);
        let followed = i + 1 >= n || ws(chars[i + 1]);
        if i == 0 {
            if "#,[]{}&*!|>'\"%@`".contains(ch) || (ch == '-' && followed) {
                block_indicators = true;
            }
            if "?:".contains(ch) && followed {
                block_indicators = true;
            }
        } else if (ch == ':' && followed) || (ch == '#' && preceded) {
            block_indicators = true;
        }
        if brk(ch) {
            line_breaks = true;
        }
        if !(ch == '\n' || (' '..='~').contains(&ch)) {
            special = true;
        }
        if ch == ' ' {
            leading |= i == 0;
            trailing |= i == n - 1;
            break_space |= previous_break;
            previous_space = true;
            previous_break = false;
        } else if brk(ch) {
            leading |= i == 0;
            trailing |= i == n - 1;
            space_break |= previous_space;
            previous_space = false;
            previous_break = true;
        } else {
            previous_space = false;
            previous_break = false;
        }
    }
    let mut block_plain = !(leading || trailing || break_space || space_break || special);
    block_plain &= !line_breaks && !block_indicators;
    Analysis {
        empty: false,
        line_breaks,
        special,
        block_plain,
        single_quoted: !(break_space || space_break || special),
    }
}

static BOOL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF)$")
        .unwrap()
});
static FLOAT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
    )
    .unwrap()
});
static INT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$",
    )
    .unwrap()
});
static NULL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?:~|null|Null|NULL|)$").unwrap());
static TIMESTAMP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(?:[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?)$",
    )
    .unwrap()
});

/// The tag PyYAML's resolver gives a plain scalar, if it is not a string.
fn resolve(s: &str) -> Option<&'static str> {
    if BOOL.is_match(s) {
        Some("bool")
    } else if FLOAT.is_match(s) {
        Some("float")
    } else if INT.is_match(s) {
        Some("int")
    } else if NULL.is_match(s) {
        Some("null")
    } else if TIMESTAMP.is_match(s) {
        Some("timestamp")
    } else if s == "<<" || s == "=" {
        Some("other")
    } else {
        None
    }
}

/// `yaml.safe_load` for the block style `dump` writes.
pub fn load(lines: &[String]) -> Result<Value, String> {
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.as_str())
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .collect();
    let mut i = 0;
    let v = parse_block(&lines, &mut i, 0, 0)?;
    match i == lines.len() {
        true => Ok(v),
        false => Err(format!("cannot read the header line {:?}", lines[i])),
    }
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// How deep the header's YAML may nest.
const MAX_DEPTH: usize = 64;

fn parse_block(
    lines: &[&str],
    i: &mut usize,
    indent: usize,
    depth: usize,
) -> Result<Value, String> {
    let Some(first) = lines.get(*i) else {
        return Ok(Value::Null);
    };
    if depth > MAX_DEPTH {
        return Err("the header nests too deep".to_string());
    }
    if indent_of(first) != indent {
        return Err(format!("cannot read the header line {first:?}"));
    }
    if is_item(&first[indent..]) {
        let mut items = Vec::new();
        while *i < lines.len() && indent_of(lines[*i]) == indent && is_item(&lines[*i][indent..]) {
            let rest = lines[*i][indent + 1..].trim_start_matches(' ');
            if rest.is_empty() {
                *i += 1;
                items.push(nested(lines, i, indent, true, depth)?);
            } else if key_value(rest).is_some() {
                // A mapping that starts on the `-` line.
                let column = lines[*i].len() - rest.len();
                let mut sub: Vec<String> = vec![format!("{}{rest}", " ".repeat(column))];
                *i += 1;
                while *i < lines.len() && indent_of(lines[*i]) >= column {
                    sub.push(lines[*i].to_string());
                    *i += 1;
                }
                let sub: Vec<&str> = sub.iter().map(|s| s.as_str()).collect();
                let mut j = 0;
                items.push(parse_block(&sub, &mut j, column, depth + 1)?);
            } else {
                *i += 1;
                items.push(parse_scalar(rest)?);
            }
        }
        return Ok(Value::Array(items));
    }
    let mut m = Map::new();
    while *i < lines.len() && indent_of(lines[*i]) == indent && !is_item(&lines[*i][indent..]) {
        let line = &lines[*i][indent..];
        let (k, rest) =
            key_value(line).ok_or_else(|| format!("cannot read the header line {line:?}"))?;
        let k = match parse_scalar(k)? {
            Value::String(s) => s,
            other => other.to_string(),
        };
        *i += 1;
        let v = match rest.is_empty() {
            true => nested(lines, i, indent, false, depth)?,
            false => parse_scalar(rest)?,
        };
        m.insert(k, v);
    }
    Ok(Value::Object(m))
}

/// The value under a key or `-` with nothing after it on its line.
fn nested(
    lines: &[&str],
    i: &mut usize,
    indent: usize,
    item: bool,
    depth: usize,
) -> Result<Value, String> {
    match lines.get(*i).map(|l| indent_of(l)) {
        Some(n) if n > indent => parse_block(lines, i, n, depth + 1),
        // An indentless sequence under a mapping key.
        Some(n) if n == indent && !item && is_item(&lines[*i][n..]) => {
            parse_block(lines, i, n, depth + 1)
        }
        _ => Ok(Value::Null),
    }
}

fn is_item(s: &str) -> bool {
    s == "-" || s.starts_with("- ")
}

/// Split `key: value` at the first `: ` outside quotes.
fn key_value(s: &str) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
    let b: Vec<(usize, char)> = s.char_indices().collect();
    for (n, &(pos, c)) in b.iter().enumerate() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if n == 0 && (c == '\'' || c == '"') => quote = Some(c),
            None if c == ':' => {
                let next = b.get(n + 1).map(|x| x.1);
                if next.is_none() || next == Some(' ') {
                    return Some((s[..pos].trim_end(), s[pos + 1..].trim()));
                }
            }
            None => {}
        }
    }
    None
}

fn parse_scalar(s: &str) -> Result<Value, String> {
    let s = s.trim();
    let bad = || format!("cannot read the header value {s:?}");
    if let Some(inner) = s.strip_prefix('\'') {
        let inner = inner.strip_suffix('\'').ok_or_else(bad)?;
        return Ok(Value::String(inner.replace("''", "'")));
    }
    if s.starts_with('"') {
        return serde_json::from_str(s).map_err(|_| bad());
    }
    if s == "{}" {
        return Ok(Value::Object(Map::new()));
    }
    if s == "[]" {
        return Ok(Value::Array(Vec::new()));
    }
    // A comment ends a plain scalar.
    let s = s.find(" #").map_or(s, |n| s[..n].trim_end());
    Ok(match resolve(s) {
        None => Value::String(s.to_string()),
        Some("null") => Value::Null,
        Some("bool") => Value::Bool(matches!(s.to_lowercase().as_str(), "yes" | "true" | "on")),
        Some("int") => {
            let digits = s.replace('_', "");
            let unsigned = digits.trim_start_matches(['-', '+']);
            if unsigned.len() > 1 && unsigned.starts_with('0') {
                return Err(bad());
            }
            let n: i64 = digits.parse().map_err(|_| bad())?;
            Value::Number(n.into())
        }
        Some("float") => {
            let f: f64 = s.replace('_', "").parse().map_err(|_| bad())?;
            Value::Number(Number::from_f64(f).ok_or_else(bad)?)
        }
        _ => return Err(bad()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(s: &str) -> Map<String, Value> {
        match serde_json::from_str(s).unwrap() {
            Value::Object(m) => m,
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_header_dumps_as_pyyaml_dumps_it_and_loads_back() {
        let m = obj(
            r#"{"jupyter": {"kernelspec": {"display_name": "Python 3 (ipykernel)",
                "language": "python", "name": "python3"},
                "jupytext": {"text_representation": {"format_version": "1.3",
                "extension": ".py"}, "cell_metadata_filter": "-all"},
                "x": {"list": ["a", 1, {"k": "v", "j": null}], "e": "", "t": "yes",
                "f": 1e-05, "q": "it's: here", "empty": {}}}}"#,
        );
        let lines = dump(&m).unwrap();
        let want = [
            "jupyter:",
            "  jupytext:",
            "    cell_metadata_filter: -all",
            "    text_representation:",
            "      extension: .py",
            "      format_version: '1.3'",
            "  kernelspec:",
            "    display_name: Python 3 (ipykernel)",
            "    language: python",
            "    name: python3",
            "  x:",
            "    e: ''",
            "    empty: {}",
            "    f: 1.0e-05",
            "    list:",
            "    - a",
            "    - 1",
            "    - j: null",
            "      k: v",
            "    q: 'it''s: here'",
            "    t: 'yes'",
        ];
        assert_eq!(lines, want);
        assert_eq!(load(&lines).unwrap(), Value::Object(m));
    }

    #[test]
    fn a_value_pyyaml_would_double_quote_is_refused() {
        assert!(dump(&obj("{\"a\": \"caf\u{e9}\"}")).is_err());
        assert!(dump(&obj(r#"{"a": "two\nlines"}"#)).is_err());
    }
}
