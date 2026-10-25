//! Cell and notebook metadata: jupytext's `metadata_filter.py` and
//! `cell_metadata.py`.
//!
//! Metadata is a list of pairs where its order reaches the text: the
//! options on a `# %%` line keep the order of the notebook's keys.

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};

/// Ordered metadata.
pub type Meta = Vec<(String, Value)>;

/// Keys jupytext keeps for itself, never restored from the notebook.
pub const JUPYTEXT_CELL_METADATA: &[&str] = &[
    "skipline",
    "noskipline",
    "cell_marker",
    "lines_to_next_cell",
    "lines_to_end_of_cell_marker",
];

/// `_IGNORE_CELL_METADATA`: cell metadata the text never shows.
pub const IGNORE_CELL_METADATA: &str = "-autoscroll,-collapsed,-scrolled,-trusted,-execution,\
     -ExecuteTime,-skipline,-noskipline,-cell_marker,-lines_to_next_cell,\
     -lines_to_end_of_cell_marker";

/// `_DEFAULT_NOTEBOOK_METADATA`: notebook metadata the header shows.
pub const DEFAULT_NOTEBOOK_METADATA: &str = "jupytext,kernelspec,kernel_info,orphan,tocdepth";

/// Look a key up in ordered metadata.
pub fn get<'a>(m: &'a Meta, key: &str) -> Option<&'a Value> {
    m.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Remove a key from ordered metadata, keeping the others' order.
pub fn pop(m: &mut Meta, key: &str) -> Option<Value> {
    let i = m.iter().position(|(k, _)| k == key)?;
    Some(m.remove(i).1)
}

/// Set a key, in place if present, else at the end.
pub fn set(m: &mut Meta, key: &str, v: Value) {
    match m.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = v,
        None => m.push((key.to_string(), v)),
    }
}

/// Ordered metadata from a JSON object.
pub fn from_map(m: &Map<String, Value>) -> Meta {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// A JSON object from ordered metadata.
pub fn to_map(m: &Meta) -> Map<String, Value> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// One side of a filter: everything, or the keys named.
#[derive(Debug, Clone, PartialEq)]
pub enum Keys {
    /// `all`.
    All,
    /// The keys named.
    Some(Vec<String>),
}

/// `metadata_filter_as_dict`: (additional, excluded); None where absent.
pub fn filter_as_dict(config: Option<&str>) -> (Option<Keys>, Option<Keys>) {
    let Some(config) = config else {
        return (None, None);
    };
    let (mut add, mut exc): (Option<Vec<String>>, Option<Vec<String>>) = (None, None);
    for key in config.split(',') {
        let key = key.trim();
        if let Some(k) = key.strip_prefix('-') {
            exc.get_or_insert_with(Vec::new).push(k.trim().to_string());
        } else if let Some(k) = key.strip_prefix('+') {
            add.get_or_insert_with(Vec::new).push(k.trim().to_string());
        } else {
            add.get_or_insert_with(Vec::new).push(key.to_string());
        }
    }
    let section = |v: Option<Vec<String>>| {
        v.map(|v| match v.iter().any(|k| k == "all") {
            true => Keys::All,
            false => Keys::Some(v.into_iter().filter(|k| !k.is_empty()).collect()),
        })
    };
    (section(add), section(exc))
}

/// `metadata_filter_as_string`.
pub fn filter_as_string(additional: &Keys, excluded: &Keys) -> String {
    let mut entries: Vec<String> = match additional {
        Keys::All => vec!["all".to_string()],
        Keys::Some(keys) => keys
            .iter()
            .filter(|k| !JUPYTEXT_CELL_METADATA.contains(&k.as_str()))
            .cloned()
            .collect(),
    };
    match excluded {
        Keys::All => entries.push("-all".to_string()),
        Keys::Some(keys) => entries.extend(keys.iter().map(|k| format!("-{k}"))),
    }
    entries.join(",")
}

static VALID_KEY: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_.@/-]+$").unwrap());
static IDENTIFIER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-zA-Z_.]+[a-zA-Z0-9_.]*$").unwrap());

