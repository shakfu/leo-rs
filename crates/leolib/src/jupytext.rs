//! `@jupytext`: a Jupyter notebook in the outline as the `py:percent` text
//! jupytext makes of it.
//!
//! Leo converts with the jupytext package (`leoJupytext.py`) and reads the
//! text as `@clean` (`at.readOneAtJupytextNode`). This is a port of the parts
//! of jupytext 1.19.6 that path uses: the `py:percent` writer and reader, and
//! `combine_inputs_with_outputs`, which a write uses to keep each cell's
//! outputs, id and metadata. Leo's write drops them; see
//! `docs/dev/porting-notes.md`.
//!
//! A notebook whose text would not read back to the same cells is refused.

mod json;
mod metadata;
mod script;
mod yaml;

use std::collections::{BTreeSet, HashMap, HashSet};

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::outline::Outline;
use crate::position::Position;
use crate::util;
use metadata::{get, pop, set, Meta};
use script::{comment_lines, uncomment};

/// The jupytext version whose output this port matches, as the header says.
pub const JUPYTEXT_VERSION: &str = "1.19.6";

/// Format options jupytext reads from a notebook's metadata that this port
/// does not apply. A notebook setting one is refused.
const UNPORTED_OPTIONS: &[&str] = &[
    "comment_magics",
    "hide_notebook_metadata",
    "root_level_metadata_as_raw_cell",
    "split_at_heading",
    "rst2md",
    "use_runtools",
    "doxygen_equation_markers",
    "root_level_metadata_filter",
    "cell_markers",
    "custom_cell_magics",
    "custom_language_magics",
    "metadata_filter",
];

type R<T> = std::result::Result<T, String>;

/// A notebook as `py:percent` text.
pub struct Text {
    /// The text.
    pub text: String,
    /// Where the first cell starts: the header and the blank lines after it.
    pub header_end: usize,
    /// What a write of the unchanged text would change in the notebook.
    pub warning: Option<String>,
}

/// Convert a notebook's JSON to `py:percent` text, as Leo's
/// `jupytext.write(nb, f, fmt='py:percent')`.
pub fn notebook_to_text(raw: &str) -> R<Text> {
    let nb = parse_notebook(raw)?;
    let (text, header_end) = write_text(&nb)?;
    let (merged, map) = merge(&text, Some(&nb))?;
    if let Some(i) = (0..map.len().max(cells(&nb).len())).find(|&i| map.get(i) != Some(&Some(i))) {
        return Err(format!(
            "cell {} would not read back from its py:percent text as the same cell, \
             so a write would replace it",
            i + 1
        ));
    }
    let changed = cells(&merged)
        .iter()
        .zip(cells(&nb))
        .filter(|(a, b)| a != b)
        .count();
    let metadata_changed = merged["metadata"] != nb["metadata"];
    let what = match (changed, metadata_changed) {
        (0, false) => None,
        (0, true) => Some("the notebook metadata".to_string()),
        (n, false) => Some(format!("{n} cell(s)")),
        (n, true) => Some(format!("{n} cell(s) and the notebook metadata")),
    };
    let warning =
        what.map(|w| format!("writing it back changes {w}, as a jupytext round trip does"));
    Ok(Text {
        text,
        header_end,
        warning,
    })
}

/// Convert `py:percent` text to a notebook's JSON, keeping the outputs, ids
/// and metadata of `old`, the notebook on disk, as jupytext's `--update`.
/// An unchanged notebook comes back as `old`'s own bytes.
pub fn text_to_notebook(text: &str, old: Option<&str>) -> R<String> {
    let old_nb = old.map(parse_notebook).transpose()?;
    let (merged, _) = merge(text, old_nb.as_ref())?;
    if let (Some(raw), Some(nb)) = (old, &old_nb) {
        if &merged == nb {
            return Ok(raw.to_string());
        }
    }
    Ok(format!("{}\n", json::dumps_notebook(&split_lines(merged))))
}

// --- The notebook ----------------------------------------------------------

fn cells(nb: &Value) -> &[Value] {
    nb["cells"].as_array().map_or(&[], |a| a.as_slice())
}

fn cell_type(cell: &Value) -> &str {
    cell["cell_type"].as_str().unwrap_or("")
}

fn source(cell: &Value) -> &str {
    cell["source"].as_str().unwrap_or("")
}

fn cell_meta(cell: &Value) -> Meta {
    cell["metadata"]
        .as_object()
        .map(metadata::from_map)
        .unwrap_or_default()
}