/// `is_valid_metadata_key`.
pub fn is_valid_key(k: &str) -> bool {
    VALID_KEY.is_match(k)
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

/// `filter_metadata`.
pub fn filter(m: &Meta, user: Option<&str>, default: &str) -> Meta {
    let (d_add, d_exc) = filter_as_dict(Some(default));
    let (u_add, u_exc) = filter_as_dict(user);
    let none = || Keys::Some(Vec::new());
    let d_add = d_add.unwrap_or_else(none);
    let mut d_exc = d_exc.unwrap_or_else(none);
    let u_add = u_add.unwrap_or_else(none);
    let u_exc = u_exc.unwrap_or_else(none);
    if matches!(&d_add, Keys::Some(k) if !k.is_empty()) && d_exc == none() {
        d_exc = Keys::All;
    }
    let names = |k: &Keys| match k {
        Keys::All => Vec::new(),
        Keys::Some(v) => v.clone(),
    };
    if d_exc == Keys::All {
        if u_add == Keys::All {
            return subset(m, None, Some(&names(&u_exc)));
        }
        if u_exc == Keys::All {
            return subset(m, Some(&names(&u_add)), None);
        }
        let mut keep = names(&u_add);
        keep.extend(names(&d_add));
        return subset(m, Some(&keep), Some(&names(&u_exc)));
    }
    if u_add == Keys::All {
        return subset(m, None, Some(&names(&u_exc)));
    }
    if u_exc == Keys::All {
        return subset(m, Some(&names(&u_add)), None);
    }
    let mut m = m.clone();
    if get(&m, "tags").is_some_and(|t| !truthy(t)) {
        pop(&mut m, "tags");
    }
    let u_add = names(&u_add);
    let mut exclude = names(&u_exc);
    exclude.extend(names(&d_exc).into_iter().filter(|k| !u_add.contains(k)));
    subset(&m, None, Some(&exclude))
}

/// `second_level`: `{"a": ["b"]}` for `["a.b"]`.
fn second_level(keys: &[String]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for key in keys {
        if let Some((left, right)) = key.split_once('.') {
            match out.iter_mut().find(|(l, _)| l == left) {
                Some((_, v)) => v.push(right.to_string()),
                None => out.push((left.to_string(), vec![right.to_string()])),
            }
        }
    }
    out
}

/// `subset_metadata`.
fn subset(m: &Meta, keep_only: Option<&[String]>, exclude: Option<&[String]>) -> Meta {
    let supported: Vec<&(String, Value)> = m.iter().filter(|(k, _)| is_valid_key(k)).collect();
    let mut out: Meta = match keep_only {
        Some(keep) => {
            let mut out: Meta = supported
                .iter()
                .filter(|(k, _)| keep.contains(k))
                .map(|kv| (*kv).clone())
                .collect();
            for (key, sub) in second_level(keep) {
                if let Some((_, Value::Object(inner))) = supported.iter().find(|(k, _)| *k == key) {
                    let v = to_map(&subset(&from_map(inner), Some(&sub), None));
                    set(&mut out, &key, Value::Object(v));
                }
            }
            out
        }
        None => supported.into_iter().cloned().collect(),
    };
    if let Some(exclude) = exclude {
        out.retain(|(k, _)| !exclude.contains(k));
        for (key, sub) in second_level(exclude) {
            if let Some((_, Value::Object(inner))) = out.iter().find(|(k, _)| *k == key) {
                let v = to_map(&subset(&from_map(inner), None, Some(&sub)));
                set(&mut out, &key, Value::Object(v));
            }
        }
    }
    out
}

/// `restore_filtered_metadata`: the text's metadata, plus what the filter
/// kept out of the text.
pub fn restore(filtered: &Meta, unfiltered: &Meta, user: Option<&str>, default: &str) -> Meta {
    let shown = filter(unfiltered, user, default);
    let mut out = filtered.clone();
    for (k, v) in unfiltered {
        if get(&shown, k).is_none() && !JUPYTEXT_CELL_METADATA.contains(&k.as_str()) {
            set(&mut out, k, v.clone());
        }
    }
    out
}

/// `is_active`.
pub fn is_active(ext: &str, m: &Meta, default: bool) -> bool {
    let frozen = get(m, "run_control").and_then(|r| r.get("frozen"));
    if frozen == Some(&Value::Bool(true)) {
        return ext == ".ipynb";
    }
    let ext = ext.replace('.', "");
    if let Some(Value::Array(tags)) = get(m, "tags") {
        for tag in tags.iter().filter_map(|t| t.as_str()) {
            if tag.starts_with("active-") {
                return tag.split('-').any(|p| p == ext);
            }
        }
    }
    match get(m, "active") {
        None => default,
        Some(a) => a.as_str().unwrap_or("").split(['.', ',']).any(|p| p == ext),
    }
}

/// `metadata_to_double_percent_options`: the options of a `# %%` line.
pub fn to_double_percent_options(m: &mut Meta, json: bool) -> String {
    let mut text: Vec<String> = Vec::new();
    if let Some(t) = pop(m, "title") {
        text.push(py_str(&t));
    }
    if let Some(d) = pop(m, "cell_depth") {
        text.insert(0, "%".repeat(d.as_u64().unwrap_or(0) as usize));
    }
    if let Some(t) = pop(m, "cell_type") {
        let name = pop(m, "region_name").unwrap_or(t);
        text.push(format!("[{}]", py_str(&name)));
    }
    to_text(&text.join(" "), m, json)
}

/// Python's `str` of a value.
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => super::json::dumps(other),
    }
}