/// `nbformat.reads`: the JSON, checked, with multi-line strings joined and
/// transient keys dropped.
fn parse_notebook(raw: &str) -> R<Value> {
    let mut nb: Value = serde_json::from_str(raw).map_err(|e| format!("not a notebook: {e}"))?;
    let Some(top) = nb.as_object_mut() else {
        return Err("not a notebook: the JSON is not an object".to_string());
    };
    match (top.get("nbformat"), top.get("nbformat_minor")) {
        (Some(Value::Number(n)), Some(Value::Number(_))) if n.as_u64() == Some(4) => {}
        (Some(n), _) => return Err(format!("nbformat {n} is not supported, only 4")),
        _ => return Err("not a notebook: no nbformat".to_string()),
    }
    let Some(Value::Object(md)) = top.get_mut("metadata") else {
        return Err("not a notebook: no metadata".to_string());
    };
    for key in ["orig_nbformat", "orig_nbformat_minor", "signature"] {
        md.remove(key);
    }
    let Some(Value::Array(cells)) = top.get_mut("cells") else {
        return Err("not a notebook: no cells".to_string());
    };
    for (i, cell) in cells.iter_mut().enumerate() {
        let bad = |what: &str| format!("cell {}: {what}", i + 1);
        let Some(c) = cell.as_object_mut() else {
            return Err(bad("not an object"));
        };
        if !matches!(
            c.get("cell_type").and_then(|t| t.as_str()),
            Some("code" | "markdown" | "raw")
        ) {
            return Err(bad("no cell_type, or not code, markdown or raw"));
        }
        match c.get_mut("metadata") {
            Some(Value::Object(m)) => {
                m.remove("trusted");
            }
            _ => return Err(bad("no metadata")),
        }
        match c.get_mut("source") {
            Some(s @ Value::Array(_)) => {
                *s = Value::String(join(s).ok_or_else(|| bad("bad source"))?)
            }
            Some(Value::String(_)) => {}
            _ => return Err(bad("no source")),
        }
        if let Some(Value::Object(atts)) = c.get_mut("attachments") {
            for att in atts.values_mut() {
                rejoin_bundle(att);
            }
        }
        if c["cell_type"] == "code" {
            if let Some(Value::Array(outputs)) = c.get_mut("outputs") {
                for out in outputs {
                    match out["output_type"].as_str() {
                        Some("execute_result" | "display_data") => {
                            if let Some(data) = out.get_mut("data") {
                                rejoin_bundle(data);
                            }
                        }
                        Some(t) if !t.is_empty() => {
                            if let Some(text) = out.get_mut("text").filter(|t| t.is_array()) {
                                if let Some(s) = join(text) {
                                    *text = Value::String(s);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(nb)
}

/// A list of strings joined, as `"".join`.
fn join(v: &Value) -> Option<String> {
    v.as_array()?
        .iter()
        .map(|s| s.as_str())
        .collect::<Option<Vec<&str>>>()
        .map(|parts| parts.concat())
}

fn is_json_mime(mime: &str) -> bool {
    mime == "application/json" || (mime.starts_with("application/") && mime.ends_with("+json"))
}

fn rejoin_bundle(data: &mut Value) {
    if let Some(m) = data.as_object_mut() {
        for (k, v) in m.iter_mut() {
            if !is_json_mime(k) {
                if let Some(s) = join(v) {
                    *v = Value::String(s);
                }
            }
        }
    }
}

/// nbformat's `split_lines` and `strip_transient`, before writing.
fn split_lines(mut nb: Value) -> Value {
    let split = |v: &mut Value| {
        if let Value::String(s) = v {
            let lines = py_splitlines_keepends(s)
                .into_iter()
                .map(Value::String)
                .collect();
            *v = Value::Array(lines);
        }
    };
    let split_bundle = |data: &mut Value| {
        if let Some(m) = data.as_object_mut() {
            for (k, v) in m.iter_mut() {
                if k.starts_with("text/") || k == "application/javascript" || k == "image/svg+xml" {
                    split(v);
                }
            }
        }
    };
    if let Some(md) = nb["metadata"].as_object_mut() {
        for key in ["orig_nbformat", "orig_nbformat_minor", "signature"] {
            md.remove(key);
        }
    }
    if let Some(cells) = nb["cells"].as_array_mut() {
        for cell in cells {
            if let Some(m) = cell["metadata"].as_object_mut() {
                m.remove("trusted");
            }
            if let Some(s) = cell.get_mut("source") {
                split(s);
            }
            if let Some(Value::Object(atts)) = cell.get_mut("attachments") {
                atts.values_mut().for_each(split_bundle);
            }
            if cell["cell_type"] != "code" {
                continue;
            }
            if let Some(Value::Array(outputs)) = cell.get_mut("outputs") {
                for out in outputs {
                    match out["output_type"].as_str() {
                        Some("execute_result" | "display_data") => {
                            if let Some(data) = out.get_mut("data") {
                                split_bundle(data);
                            }
                        }
                        Some("stream") => {
                            if let Some(text) = out.get_mut("text") {
                                split(text);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    nb
}

fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r'
            | '\u{b}'
            | '\u{c}'
            | '\u{1c}'
            | '\u{1d}'
            | '\u{1e}'
            | '\u{85}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Python's `str.splitlines(True)`.
fn py_splitlines_keepends(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if is_line_break(c) {
            let mut end = i + c.len_utf8();
            if c == '\r' && it.peek().map(|x| x.1) == Some('\n') {
                it.next();
                end += 1;
            }
            out.push(s[start..end].to_string());
            start = end;
        }
    }
    if start < s.len() {
        out.push(s[start..].to_string());
    }
    out
}

/// Python's `str.splitlines()`.
fn py_splitlines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if !is_line_break(c) {
            line.push(c);
            continue;
        }
        if c == '\r' && it.peek() == Some(&'\n') {
            it.next();
        }
        out.push(std::mem::take(&mut line));
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// `default_language_from_metadata_and_ext` for `.py`.
fn default_language(md: &mut Map<String, Value>, pop_main: bool) -> R<String> {
    let main = md
        .get("jupytext")
        .and_then(|j| j.get("main_language"))
        .and_then(|l| l.as_str())
        .map(str::to_string);
    let kernel = match md.get("kernelspec").and_then(|k| k.get("language")) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => return Err(format!("kernelspec language {other} is not a string")),
    };
    let default = kernel.unwrap_or_else(|| "python".to_string());
    if pop_main && main.as_deref() == Some(default.as_str()) {
        if let Some(Value::Object(j)) = md.get_mut("jupytext") {
            j.remove("main_language");
        }
    }
    let language = main.filter(|m| !m.is_empty()).unwrap_or(default);
    let language = match language.as_str() {
        "R" | "sas" => language,
        l if l.starts_with("C++") => "c++".to_string(),
        l => l.to_lowercase().replace('#', "sharp"),
    };
    if language == "go" || language == "csharp" {
        return Err(format!(
            "jupytext's handling of {language} notebooks is not ported"
        ));
    }
    Ok(language)
}

/// Refuse the jupytext format options this port does not apply.
fn check_options(md: &Map<String, Value>) -> R<()> {
    for key in [
        "nbrmd_formats",
        "nbrmd_format_version",
        "jupytext_formats",
        "jupytext_format_version",
    ] {
        if md.contains_key(key) {
            return Err(format!(
                "the legacy notebook metadata {key:?} is not ported"
            ));
        }
    }
    let Some(j) = md.get("jupytext") else {
        return Ok(());
    };
    let Some(j) = j.as_object() else {
        return Err("the notebook's jupytext metadata is not a mapping".to_string());
    };
    for (k, v) in j {
        if UNPORTED_OPTIONS.contains(&k.as_str()) {
            return Err(format!("the jupytext option {k:?} is not ported"));
        }
        let text_keys = [
            "notebook_metadata_filter",
            "cell_metadata_filter",
            "main_language",
            "encoding",
            "executable",
            "formats",
        ];
        if text_keys.contains(&k.as_str()) && !v.is_string() {
            return Err(format!("the jupytext option {k:?} is not a string"));
        }
        if k == "cell_metadata_json" && !v.is_boolean() {
            return Err(format!("the jupytext option {k:?} is not a boolean"));
        }
    }
    if let Some(name) = j
        .get("text_representation")
        .and_then(|t| t.get("format_name"))
    {
        if name != "percent" {
            return Err(format!(
                "the header names the jupytext format {name}, not percent"
            ));
        }
    }
    Ok(())
}

fn jupytext_str<'a>(md: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    md.get("jupytext")?.get(key)?.as_str()
}

fn jupytext_mut(md: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let j = md
        .entry("jupytext")
        .or_insert_with(|| Value::Object(Map::new()));
    if !j.is_object() {
        *j = Value::Object(Map::new());
    }
    j.as_object_mut().expect("just made an object")
}

/// `update_jupytext_formats_metadata`: the first `.py` format of a paired
/// notebook becomes `py:percent`.
fn update_formats(md: &mut Map<String, Value>) -> R<()> {
    let Some(formats) = jupytext_str(md, "formats").map(str::to_string) else {
        return Ok(());
    };
    static SIMPLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Za-z]+(:[A-Za-z]+)?$").unwrap());
    let entries: Vec<&str> = formats.split(',').map(str::trim).collect();
    if entries
        .iter()
        .any(|e| !SIMPLE.is_match(e) || e.starts_with("auto"))
    {
        return Err(format!("the jupytext formats {formats:?} are not ported"));
    }
    let first_py = entries
        .iter()
        .position(|e| e.split(':').next() == Some("py"));
    let updated: Vec<String> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| match Some(i) == first_py {
            true => "py:percent".to_string(),
            false => e.to_string(),
        })
        .collect();
    jupytext_mut(md).insert("formats".to_string(), Value::String(updated.join(",")));
    Ok(())
}

// --- Notebook to text -------------------------------------------------------

static HEADER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^---\s*$").unwrap());

/// `cell_source`: a cell's lines, with a last empty line if it ends in one.
fn cell_source(s: &str) -> Vec<String> {
    let mut lines = py_splitlines(s);
    if s.is_empty() || s.ends_with('\n') {
        lines.push(String::new());
    }
    lines
}

/// `TextNotebookConverter.writes` for `py:percent`, with the newline
/// `jupytext.write` adds. Also returns where the first cell starts.
fn write_text(nb: &Value) -> R<(String, usize)> {
    let mut md = nb["metadata"].as_object().cloned().unwrap_or_default();
    check_options(&md)?;
    update_formats(&mut md)?;
    let default_language = default_language(&mut md, true)?;
    let cell_filter = jupytext_str(&md, "cell_metadata_filter").map(str::to_string);
    let nb_filter = jupytext_str(&md, "notebook_metadata_filter").map(str::to_string);

    let json_options =
        md.get("jupytext").and_then(|j| j.get("cell_metadata_json")) == Some(&Value::Bool(true));

    // `encoding_and_executable`.
    let mut header: Vec<String> = Vec::new();
    let mut take = |key: &str| match md.get_mut("jupytext") {
        Some(Value::Object(j)) => j.remove(key).and_then(|v| v.as_str().map(str::to_string)),
        _ => None,
    };
    if let Some(e) = take("executable") {
        header.push(format!("#!{e}"));
    }
    if let Some(e) = take("encoding") {
        header.push(e);
    } else if default_language != "python" && cells(nb).iter().any(|c| !source(c).is_ascii()) {
        header.push("# -*- coding: utf-8 -*-".to_string());
    }

    // `metadata_and_cell_to_header`.
    let mut all = cells(nb);
    let mut yaml_lines: Vec<String> = Vec::new();
    let mut header_lines_to_next: Option<usize> = None;
    if let Some(first) = all.first().filter(|c| cell_type(c) == "raw") {
        let lines = py_splitlines(source(first).trim_matches(['\n', '\t', ' ']));
        if lines.len() >= 2
            && HEADER.is_match(&lines[0])
            && HEADER.is_match(&lines[lines.len() - 1])
        {
            yaml_lines = lines[1..lines.len() - 1].to_vec();
            header_lines_to_next = lines_to_next(&cell_meta(first))?;
            all = &all[1..];
        }
    }
    let representation = serde_json::json!({
        "extension": ".py",
        "format_name": "percent",
        "format_version": "1.3",
        "jupytext_version": JUPYTEXT_VERSION,
    });
    jupytext_mut(&mut md).insert("text_representation".to_string(), representation);
    let shown = metadata::filter(
        &metadata::from_map(&md),
        nb_filter.as_deref(),
        metadata::DEFAULT_NOTEBOOK_METADATA,
    );
    if !shown.is_empty() {
        let mut root = Map::new();
        root.insert(
            "jupyter".to_string(),
            Value::Object(metadata::to_map(&shown)),
        );
        yaml_lines.extend(yaml::dump(&root)?);
    }
    let header_content = match yaml_lines.is_empty() {
        true => Vec::new(),
        false => {
            let mut h = vec!["---".to_string()];
            h.extend(yaml_lines);
            h.push("---".to_string());
            comment_lines(&h, "#")
        }
    };

    // The cells, last first, for the blank lines between them.
    let mut texts = Vec::new();
    for cell in all {
        texts.push(cell_to_text(
            cell,
            &default_language,
            cell_filter.as_deref(),
            json_options,
        )?);
    }
    let mut blanks = vec![0; texts.len()];
    for i in (0..texts.len()).rev() {
        blanks[i] = match texts[i].1 {
            Some(n) => n,
            None => script::pep8_lines_between_cells(
                &texts[i].0,
                following(&texts[i + 1..], &blanks[i + 1..]),
            ),
        };
    }
    let n = match header_lines_to_next {
        Some(n) => n,
        None => script::pep8_lines_between_cells(&header_content, following(&texts, &blanks)),
    };
    let lines: Vec<String> = following(&texts, &blanks).map(str::to_string).collect();
    header.extend(header_content);
    header.extend(std::iter::repeat_n(String::new(), n));
    let header_end = match (header.is_empty(), lines.is_empty()) {
        (true, _) => 0,
        (false, false) => header.join("\n").len() + 1,
        (false, true) => usize::MAX,
    };
    header.extend(lines);
    let mut text = header.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok((text.clone(), header_end.min(text.len())))
}

/// The lines of `texts`, each followed by its blank lines.
fn following<'a>(
    texts: &'a [(Vec<String>, Option<usize>)],
    blanks: &'a [usize],
) -> impl Iterator<Item = &'a str> + Clone {
    texts.iter().zip(blanks).flat_map(|((t, _), n)| {
        t.iter()
            .map(String::as_str)
            .chain(std::iter::repeat_n("", *n))
    })
}

/// A cell's `lines_to_next_cell`, which sets the blank lines after it.
fn lines_to_next(m: &Meta) -> R<Option<usize>> {
    match get(m, "lines_to_next_cell") {
        None => Ok(None),
        Some(v) => match v.as_i64() {
            Some(n) => Ok(Some(n.max(0) as usize)),
            None => Err(format!("lines_to_next_cell {v} is not an integer")),
        },
    }
}

/// `cell_language`: a first-line `%%bash` names the cell's language.
fn cell_language(source: &mut Vec<String>) -> Option<(String, String)> {
    let magic = source.first()?.strip_prefix("%%")?;
    let (language, args) = magic.split_once(' ').unwrap_or((magic, ""));
    if !script::is_jupyter_language(language) {
        return None;
    }
    let found = (language.to_string(), args.to_string());
    source.remove(0);
    Some(found)
}

fn has_active_tag(m: &Meta) -> bool {
    get(m, "tags").and_then(|t| t.as_array()).is_some_and(|t| {
        t.iter()
            .any(|x| x.as_str().is_some_and(|s| s.starts_with("active-")))
    })
}

/// `DoublePercentCellExporter.cell_to_text`: the cell's lines, and the
/// blank lines it asks for after it.
fn cell_to_text(
    cell: &Value,
    default_language: &str,
    filter: Option<&str>,
    json_options: bool,
) -> R<(Vec<String>, Option<usize>)> {
    let kind = cell_type(cell);
    let mut source = cell_source(source(cell));
    let unfiltered = cell_meta(cell);
    let mut md = metadata::filter(&unfiltered, filter, metadata::IGNORE_CELL_METADATA);
    let magic = cell_language(&mut source);
    if let Some((language, args)) = &magic {
        if !args.is_empty() {
            set(&mut md, "magic_args", Value::String(args.clone()));
        }
        set(&mut md, "language", Value::String(language.clone()));
    }
    let language = match (&magic, get(&unfiltered, "language")) {
        (Some((l, _)), _) => l.clone(),
        (None, Some(Value::String(l))) => l.clone(),
        (None, Some(v)) => return Err(format!("cell language {v} is not a string")),
        (None, None) => default_language.to_string(),
    };
    let lines_to_next_cell = lines_to_next(&unfiltered)?;
    if kind == "raw" && get(&md, "active").is_none() && !has_active_tag(&md) {
        set(&mut md, "active", Value::String(String::new()));
    }
    let same = script::same_language(&language, default_language);
    let active = metadata::is_active(".py", &md, same);
    if kind == "raw" && get(&md, "active") == Some(&Value::String(String::new())) {
        pop(&mut md, "active");
    }
    let is_code =
        kind == "code" || (kind == "raw" && get(&md, "active").is_some()) || has_active_tag(&md);
    if !is_code {
        set(&mut md, "cell_type", Value::String(kind.to_string()));
    }
    let options = metadata::to_double_percent_options(&mut md, json_options);
    let mut indent = "";
    if let Some(first) = source
        .first()
        .filter(|f| is_code && active && !f.trim().is_empty())
    {
        indent = &first[..first.len() - first.trim_start().len()];
    }
    let marker = match options.is_empty() || options.starts_with('%') {
        true => format!("%%{options}"),
        false => format!("%% {options}"),
    };
    let mut lines = comment_lines(&[marker], &format!("{indent}#"));
    if is_code && active {
        script::comment_magic(&mut source, &language, true);
        if source != [""] {
            lines.extend(source);
        }
        return Ok((lines, lines_to_next_cell));
    }
    lines.extend(markdown_to_text(source, &unfiltered));
    Ok((lines, lines_to_next_cell))
}

/// `markdown_to_text` for `py:percent`: commented, or in the triple quotes
/// a `cell_marker` asks for.
fn markdown_to_text(mut source: Vec<String>, unfiltered: &Meta) -> Vec<String> {
    let markers = get(unfiltered, "cell_marker")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if !markers.is_empty() && !source.is_empty() {
        let (mut left, right) = match markers.split_once(',') {
            Some((l, r)) => (l.to_string(), r.to_string()),
            None => {
                let close = markers.strip_prefix(['r', 'R']).unwrap_or(markers);
                (format!("{markers}\n"), format!("\n{close}"))
            }
        };
        let head = |s: &str, a: usize, b: usize| s.chars().skip(a).take(b - a).collect::<String>();
        let tail: String = {
            let c: Vec<char> = right.chars().collect();
            c[c.len().saturating_sub(3)..].iter().collect()
        };
        let raw_prefix = left.starts_with(['r', 'R']);
        if (head(&left, 0, 3) == tail || (raw_prefix && head(&left, 1, 4) == tail))
            && (tail == "\"\"\"" || tail == "'''")
        {
            if !raw_prefix && source.join("\n").contains('\\') {
                left = format!("r{left}");
            }
            source[0] = format!("{left}{}", source[0]);
            let last = source.len() - 1;
            source[last].push_str(&right);
            return source;
        }
    }
    comment_lines(&source, "#")
}

// --- Text to notebook -------------------------------------------------------

/// A cell read from the text.
#[derive(Debug, Clone)]
struct Cell {
    kind: String,
    source: String,
    md: Meta,
}

static START: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*#\s*%%(%*)\s(.*)$").unwrap());
static ALT_START: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*#\s*(%%|<codecell>|In\[[0-9 ]*\]:?)\s*$").unwrap());
static ENCODING: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[ \t\f]*#.*?coding[:=][ \t]*([-_.a-zA-Z0-9]+)").unwrap());
static JUPYTER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^jupyter\s*:\s*$").unwrap());
static FENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})(.*)$").unwrap());

fn is_start(line: &str) -> bool {
    START.is_match(line) || ALT_START.is_match(line)
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// What a text's header gives.
struct Header {
    /// The notebook metadata.
    metadata: Map<String, Value>,
    /// Whether the header had a `jupyter` section.
    jupyter: bool,
    /// A raw cell for the header's other lines.
    cell: Option<Cell>,
    /// The line the cells start at.
    end: usize,
}

/// `header_to_metadata_and_cell`.
fn read_header(lines: &[String]) -> R<Header> {
    let mut header: Vec<String> = Vec::new();
    let mut jupyter: Vec<String> = Vec::new();
    let (mut in_jupyter, mut started, mut ended) = (false, false, false);
    let mut md = Map::new();
    let mut start = 0;
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        if i == 0 && line.starts_with("#!") {
            jupytext_mut(&mut md).insert(
                "executable".to_string(),
                Value::String(line[2..].to_string()),
            );
            start = i + 1;
            i += 1;
            continue;
        }
        if i == 0 || (i == 1 && !ENCODING.is_match(&lines[0])) {
            if let Some(m) = ENCODING.captures(line) {
                if &m[1] != "utf-8" {
                    return Err("encodings other than utf-8 are not supported".to_string());
                }
                jupytext_mut(&mut md).insert("encoding".to_string(), Value::String(line.clone()));
                start = i + 1;
                i += 1;
                continue;
            }
        }
        if !line.starts_with('#') {
            break;
        }
        let line = uncomment(std::slice::from_ref(line), "#").remove(0);
        if HEADER.is_match(&line) {
            if !started {
                started = true;
                i += 1;
                continue;
            }
            ended = true;
            break;
        }
        if !started && !blank(&line) {
            break;
        }
        if JUPYTER.is_match(&line) {
            in_jupyter = true;
        } else if !line.is_empty() && !line.starts_with(char::is_whitespace) {
            in_jupyter = false;
        }
        match in_jupyter {
            true => jupyter.push(line),
            false => header.push(line),
        }
        i += 1;
    }
    if !ended {
        return Ok(Header {
            metadata: md,
            jupyter: false,
            cell: None,
            end: start,
        });
    }
    if !jupyter.is_empty() {
        let extra = md;
        md = match yaml::load(&jupyter)?.get("jupyter") {
            Some(Value::Object(m)) => m.clone(),
            _ => return Err("the header's jupyter section is not a mapping".to_string()),
        };
        for (k, v) in extra {
            if let Value::Object(inner) = v {
                jupytext_mut(&mut md).extend(inner);
            } else {
                md.insert(k, v);
            }
        }
    }
    let mut lines_to_next_cell = 1;
    if lines.len() > i + 1 {
        if !blank(&uncomment(&lines[i + 1..i + 2], "#")[0]) {
            lines_to_next_cell = 0;
        } else {
            i += 1;
        }
    } else {
        lines_to_next_cell = 0;
    }
    let cell = match header.is_empty() {
        true => None,
        false => {
            let mut all = vec!["---".to_string()];
            all.extend(header);
            all.push("---".to_string());
            let next = lines[i + 1..].iter().map(String::as_str);
            let expected = script::pep8_lines_between_cells(&["---".to_string()], next);
            let mut cell_md = Vec::new();
            if lines_to_next_cell != expected {
                cell_md.push((
                    "lines_to_next_cell".to_string(),
                    Value::from(lines_to_next_cell),
                ));
            }
            Some(Cell {
                kind: "raw".to_string(),
                source: all.join("\n"),
                md: cell_md,
            })
        }
    };
    Ok(Header {
        metadata: md,
        jupyter: !jupyter.is_empty(),
        cell,
        end: i + 1,
    })
}

/// `LightScriptCellReader.options_to_metadata`: a `# %%` line's options.
fn options_to_metadata(options: &str) -> R<Meta> {
    let (mut title, mut md) = metadata::text_to_metadata(options)?;
    for kind in ["markdown", "raw", "md"] {
        let code = format!("[{kind}]");
        if title.contains(&code) {
            title = title.replace(&code, "").trim().to_string();
            set(&mut md, "cell_type", Value::String(kind.to_string()));
            if kind == "md" {
                set(&mut md, "region_name", Value::String("md".to_string()));
                set(&mut md, "cell_type", Value::String("markdown".to_string()));
            }
            break;
        }
    }
    let depth = title.len() - title.trim_start_matches('%').len();
    if depth > 0 {
        set(&mut md, "cell_depth", Value::from(depth));
        title = title[depth..].trim().to_string();
    }
    if !title.is_empty() {
        set(&mut md, "title", Value::String(title));
    }
    Ok(md)
}

fn opening_fence(text: &str) -> Option<(char, usize)> {
    let m = FENCE.captures(text)?;
    let fence = m.get(1)?.as_str();
    let c = fence.chars().next()?;
    if c == '`' && m[2].contains('`') {
        return None;
    }
    Some((c, fence.len()))
}

fn closes_fence(text: &str, fence: (char, usize)) -> bool {
    FENCE.captures(text).is_some_and(|m| {
        m[1].starts_with(fence.0) && m[1].len() >= fence.1 && m[2].trim().is_empty()
    })
}

/// For each line of a text, the longest closing fence of each kind there
/// or after, with the lines uncommented and not. jupytext searches the rest
/// of the text at each opening fence, which takes quadratic time.
struct Fences {
    after: [[Vec<usize>; 2]; 2],
}

impl Fences {
    fn new(lines: &[String]) -> Fences {
        let n = lines.len();
        let mut after = [
            [vec![0; n + 1], vec![0; n + 1]],
            [vec![0; n + 1], vec![0; n + 1]],
        ];
        for (mode, comment) in ["#", ""].into_iter().enumerate() {
            for i in (0..n).rev() {
                let text = uncomment(&lines[i..=i], comment).remove(0);
                for (kind, c) in ['`', '~'].into_iter().enumerate() {
                    let here = FENCE
                        .captures(&text)
                        .filter(|m| m[1].starts_with(c) && m[2].trim().is_empty())
                        .map_or(0, |m| m[1].len());
                    after[mode][kind][i] = after[mode][kind][i + 1].max(here);
                }
            }
        }
        Fences { after }
    }

    /// Whether a line at or after `from` closes `open`.
    fn closed(&self, comment: &str, open: (char, usize), from: usize) -> bool {
        let mode = usize::from(comment.is_empty());
        let kind = usize::from(open.0 == '~');
        self.after[mode][kind][from] >= open.1
    }
}

fn last_two_lines_blank(source: &[String]) -> bool {
    let n = source.len();
    n >= 3 && !blank(&source[n - 3]) && blank(&source[n - 2]) && blank(&source[n - 1])
}

/// `DoublePercentScriptCellReader.read`: the first cell of `lines`, where
/// the next starts, and whether its options were JSON.
fn read_cell(
    lines: &[String],
    default_language: &str,
    fences: &Fences,
    base: usize,
) -> R<(Cell, usize, bool)> {
    let mut json_options = false;
    let mut md = match START.is_match(&lines[0]) {
        true => {
            let line = uncomment(&lines[..1], "#").remove(0);
            let options = &line[line.find("%%").unwrap_or(0) + 2..];
            json_options = metadata::is_json_metadata(options);
            options_to_metadata(options)?
        }
        false => Vec::new(),
    };
    let language = match pop(&mut md, "language") {
        None => None,
        Some(Value::String(l)) => Some(l),
        Some(v) => return Err(format!("cell language {v} is not a string")),
    };
    let lang = language
        .clone()
        .unwrap_or_else(|| default_language.to_string());

    // `find_cell_end`.
    let mut comment = "#";
    let kind = match pop(&mut md, "cell_type") {
        Some(Value::String(k)) => k,
        Some(v) => return Err(format!("cell_type {v} is not a string")),
        None if !metadata::is_active(".ipynb", &md, true) => {
            if get(&md, "active") == Some(&Value::String(String::new())) {
                pop(&mut md, "active");
            }
            if metadata::is_active(".py", &md, true) {
                comment = "";
            }
            "raw".to_string()
        }
        None => "code".to_string(),
    };
    let mut next_cell = lines.len();
    let mut parser = script::StringParser::new(Some(&lang));
    let mut fence: Option<(char, usize)> = None;
    for (i, line) in lines.iter().enumerate() {
        if parser.is_quoted() {
            parser.read_line(line);
            continue;
        }
        parser.read_line(line);
        if kind == "markdown" || kind == "raw" {
            let text = uncomment(std::slice::from_ref(line), comment).remove(0);
            if let Some(f) = fence {
                if closes_fence(&text, f) {
                    fence = None;
                }
                continue;
            }
            if let Some(open) = opening_fence(&text) {
                if fences.closed(comment, open, base + i + 1) {
                    fence = Some(open);
                    continue;
                }
            }
        }
        if i > 0 && is_start(line) {
            next_cell = i;
            break;
        }
    }
    let cell_end = if last_two_lines_blank(&lines[..next_cell]) {
        next_cell - 2
    } else if next_cell > 0 && lines[next_cell - 1].is_empty() {
        next_cell - 1
    } else {
        next_cell
    };

    // `find_cell_content`.
    let cell_start = usize::from(is_start(&lines[0]));
    let org_content = &lines[cell_start.min(cell_end)..cell_end];
    let content = extract_content(
        org_content,
        &kind,
        &mut md,
        comment,
        language.as_deref(),
        default_language,
    );
    let lines_to_next_cell = match (cell_end < lines.len(), next_cell >= lines.len()) {
        (true, true) => next_cell - cell_end + 1,
        (true, false) => next_cell - cell_end,
        (false, _) => 1,
    };
    let org: Vec<String> = match org_content.is_empty() {
        true => vec![String::new()],
        false => org_content.to_vec(),
    };
    let next = lines[next_cell..].iter().map(String::as_str);
    if lines_to_next_cell != script::pep8_lines_between_cells(&org, next) {
        set(
            &mut md,
            "lines_to_next_cell",
            Value::from(lines_to_next_cell),
        );
    }
    if let Some(l) = language {
        set(&mut md, "language", Value::String(l));
    }
    let cell = Cell {
        kind,
        source: content.join("\n"),
        md,
    };
    Ok((cell, next_cell.max(1), json_options))
}

/// `BaseCellReader.extract_content` and `uncomment_code_and_magics`.
fn extract_content(
    lines: &[String],
    kind: &str,
    md: &mut Meta,
    comment: &str,
    language: Option<&str>,
    default_language: &str,
) -> Vec<String> {
    if !metadata::is_active(".py", md, kind == "code") {
        if let Some(lines) = triple_quoted(lines, md) {
            return lines;
        }
    }
    let other_language =
        get(md, "active").is_none() && language.is_some_and(|l| l != default_language);
    if !metadata::is_active(".py", md, true) || other_language {
        return uncomment(lines, comment);
    }
    let lang = language.unwrap_or(default_language);
    let mut lines = lines.to_vec();
    // `is_active` is true here, so magics are uncommented.
    script::uncomment_magic(&mut lines, lang, true);
    if kind == "code" {
        script::unescape_code_start(&mut lines, lang);
        return lines;
    }
    uncomment(&lines, comment)
}

/// A markdown cell written as a triple-quoted string, with its marker.
fn triple_quoted(lines: &[String], md: &mut Meta) -> Option<Vec<String>> {
    let joined = lines.join("\n");
    let content = joined.trim();
    for prefix in ["", "r", "R"] {
        for quote in ["\"\"\"", "'''"] {
            let mut left = format!("{prefix}{quote}");
            let mut right = quote.to_string();
            if !(content.starts_with(&left)
                && content.ends_with(&right)
                && content.len() >= left.len() + right.len())
            {
                continue;
            }
            let mut inner = &content[left.len()..content.len() - right.len()];
            if let Some(rest) = inner.strip_prefix('\n') {
                inner = rest;
                left.push('\n');
            }
            if let Some(rest) = inner.strip_suffix('\n') {
                inner = rest;
                right = format!("\n{right}");
            }
            let marker = if prefix.is_empty() {
                (left.len() == 4 && right.len() == 4).then(|| left[..3].to_string())
            } else if left.len() - 1 == 4 && right.len() == 4 {
                Some(left[..4].to_string())
            } else {
                Some(format!("{left},{right}"))
            };
            if let Some(m) = marker {
                set(md, "cell_marker", Value::String(m));
            }
            return Some(py_splitlines(inner));
        }
    }
    None
}

/// `TextNotebookConverter.reads` for `py:percent`: the notebook metadata
/// and the cells.
fn read_text(text: &str) -> R<(Map<String, Value>, Vec<Cell>)> {
    let all = py_splitlines(text);
    let Header {
        metadata: mut md,
        jupyter: jupyter_md,
        cell: header_cell,
        end: pos,
    } = read_header(&all)?;
    check_options(&md)?;
    let default_language = default_language(&mut md.clone(), false)?;
    let mut cells: Vec<Cell> = header_cell.into_iter().collect();
    let fences = Fences::new(&all);
    let mut at = pos.min(all.len());
    let mut json_options = false;
    while at < all.len() {
        let (cell, n, json) = read_cell(&all[at..], &default_language, &fences, at)?;
        cells.push(cell);
        json_options |= json;
        at += n;
    }

    // `set_main_and_cell_language`.
    let main = default_language;
    let kernel_language = md
        .get("kernelspec")
        .and_then(|k| k.get("language"))
        .is_some();
    if !kernel_language && !cells.is_empty() {
        jupytext_mut(&mut md).insert("main_language".to_string(), Value::String(main.clone()));
    }
    for cell in &mut cells {
        let Some(Value::String(language)) = get(&cell.md, "language").cloned() else {
            continue;
        };
        if language == main {
            pop(&mut cell.md, "language");
            continue;
        }
        if script::usual_language_name(&language) == main || !script::is_jupyter_language(&language)
        {
            continue;
        }
        pop(&mut cell.md, "language");
        cell.source = match pop(&mut cell.md, "magic_args") {
            Some(args) => format!("%%{language} {}\n{}", py_str(&args), cell.source),
            None => format!("%%{language}\n{}", cell.source),
        };
    }

    // `update_metadata_filters`.
    let keys: BTreeSet<String> = cells
        .iter()
        .flat_map(|c| c.md.iter().map(|(k, _)| k.clone()))
        .collect();
    update_metadata_filters(&mut md, jupyter_md, &keys);
    if json_options {
        jupytext_mut(&mut md)
            .entry("cell_metadata_json")
            .or_insert(Value::Bool(true));
    }
    let representation = jupytext_mut(&mut md)
        .entry("text_representation")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(r) = representation.as_object_mut() {
        r.insert("extension".to_string(), Value::String(".py".to_string()));
        r.insert(
            "format_name".to_string(),
            Value::String("percent".to_string()),
        );
    }
    Ok((md, cells))
}

fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => json::dumps(other),
    }
}

/// `update_metadata_filters`. jupytext lists a set's keys in hash order;
/// they are sorted here.
fn update_metadata_filters(md: &mut Map<String, Value>, jupyter_md: bool, keys: &BTreeSet<String>) {
    let keys: Vec<String> = keys.iter().cloned().collect();
    if !jupyter_md {
        let j = jupytext_mut(md);
        j.insert(
            "notebook_metadata_filter".to_string(),
            Value::String("-all".to_string()),
        );
        let filter = metadata::filter_as_string(&metadata::Keys::Some(keys), &metadata::Keys::All);
        j.entry("cell_metadata_filter")
            .or_insert(Value::String(filter));
        return;
    }
    if let Some(current) = jupytext_str(md, "cell_metadata_filter").map(str::to_string) {
        let (add, exc) = metadata::filter_as_dict(Some(&current));
        let exc = match exc {
            Some(metadata::Keys::Some(e)) => {
                metadata::Keys::Some(e.into_iter().filter(|k| !keys.contains(k)).collect())
            }
            Some(all) => all,
            None => metadata::Keys::Some(Vec::new()),
        };
        let add = match add.unwrap_or(metadata::Keys::Some(Vec::new())) {
            metadata::Keys::Some(mut a) => {
                for k in &keys {
                    if !a.contains(k) {
                        a.push(k.clone());
                    }
                }
                metadata::Keys::Some(a)
            }
            all => all,
        };
        let s = metadata::filter_as_string(&add, &exc);
        jupytext_mut(md).insert("cell_metadata_filter".to_string(), Value::String(s));
        return;
    }
    let current = jupytext_str(md, "notebook_metadata_filter")
        .unwrap_or("")
        .to_string();
    let mut filter: Vec<String> = current
        .split(',')
        .filter(|k| !k.is_empty())
        .map(str::to_string)
        .collect();
    if filter.iter().any(|k| k == "all" || k == "-all") {
        return;
    }
    let defaults: Vec<&str> = metadata::DEFAULT_NOTEBOOK_METADATA.split(',').collect();
    for key in md.keys() {
        if defaults.contains(&key.as_str())
            || filter.contains(key)
            || filter.contains(&format!("-{key}"))
        {
            continue;
        }
        filter.push(key.clone());
    }
    if !filter.is_empty() {
        jupytext_mut(md).insert(
            "notebook_metadata_filter".to_string(),
            Value::String(filter.join(",")),
        );
    }
}

// --- Combining with the notebook on disk ------------------------------------

/// `black_invariant`: the text without what reformatting may change.
fn black_invariant(s: &str) -> String {
    s.chars()
        .filter(|c| !" \t\n,'\"()\\".contains(*c))
        .collect()
}

/// `map_outputs_to_inputs`: for each text cell, the notebook cell whose
/// outputs it keeps.
fn map_outputs(inputs: &[Cell], outputs: &[Value]) -> Vec<Option<usize>> {
    let ins: Vec<String> = inputs.iter().map(|c| black_invariant(&c.source)).collect();
    let outs: Vec<String> = outputs.iter().map(|c| black_invariant(source(c))).collect();
    let same = |i: usize, j: usize, ends_with: bool| {
        let (r, t) = (&outs[j], &ins[i]);
        match ends_with && !t.is_empty() {
            true => r.ends_with(t.as_str()),
            false => r == t,
        }
    };
    let mut map: Vec<Option<usize>> = vec![None; inputs.len()];
    // First, by type and content, in order within each type.
    let mut first: HashMap<&str, usize> = HashMap::new();
    for (i, cell) in inputs.iter().enumerate() {
        let from = first.get(cell.kind.as_str()).copied().unwrap_or(0);
        let found = (from..outputs.len())
            .find(|&j| cell_type(&outputs[j]) == cell.kind && same(i, j, false));
        if let Some(j) = found {
            map[i] = Some(j);
            first.insert(cell.kind.as_str(), j + 1);
        }
    }
    // Then by content in any order, then a cell that ends an old one.
    let mut unused: BTreeSet<usize> = (0..outputs.len())
        .filter(|j| !map.contains(&Some(*j)))
        .collect();
    for ends_with in [false, true] {
        if unused.is_empty() {
            return map;
        }
        for (i, cell) in inputs.iter().enumerate() {
            if map[i].is_some() {
                continue;
            }
            let found = unused
                .iter()
                .copied()
                .find(|&j| cell_type(&outputs[j]) == cell.kind && same(i, j, ends_with));
            if let Some(j) = found {
                map[i] = Some(j);
                unused.remove(&j);
            }
        }
    }
    // Last, a changed cell keeps the outputs of the old cell in its place.
    if unused.is_empty() {
        return map;
    }
    let mut prev: isize = -1;
    for (i, cell) in inputs.iter().enumerate() {
        if let Some(j) = map[i] {
            prev = j as isize;
            continue;
        }
        let j = (prev + 1) as usize;
        if !unused.contains(&j) {
            continue;
        }
        if cell_type(&outputs[j]) == cell.kind && !cell.source.trim().is_empty() {
            map[i] = Some(j);
            unused.remove(&j);
            prev = j as isize;
        }
    }
    map
}

/// A fresh cell id, as nbformat's `random_cell_id`: eight hex digits.
fn new_cell_id(taken: &HashSet<String>) -> String {
    use std::hash::{BuildHasher, Hasher};
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    loop {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        h.write_u128(nanos);
        let id = format!("{:08x}", h.finish() as u32);
        if !taken.contains(&id) {
            return id;
        }
    }
}

/// `combine_inputs_with_outputs`, then `drop_text_representation_metadata`:
/// the notebook a write produces, and which old cell each new one kept.
fn merge(text: &str, old: Option<&Value>) -> R<(Value, Vec<Option<usize>>)> {
    let (src_md, src_cells) = read_text(text)?;
    let empty = Value::Object(Map::new());
    let old_md = old
        .map_or(&empty, |nb| &nb["metadata"])
        .as_object()
        .cloned()
        .unwrap_or_default();
    let old_cells = old.map_or(&[][..], cells);
    let minor = old
        .map_or(Some(5), |nb| nb["nbformat_minor"].as_u64())
        .unwrap_or(5);

    let nb_filter = jupytext_str(&src_md, "notebook_metadata_filter").map(str::to_string);
    let mut nb_md = match (old, nb_filter.as_deref()) {
        (Some(_), Some("-all")) => old_md.clone(),
        (Some(_), filter) => metadata::to_map(&metadata::restore(
            &metadata::from_map(&src_md),
            &metadata::from_map(&old_md),
            filter,
            metadata::DEFAULT_NOTEBOOK_METADATA,
        )),
        (None, _) => src_md.clone(),
    };
    if let Some(Value::Object(j)) = nb_md.get_mut("jupytext") {
        j.remove("text_representation");
    }
    if nb_md
        .get("jupytext")
        .is_some_and(|j| j.as_object().is_some_and(|m| m.is_empty()))
    {
        nb_md.remove("jupytext");
    }
    let cell_filter = jupytext_str(&nb_md, "cell_metadata_filter").map(str::to_string);

    let map = match old {
        Some(_) => map_outputs(&src_cells, old_cells),
        None => vec![None; src_cells.len()],
    };
    let mut taken: HashSet<String> = old_cells
        .iter()
        .filter_map(|c| c["id"].as_str().map(str::to_string))
        .collect();
    let mut out = Vec::new();
    for (cell, j) in src_cells.iter().zip(&map) {
        let value = match j {
            Some(j) => {
                let mut c = old_cells[*j].clone();
                let md = metadata::restore(
                    &cell.md,
                    &cell_meta(&c),
                    cell_filter.as_deref(),
                    metadata::IGNORE_CELL_METADATA,
                );
                c["source"] = Value::String(cell.source.clone());
                c["metadata"] = Value::Object(metadata::to_map(&md));
                c
            }
            None => {
                let mut c = Map::new();
                let kind = match cell.kind.as_str() {
                    "code" | "markdown" => cell.kind.as_str(),
                    _ => "raw",
                };
                c.insert("cell_type".to_string(), Value::String(kind.to_string()));
                if minor >= 5 {
                    let id = new_cell_id(&taken);
                    taken.insert(id.clone());
                    c.insert("id".to_string(), Value::String(id));
                }
                c.insert(
                    "metadata".to_string(),
                    Value::Object(metadata::to_map(&cell.md)),
                );
                c.insert("source".to_string(), Value::String(cell.source.clone()));
                if kind == "code" {
                    c.insert("execution_count".to_string(), Value::Null);
                    c.insert("outputs".to_string(), Value::Array(Vec::new()));
                }
                Value::Object(c)
            }
        };
        out.push(value);
    }
    let nb = serde_json::json!({
        "cells": out,
        "metadata": nb_md,
        "nbformat": 4,
        "nbformat_minor": minor,
    });
    Ok((nb, map))
}

// --- The outline ------------------------------------------------------------

/// Read the `@jupytext` node at `root` from its notebook, as Leo's
/// `at.readOneAtJupytextNode`: `@clean` over the notebook's text.
pub fn read_one_at_jupytext_node(o: &mut Outline, root: &Position) -> Result<bool> {
    let path = o.full_path(root);
    if !std::path::Path::new(&path).exists() {
        return Err(Error::NotFound { path });
    }
    let raw = crate::external::read_file_to_string(&path)?;
    let import_error = |detail: String| Error::Import {
        path: util::short_file_name(&path),
        detail,
    };
    let converted = notebook_to_text(&raw).map_err(import_error)?;
    let read = match crate::atclean::update_from_text(o, root, &path, &converted.text)? {
        Some(read) => read,
        None => {
            create_outline(o, root, &converted);
            let written = crate::atfile_write::at_file_to_string(o, root, false);
            if written.as_deref().ok() != Some(converted.text.as_str()) {
                return Err(import_error(
                    "the outline made of its text would not write the text back".to_string(),
                ));
            }
            true
        }
    };
    if let Some(warning) = converted.warning {
        o.add_import_warning(root, warning);
    }
    o.remember_read_path(root, &path);
    Ok(read)
}

/// `jtm.create_outline`: a `<< prefix >>` node for the header, then a node
/// per cell. Leo finds `# %%` anywhere; here only at the start of a line.
fn create_outline(o: &mut Outline, root: &Position, t: &Text) {
    if root.has_children(o) {
        o.set_body(root, &t.text);
        return;
    }
    let mut body = String::new();
    if t.header_end > 0 {
        let p = o.insert_as_last_child(root);
        o.set_headline(&p, "<< prefix >>");
        o.set_body(&p, &t.text[..t.header_end]);
        body.push_str("<< prefix >>\n");
    }
    let mut cells: Vec<String> = Vec::new();
    for line in util::split_lines(&t.text[t.header_end..]) {
        match (line.trim_start().starts_with("# %%"), cells.last_mut()) {
            (false, Some(last)) => last.push_str(&line),
            _ => cells.push(line),
        }
    }
    for cell in cells {
        let p = o.insert_as_last_child(root);
        let n = p.child_index;
        o.set_headline(&p, &headline(&cell, n));
        o.set_body(&p, &cell);
    }
    body.push_str("@others\n@language jupytext\n@tabwidth -4\n");
    o.set_body(root, &body);
}

/// `jtm.compute_headline`.
fn headline(cell: &str, child_index: usize) -> String {
    let lines = py_splitlines_keepends(cell);
    if lines.len() > 1 && lines[0].contains("[markdown]") {
        let line1 = lines[1].trim();
        if let Some(section) = line1.strip_prefix("# ").map(str::trim) {
            if section.starts_with('#') {
                return shorten(section);
            }
        }
        return shorten(line1);
    }
    let Some(line1) = lines[1.min(lines.len())..]
        .iter()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
    else {
        return format!("Cell {child_index}");
    };
    if let Some(comment) = line1
        .strip_prefix('#')
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        return shorten(comment);
    }
    if let Some(i) = line1.find('#') {
        let comment = line1[i + 1..].trim();
        if !comment.is_empty() {
            return shorten(comment);
        }
    }
    shorten(line1)
}

/// `textwrap.shorten(s, width=60, placeholder='')`.
fn shorten(s: &str) -> String {
    const WIDTH: usize = 60;
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= WIDTH {
        return collapsed;
    }
    let mut chunks: Vec<String> = Vec::new();
    for (i, word) in collapsed.split(' ').enumerate() {
        if i > 0 {
            chunks.push(" ".to_string());
        }
        chunks.extend(hyphen_chunks(word));
    }
    chunks.reverse();
    let mut line: Vec<String> = Vec::new();
    let mut len = 0;
    while let Some(chunk) = chunks.last() {
        let n = chunk.chars().count();
        if len + n > WIDTH {
            break;
        }
        len += n;
        line.push(chunks.pop().unwrap_or_default());
    }
    if let Some(chunk) = chunks.last_mut() {
        if chunk.chars().count() > WIDTH {
            let room = WIDTH - len;
            let chars: Vec<char> = chunk.chars().collect();
            let mut end = room;
            if let Some(h) = chars[..room].iter().rposition(|c| *c == '-') {
                if h > 0 && chars[..h].iter().any(|c| *c != '-') {
                    end = h + 1;
                }
            }
            line.push(chars[..end].iter().collect());
        }
    }
    line.concat().trim_end().to_string()
}

/// textwrap's chunks of a word: split after a hyphen between letters, and
/// around a dash of two or more hyphens.
fn hyphen_chunks(word: &str) -> Vec<String> {
    let c: Vec<char> = word.chars().collect();
    let letter = |i: usize| c.get(i).is_some_and(|ch| ch.is_alphabetic() || *ch == '_');
    let word_char = |i: usize| {
        c.get(i)
            .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
    };
    let punct = |i: usize| word_char(i) || c.get(i).is_some_and(|ch| "!\"'&.,?".contains(*ch));
    let mut out = Vec::new();
    let mut start = 0;
    let mut k = 1;
    while k < c.len() {
        if c[k] != '-' {
            k += 1;
            continue;
        }
        let run = c[k..].iter().take_while(|ch| **ch == '-').count();
        if run >= 2 && k > start && punct(k - 1) && word_char(k + run) {
            out.push(c[start..k].iter().collect());
            out.push(c[k..k + run].iter().collect());
            start = k + run;
            k = start + 1;
            continue;
        }
        let before = (k >= 2 && letter(k - 2) && letter(k - 1))
            || (k >= 3 && letter(k - 3) && c[k - 2] == '-' && letter(k - 1));
        let after =
            letter(k + 1) && (letter(k + 2) || (c.get(k + 2) == Some(&'-') && letter(k + 3)));
        if before && after && k > start {
            out.push(c[start..=k].iter().collect());
            start = k + 1;
        }
        k += 1;
    }
    if start < c.len() {
        out.push(c[start..].iter().collect());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[&str] = &["python", "r", "header"];

    /// A file from `tests/jupytext`, made by `scripts/make_jupytext_fixtures.py`.
    fn fixture(name: &str) -> String {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/jupytext");
        std::fs::read_to_string(dir.join(name)).unwrap()
    }

    /// `json` with the ids of the cells `old` lacks spelled NEW.
    fn mark_new_ids(json: &str, old: Option<&str>) -> String {
        let old_ids: HashSet<String> = old.map_or_else(HashSet::new, |o| {
            let nb = parse_notebook(o).unwrap();
            cells(&nb)
                .iter()
                .filter_map(|c| c["id"].as_str().map(str::to_string))
                .collect()
        });
        let mut out = json.to_string();
        for cell in cells(&serde_json::from_str(json).unwrap()) {
            if let Some(id) = cell["id"].as_str().filter(|id| !old_ids.contains(*id)) {
                assert!(
                    id.len() == 8 && id.chars().all(|c| c.is_ascii_hexdigit()),
                    "{id}"
                );
                out = out.replace(&format!("\"id\": \"{id}\""), "\"id\": \"NEW\"");
            }
        }
        out
    }

    #[test]
    fn a_notebook_reads_as_the_text_jupytext_writes() {
        for case in CASES {
            let t = notebook_to_text(&fixture(&format!("{case}.ipynb"))).unwrap();
            assert_eq!(t.text, fixture(&format!("{case}.py")), "{case}");
        }
    }

    #[test]
    fn an_unchanged_text_writes_the_notebook_back_byte_for_byte() {
        // The R notebook gains jupytext's encoding line on its first write.
        for case in ["python", "header"] {
            let raw = fixture(&format!("{case}.ipynb"));
            let back = text_to_notebook(&fixture(&format!("{case}.py")), Some(&raw)).unwrap();
            assert_eq!(back, raw, "{case}");
        }
    }

    #[test]
    fn an_edited_text_writes_what_jupytext_update_writes() {
        let edits = [
            ("python", "edit"),
            ("python", "add"),
            ("python", "delete"),
            ("python", "reorder"),
            ("python", "meta"),
            ("r", "add"),
            ("header", "edit"),
        ];
        for (case, edit) in edits {
            let raw = fixture(&format!("{case}.ipynb"));
            let text = fixture(&format!("{case}.{edit}.py"));
            let got = text_to_notebook(&text, Some(&raw)).unwrap();
            let want = fixture(&format!("{case}.{edit}.ipynb"));
            assert_eq!(mark_new_ids(&got, Some(&raw)), want, "{case}.{edit}");
        }
    }

    #[test]
    fn a_text_with_no_notebook_makes_the_notebook_jupytext_makes() {
        for case in CASES {
            let got = text_to_notebook(&fixture(&format!("{case}.py")), None).unwrap();
            let want = fixture(&format!("{case}.new.ipynb"));
            assert_eq!(mark_new_ids(&got, None), want, "{case}");
        }
    }

    #[test]
    fn a_notebook_a_write_would_change_reads_with_a_warning() {
        let t = notebook_to_text(&fixture("r.ipynb")).unwrap();
        assert!(t.warning.is_some_and(|w| w.contains("metadata")));
        let t = notebook_to_text(&fixture("python.ipynb")).unwrap();
        assert!(t.warning.is_none(), "{:?}", t.warning);
    }

    #[test]
    fn what_cannot_be_converted_is_refused() {
        let nb = |md: &str, cells: &str| {
            format!("{{\"nbformat\": 4, \"nbformat_minor\": 5, \"metadata\": {md}, \"cells\": [{cells}]}}")
        };
        let heading = "{\"cell_type\": \"heading\", \"metadata\": {}, \"source\": \"\"}";
        let cases = [
            ("{\"cells\": [".to_string(), "not a notebook"),
            ("[]".to_string(), "not an object"),
            (
                "{\"nbformat\": 3, \"nbformat_minor\": 0}".to_string(),
                "nbformat 3",
            ),
            (nb("{}", heading), "cell 1"),
            (
                nb("{\"jupytext\": {\"comment_magics\": false}}", ""),
                "comment_magics",
            ),
            (nb("{\"kernelspec\": {\"language\": \"go\"}}", ""), "go"),
            (
                nb("{\"kernelspec\": {\"name\": \"caf\\u00e9\"}}", ""),
                "YAML",
            ),
        ];
        for (raw, want) in cases {
            let err = notebook_to_text(&raw).err().unwrap_or_default();
            assert!(err.contains(want), "{raw}: {err}");
        }
    }

    #[test]
    fn a_long_headline_is_shortened_as_textwrap_shortens_it() {
        let long = "word ".repeat(20);
        assert_eq!(shorten(&long), "word ".repeat(12).trim_end());
        assert_eq!(shorten(&"x".repeat(70)), "x".repeat(60));
        assert_eq!(headline("# %% [markdown]\n# # Title\n", 0), "# Title");
        assert_eq!(headline("# %%\nx = 1  # the x\n", 0), "the x");
        assert_eq!(headline("# %%\n\n", 3), "Cell 3");
    }
}