/// `metadata_to_text`: `key=json` pairs, or one JSON object, after a title.
fn to_text(title: &str, m: &Meta, json: bool) -> String {
    let mut text: Vec<String> = Vec::new();
    if !title.is_empty() {
        text.push(title.to_string());
    }
    if json {
        let shown: Meta = m
            .iter()
            .filter(|(k, _)| !JUPYTEXT_CELL_METADATA.contains(&k.as_str()))
            .cloned()
            .collect();
        if !shown.is_empty() {
            let pairs: Vec<String> = shown
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}: {}",
                        super::json::dumps(&Value::String(k.clone())),
                        super::json::dumps(v)
                    )
                })
                .collect();
            text.push(format!("{{{}}}", pairs.join(", ")));
        }
        return text.join(" ");
    }
    for (k, v) in m {
        if JUPYTEXT_CELL_METADATA.contains(&k.as_str()) {
            continue;
        }
        if k == "incorrectly_encoded_metadata" {
            text.push(py_str(v));
        } else if v.is_null() {
            text.push(k.clone());
        } else {
            text.push(format!("{k}={}", super::json::dumps(v)));
        }
    }
    text.join(" ")
}

/// `text_to_metadata(text, allow_title=True)`: the title and metadata of a
/// `# %%` line. Python literals, which jupytext also reads, are not ported:
/// such an option is kept whole, as jupytext keeps one it cannot read.
pub fn text_to_metadata(text: &str) -> Result<(String, Meta), String> {
    let text = text.trim();
    let curly = text.find('{');
    let equal = text.find('=');
    let key_value = match (curly, equal) {
        (None, _) => true,
        (Some(c), Some(e)) => e < c,
        _ => false,
    };
    if key_value {
        let mut words: Vec<&str> = match equal {
            Some(e) => {
                let mut w: Vec<&str> = text[..e].split(' ').collect();
                while w.last().is_some_and(|x| x.is_empty()) {
                    w.pop();
                }
                w.pop();
                w
            }
            None => text.split(' ').collect(),
        };
        while words
            .last()
            .is_some_and(|w| w.trim().is_empty() || w.starts_with('.'))
        {
            words.pop();
        }
        let title = words.join(" ");
        let m = parse_key_equal_value(&text[title.len()..]);
        return Ok((title, m));
    }
    let c = curly.unwrap_or(0);
    let m = match serde_json::from_str::<Value>(text[c..].trim()) {
        Ok(Value::Object(m)) => from_map(&m),
        Ok(_) => return Err(format!("cell options {text:?} are not a mapping")),
        Err(_) => incorrectly_encoded(&text[c..]),
    };
    Ok((text[..c].trim().to_string(), m))
}

/// `is_json_metadata`.
pub fn is_json_metadata(text: &str) -> bool {
    match (text.find('{'), text.find('=')) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(c), Some(e)) => c < e,
    }
}

fn incorrectly_encoded(text: &str) -> Meta {
    vec![(
        "incorrectly_encoded_metadata".to_string(),
        Value::String(text.to_string()),
    )]
}

/// `parse_key_equal_value`: `key1=value1 key2` from the right.
fn parse_key_equal_value(text: &str) -> Meta {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let last_space = text.rfind(' ');
    let last = last_space.map_or(text, |n| &text[n + 1..]);
    if !text.starts_with("--") && IDENTIFIER.is_match(last) {
        let mut out = vec![(last.to_string(), Value::Null)];
        if let Some(n) = last_space.filter(|n| *n > 0) {
            for (k, v) in parse_key_equal_value(&text[..n]) {
                set(&mut out, &k, v);
            }
        }
        return out;
    }
    let mut end = text.len();
    loop {
        let Some(equal) = text[..end].rfind('=') else {
            return incorrectly_encoded(text);
        };
        end = equal;
        let left = text[..equal].trim_end();
        let prev_space = left.rfind(' ');
        let key = text[prev_space.map_or(0, |n| n + 1)..equal].trim();
        if !is_valid_key(key) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(text[equal + 1..].trim()) else {
            continue;
        };
        let mut m = match prev_space {
            Some(n) if n > 0 => parse_key_equal_value(&text[..n]),
            _ => Vec::new(),
        };
        set(&mut m, key, value);
        return m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(s: &str) -> Meta {
        match serde_json::from_str(s).unwrap() {
            Value::Object(m) => from_map(&m),
            _ => unreachable!(),
        }
    }

    #[test]
    fn options_read_back_as_written() {
        let mut m = meta(r#"{"tags": ["a", "b"], "k": "v", "n": null, "cell_type": "markdown"}"#);
        m.insert(
            0,
            ("title".to_string(), Value::String("A title".to_string())),
        );
        let line = to_double_percent_options(&mut m, false);
        let (title, back) = text_to_metadata(&line).unwrap();
        assert!(line.starts_with("A title [markdown] "), "{line}");
        assert_eq!(title, "A title [markdown]");
        assert_eq!(to_map(&back), to_map(&m));
    }

    #[test]
    fn the_default_cell_filter_hides_jupyter_s_own_keys() {
        let m = meta(r#"{"collapsed": true, "tags": [], "x": 1, "bad key": 2}"#);
        assert_eq!(filter(&m, None, IGNORE_CELL_METADATA), meta(r#"{"x": 1}"#));
        let header = meta(r#"{"kernelspec": {}, "language_info": {}, "jupytext": {}}"#);
        let shown = filter(&header, None, DEFAULT_NOTEBOOK_METADATA);
        assert_eq!(
            to_map(&shown),
            to_map(&meta(r#"{"kernelspec": {}, "jupytext": {}}"#))
        );
    }
}
