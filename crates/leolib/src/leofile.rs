//! Reading and writing `.leo` files.
//!
//! A `.leo` file stores the outline's own nodes and nothing else: the contents
//! of `@file`, `@auto` and `@shadow` trees live in the external files and are
//! read separately. `<v>` elements carry the shape, `<t>` elements the body
//! text, and a clone is a second `<v>` naming a gnx that already appeared.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;

use quick_xml::escape::unescape;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{Error, Result};
use crate::node::{status, Ua, VnodeId};
use crate::outline::{Outline, HIDDEN_ROOT_GNX};
use crate::pickle;
use crate::position::Position;
use crate::util;

/// The `<v>` attributes holding the uAs of nodes the file does not otherwise
/// store, parked under a `__native__` key until the read applies them.
const DESCENDENT_UA_KEYS: [&str; 2] = [
    "descendentTnodeUnknownAttributes",
    "descendentVnodeUnknownAttributes",
];

/// Attributes of a `<v>` element that Leo interprets itself.
const NATIVE_VNODE_ATTRIBUTES: &[&str] = &[
    "a",
    "descendentTnodeUnknownAttributes",
    "descendentVnodeUnknownAttributes",
    "expanded",
    "marks",
    "t",
    "tnodeList",
];

/// One `<v>` or `<t>` element, as read from the file.
#[derive(Debug, Default)]
struct Element {
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<Element>,
    name: String,
}

impl Element {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Parse the `.leo` XML into the three sections the reader needs.
fn parse(contents: &str) -> Result<(Element, Element)> {
    let mut reader = Reader::from_reader(contents.as_bytes());
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    // A stack of elements under construction. The root is a sentinel.
    let mut stack: Vec<Element> = vec![Element::default()];
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => {
                return Err(Error::BadXml {
                    detail: e.to_string(),
                })
            }
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                stack.push(element_from_start(&e, &reader)?);
            }
            Ok(Event::Empty(e)) => {
                let el = element_from_start(&e, &reader)?;
                stack.last_mut().unwrap().children.push(el);
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(el);
                }
            }
            // Raw text, not `xml_content()`: that normalizes `\r\n`, and the
            // body must keep the bytes the file holds.
            Ok(Event::Text(e)) => {
                stack.last_mut().unwrap().text.push_str(&e);
            }
            // `&lt;`, `&#10;` and the like arrive apart from the text around them.
            Ok(Event::GeneralRef(e)) => {
                let r = format!("&{};", &*e);
                let s = unescape(&r).map_err(|e| Error::BadXml {
                    detail: e.to_string(),
                })?;
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Ok(Event::CData(e)) => {
                stack.last_mut().unwrap().text.push_str(&e);
            }
            _ => {}
        }
        buf.clear();
    }
    let root = stack.remove(0);
    let leo_file = root
        .children
        .into_iter()
        .find(|e| e.name == "leo_file")
        .ok_or_else(|| Error::NotALeoFile {
            detail: "no <leo_file> element".to_string(),
        })?;
    let mut vnodes = Element::default();
    let mut tnodes = Element::default();
    for child in leo_file.children {
        match child.name.as_str() {
            "vnodes" => vnodes = child,
            "tnodes" => tnodes = child,
            _ => {}
        }
    }
    Ok((vnodes, tnodes))
}

fn element_from_start<R: BufRead>(
    e: &quick_xml::events::BytesStart,
    _reader: &Reader<R>,
) -> Result<Element> {
    let name = e.name().as_ref().to_owned();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| Error::BadXml {
            detail: e.to_string(),
        })?;
        let key = a.key.as_ref().to_owned();
        // Not `normalized_value()`: it turns newlines and tabs into spaces.
        let val = unescape(&a.value)
            .map_err(|e| Error::BadXml {
                detail: e.to_string(),
            })?
            .into_owned();
        attrs.push((key, val));
    }
    Ok(Element {
        attrs,
        text: String::new(),
        children: Vec::new(),
        name,
    })
}

/// Read a `.leo` file into a fresh outline. External files are not read here.
///
/// A file that is not UTF-8 is refused rather than decoded with replacements,
/// for the reason [`crate::external::read_file_to_string`] gives: the writer
/// emits UTF-8, so a lossy read followed by a save would replace the outline's
/// own text. Leo hands the bytes to an XML parser, which honours the encoding
/// in the prolog; matching that needs a decoder this crate does not have.
///
/// The prolog this writes always says utf-8, whatever the file read said, so
/// the declaration and the bytes agree. Leo does the same: it writes with
/// `leo_file_encoding`, a setting, not a property of the file it read.
pub fn read_leo_file(path: &str) -> Result<Outline> {
    let stamp = util::file_stamp(path);
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    let contents = String::from_utf8(bytes).map_err(|e| Error::not_utf8(path, &e.utf8_error()))?;
    let mut o = Outline::new(path);
    read_leo_string(&mut o, &contents)?;
    o.record_file_stamp(path, stamp);
    Ok(o)
}

/// Rebuild `o` from the text of a `.leo` file.
pub fn read_leo_string(o: &mut Outline, contents: &str) -> Result<()> {
    // #1510: characters that are not valid in XML at all. Leo strips them
    // rather than failing, because files in the wild contain them.
    let cleaned: String = contents
        .chars()
        .filter(|c| {
            let n = *c as u32;
            n >= 32 || *c == '\t' || *c == '\n' || *c == '\r'
        })
        .collect();
    let (v_elements, t_elements) = parse(&cleaned)?;

    // <t> elements: body text and unknown attributes, by gnx.
    let mut gnx2body: HashMap<String, String> = HashMap::new();
    let mut gnx2ua: HashMap<String, Vec<(String, Ua)>> = HashMap::new();
    for e in &t_elements.children {
        let Some(gnx) = e.attr("tx") else { continue };
        gnx2body.insert(gnx.to_string(), e.text.clone());
        for (key, val) in &e.attrs {
            // #4875: `_mod_time` is in-memory only; files from 6.8.x still carry it.
            if key == "tx" || key == "_mod_time" {
                continue;
            }
            gnx2ua
                .entry(gnx.to_string())
                .or_default()
                .push((key.clone(), make_ua(key, val)));
        }
    }

    // <v> elements: the shape of the outline.
    let hidden = o.hidden_root;
    o.delete_all_children(hidden);
    let mut visitor = VnodeVisitor {
        o,
        gnx2body: &gnx2body,
        gnx2ua: &gnx2ua,
    };
    visitor.visit(&v_elements, hidden);

    // #1111: every outline has at least one node.
    if o.node(hidden).children.is_empty() {
        let v = o.new_vnode(None);
        o.node_mut(v).h = "newHeadline".to_string();
        o.node_mut(hidden).children.push(v);
        o.node_mut(v).parents.push(hidden);
    }
    Ok(())
}

fn make_ua(key: &str, val: &str) -> Ua {
    if key.starts_with("str_") || key.starts_with("json_") {
        Ua::Text(val.to_string())
    } else {
        Ua::Opaque(val.to_string())
    }
}

struct VnodeVisitor<'a> {
    o: &'a mut Outline,
    gnx2body: &'a HashMap<String, String>,
    gnx2ua: &'a HashMap<String, Vec<(String, Ua)>>,
}

impl VnodeVisitor<'_> {
    fn visit(&mut self, parent_e: &Element, parent_v: VnodeId) {
        for e in &parent_e.children {
            match e.name.as_str() {
                "vh" => {
                    self.o.node_mut(parent_v).h = e.text.clone();
                }
                "v" => {
                    let gnx = e.attr("t").unwrap_or("").to_string();
                    let existing = if gnx.is_empty() {
                        None
                    } else {
                        self.o.find_gnx(&gnx)
                    };
                    match existing {
                        Some(v) => {
                            // A clone. The last body in the file wins, as in Leo.
                            self.o.node_mut(parent_v).children.push(v);
                            self.o.node_mut(v).parents.push(parent_v);
                            if let Some(body) = self.gnx2body.get(&gnx) {
                                self.o.node_mut(v).b = body.clone();
                            }
                        }
                        None => {
                            let v =
                                self.o
                                    .new_vnode(if gnx.is_empty() { None } else { Some(&gnx) });
                            let gnx = self.o.gnx(v).to_string();
                            self.o.node_mut(parent_v).children.push(v);
                            self.o.node_mut(v).parents.push(parent_v);
                            self.o.node_mut(v).b =
                                self.gnx2body.get(&gnx).cloned().unwrap_or_default();
                            self.o.node_mut(v).h = "PLACE HOLDER".to_string();
                            for (key, val) in self.gnx2ua.get(&gnx).into_iter().flatten() {
                                self.o.node_mut(v).uas.insert(key.clone(), val.clone());
                            }
                            for (key, val) in &e.attrs {
                                if !NATIVE_VNODE_ATTRIBUTES.contains(&key.as_str()) {
                                    self.o
                                        .node_mut(v)
                                        .uas
                                        .insert(key.clone(), make_ua(key, val));
                                }
                            }
                            // Two attributes hold pickled uAs for descendants that
                            // the file does not otherwise store. Nothing here
                            // unpickles them, so they are kept verbatim and
                            // written back on the node that carried them.
                            for key in DESCENDENT_UA_KEYS {
                                if let Some(val) = e.attr(key) {
                                    self.o
                                        .node_mut(v)
                                        .uas
                                        .insert(format!("__native__{key}"), Ua::Opaque(val.into()));
                                }
                            }
                            self.visit(e, v);
                        }
                    }
                }
                _ => {}
            }
        }
        self.o.generation += 1;
    }
}

// --- Writing ------------------------------------------------------------

/// Return the outline in `.leo` (XML) format.
pub fn outline_to_xml_string(o: &mut Outline) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "<?xml version=\"1.0\" encoding=\"{}\"?>\n",
        o.config.leo_file_encoding
    ));
    out.push_str("<!-- Created by Leo: https://leo-editor.github.io/leo-editor/leo_toc.html -->\n");
    out.push_str(
        "<leo_file xmlns:leo=\"https://leo-editor.github.io/leo-editor/namespaces/leo-python-editor/1.1\" >\n",
    );
    out.push_str("<leo_header file_format=\"2\"/>\n");
    out.push_str("<globals/>\n");
    out.push_str("<preferences/>\n");
    out.push_str("<find_panel_settings/>\n");
    put_v_elements(o, &mut out);
    put_t_elements(o, &mut out);
    out.push_str("</leo_file>\n");
    out
}

fn put_v_elements(o: &mut Outline, out: &mut String) {
    // The write bit says "this node's body belongs in the .leo file". It is
    // recomputed here rather than inherited: a node that moved out of an @file
    // tree since the last save would otherwise keep a stale answer.
    for v in 0..o.node_count() {
        o.node_mut(VnodeId(v as u32)).clear_bit(status::WRITE);
    }
    out.push_str("<vnodes>\n");
    let mut written: HashSet<String> = HashSet::new();
    // Whether any node has a uA at all, asked once: a blob is rebuilt from
    // the subtree of every `<v>` element, and almost no outline has one.
    let any_uas = (0..o.node_count()).any(|v| {
        o.node(VnodeId(v as u32))
            .uas
            .keys()
            .any(|k| !k.starts_with("__native__"))
    });
    if let Some(root) = o.root_position() {
        for p in root.self_and_siblings(o) {
            let ignore = p.is_at_ignore_node(o);
            put_v_element(o, out, &p, ignore, any_uas, &mut written);
        }
    }
    out.push_str("</vnodes>\n");
}

fn put_v_element(
    o: &mut Outline,
    out: &mut String,
    p: &Position,
    is_ignore: bool,
    any_uas: bool,
    written: &mut HashSet<String>,
) {
    // An external file holds its own tree, so the .leo file stores only the
    // @<file> node itself. @clean is deliberately absent from this list: its
    // file has no sentinels, so the outline is the only record of its shape.
    let is_auto = p.is_at_auto_node(o);
    let is_edit = p.is_at_edit_node(o) && !p.has_children(o);
    let is_external = is_auto
        || is_edit
        || p.is_at_file_node(o)
        || p.is_at_shadow_file_node(o)
        || p.is_at_thin_file_node(o);
    let force_write = if is_ignore || p.is_at_ignore_node(o) {
        true
    } else {
        !is_external
    };
    let gnx = o.gnx(p.v).to_string();
    if force_write {
        o.node_mut(p.v).set_bit(status::WRITE);
    }
    let attrs = descendent_ua_attrs(o, p, any_uas);
    let v_head = format!("<v t=\"{gnx}\"{attrs}>");
    if written.contains(&gnx) {
        out.push_str(&v_head);
        out.push_str("</v>\n");
        return;
    }
    written.insert(gnx);
    let h = util::xml_escape(p.h(o));
    let v_head = format!("{v_head}<vh>{h}</vh>");
    if p.has_children(o) && force_write {
        out.push_str(&v_head);
        out.push('\n');
        for child in p.children(o) {
            put_v_element(o, out, &child, is_ignore, any_uas, written);
        }
        out.push_str("</v>\n");
    } else {
        out.push_str(&v_head);
        out.push_str("</v>\n");
    }
}

/// The `descendentVnodeUnknownAttributes` attribute for this node.
///
/// Leo writes one for every `<v>` element, holding the uAs of each node in
/// its subtree keyed by that node's position relative to it
/// (`fc.putDescendentVnodeUas`). It is the only place the uAs of a node the
/// file does not otherwise store -- one an importer or a sentinel file
/// builds -- can live, so it is rebuilt here rather than copied: a copy names
/// the positions the tree had when it was read.
///
/// A blob [`crate::pickle`] could not read is still a copy, and still goes
/// back as it arrived. `Outline::invalidate_descendent_uas` drops that one
/// when the subtree changes, because its positions no longer hold.
fn descendent_ua_attrs(o: &Outline, p: &Position, any_uas: bool) -> String {
    let mut out = String::new();
    for key in DESCENDENT_UA_KEYS {
        if let Some(ua) = o.node(p.v).uas.get(&format!("__native__{key}")) {
            out.push_str(&format!(" {key}=\"{}\"", ua.as_file_text()));
        }
    }
    // A blob still parked is one the read did not apply, either because it
    // could not be read or because the outline was opened without its
    // external files. Rebuilding beside it would write the same uAs twice.
    if !out.is_empty() {
        return out;
    }
    if let Some(hex) = rebuild_descendent_uas(o, p, any_uas) {
        out.push_str(&format!(" descendentVnodeUnknownAttributes=\"{hex}\""));
    }
    out
}

/// The blob for p's subtree, or None when no node in it has a uA.
fn rebuild_descendent_uas(o: &Outline, p: &Position, any_uas: bool) -> Option<String> {
    if !any_uas {
        return None; // The common case: nothing to walk for.
    }
    let mut items: Vec<(pickle::Value, pickle::Value)> = Vec::new();
    for q in p.self_and_subtree(o) {
        let uas = node_uas_as_values(o, q.v);
        if !uas.is_empty() {
            items.push((
                pickle::Value::Str(archived_position(o, &q, p)),
                pickle::Value::Dict(uas),
            ));
        }
    }
    match items.is_empty() {
        true => None,
        false => Some(pickle::dumps_hexlify(&pickle::Value::Dict(items))),
    }
}

/// A node's own uAs as pickle values, in the order the `.leo` file spells
/// them. The parked blobs are not uAs of this node and stay out.
fn node_uas_as_values(o: &Outline, v: VnodeId) -> Vec<(pickle::Value, pickle::Value)> {
    let mut out = Vec::new();
    for (key, val) in &o.node(v).uas {
        if key.starts_with("__native__") {
            continue;
        }
        let value = match val {
            // A `str_` or `json_` value is text in the file, so it is text
            // here; anything else is already a pickle of the value itself.
            Ua::Text(s) => pickle::Value::Str(s.clone()),
            Ua::Opaque(hex) => match pickle::unhexlify_loads(hex) {
                Ok(value) => value,
                // Not readable, so not rebuildable: leaving it out of the
                // blob is what `invalidate_descendent_uas` would do anyway.
                Err(_) => continue,
            },
        };
        out.push((pickle::Value::Str(key.clone()), value));
    }
    out
}

/// p's position relative to `root`, as Leo's `p.archivedPosition(root_p)`
/// spells it: the child index of each node from the root down, the root
/// itself as 0, joined with periods.
fn archived_position(o: &Outline, p: &Position, root: &Position) -> String {
    let mut parts: Vec<String> = Vec::new();
    for q in p.self_and_parents(o) {
        if q == *root {
            parts.push("0".to_string());
            break;
        }
        parts.push(q.child_index.to_string());
    }
    parts.reverse();
    parts.join(".")
}

/// The vnode an archived position names, relative to `root_v`.
///
/// Leo's `fc.resolveArchivedPosition`. The first index stands for the root
/// itself, whatever it says, and the rest are child indices from there.
fn resolve_archived_position(o: &Outline, root_v: VnodeId, key: &str) -> Option<VnodeId> {
    let mut steps = key.split('.');
    steps.next()?; // The root.
    let mut v = root_v;
    for step in steps {
        let n: usize = step.parse().ok()?;
        v = *o.node(v).children.get(n)?;
    }
    Some(v)
}

/// Give the nodes named by the `descendent*UnknownAttributes` blobs their uAs.
///
/// Leo's `fc.restoreDescendentAttributes`, called once the external files are
/// read: the nodes a `V` blob names by position are the ones an importer or a
/// sentinel file has just built, and the ones a `T` blob names by gnx may be
/// among them. The blob is consumed, because the write rebuilds it from the
/// tree; one this port cannot read stays parked and is written back as it is.
///
/// Reading a `.leo` file without its external files leaves every blob parked,
/// as Leo leaves them: the nodes they name are not there to take them.
pub fn restore_descendent_uas(o: &mut Outline) {
    for v in o.all_unique_nodes() {
        for key in DESCENDENT_UA_KEYS {
            let parked = format!("__native__{key}");
            let Some(ua) = o.node(v).uas.get(&parked) else {
                continue;
            };
            let Ok(blob) = pickle::unhexlify_loads(ua.as_file_text()) else {
                continue; // Left where it is, and written back unchanged.
            };
            let Some(items) = blob.as_dict() else {
                continue;
            };
            let by_gnx = key.starts_with("descendentTnode");
            let mut restored: Vec<(VnodeId, Vec<(String, Ua)>)> = Vec::new();
            for (name, value) in items {
                let (Some(name), Some(uas)) = (name.as_str(), value.as_dict()) else {
                    continue;
                };
                let target = match by_gnx {
                    true => o.find_gnx(name),
                    false => resolve_archived_position(o, v, name),
                };
                let Some(target) = target else {
                    continue; // The node is not there: nothing to give it to.
                };
                let uas = uas
                    .iter()
                    .filter_map(|(k, val)| Some((k.as_str()?.to_string(), ua_from_value(k, val))))
                    .collect();
                restored.push((target, uas));
            }
            o.node_mut(v).uas.remove(&parked);
            for (target, uas) in restored {
                for (k, val) in uas {
                    o.node_mut(target).uas.insert(k, val);
                }
            }
        }
    }
}

/// A blob value as this port stores a uA: text for the keys Leo leaves as
/// text, and the value's own pickle for the rest, which is what a `<t>`
/// element would have spelled.
fn ua_from_value(key: &pickle::Value, value: &pickle::Value) -> Ua {
    let text_key = key
        .as_str()
        .is_some_and(|k| k.starts_with("str_") || k.starts_with("json_"));
    match (text_key, value.as_str()) {
        (true, Some(s)) => Ua::Text(s.to_string()),
        _ => Ua::Opaque(pickle::dumps_hexlify(value)),
    }
}

fn put_t_elements(o: &Outline, out: &mut String) {
    out.push_str("<tnodes>\n");
    let mut by_gnx: Vec<(String, VnodeId)> = o
        .all_unique_nodes()
        .into_iter()
        .map(|v| (o.gnx(v).to_string(), v))
        .collect();
    by_gnx.sort_by(|a, b| a.0.cmp(&b.0));
    for (gnx, v) in by_gnx {
        if o.node(v).status & status::WRITE == 0 {
            continue;
        }
        let ua = put_unknown_attributes(o, v);
        let body = util::xml_escape(&o.node(v).b);
        out.push_str(&format!("<t tx=\"{gnx}\"{ua}>{body}</t>\n"));
    }
    out.push_str("</tnodes>\n");
}

fn put_unknown_attributes(o: &Outline, v: VnodeId) -> String {
    let mut out = String::new();
    for (key, val) in &o.node(v).uas {
        if key.starts_with("__native__") {
            continue;
        }
        // Both kinds are escaped. Leo's `fc.pickle` writes a hexlified
        // pickle, which quoting leaves byte for byte as it is; a value some
        // other writer put there is what the read unescaped, and writing it
        // raw made the `.leo` file unreadable by both implementations.
        out.push_str(&format!(
            " {key}={}",
            util::xml_quoteattr(val.as_file_text())
        ));
    }
    out
}

/// Write the outline to `path` in `.leo` format, and make `path` its file.
/// Returns the path written. Leo's `save-as`, or `save` when `path` is "".
///
/// The outline's file name decides where relative `@<file>` paths resolve,
/// so this also moves where the external files are written.
pub fn write_leo_file(o: &mut Outline, path: &str) -> Result<String> {
    let path = match path.is_empty() {
        true => o.file_name.clone(),
        false => util::finalize(path),
    };
    write_xml(o, &path)?;
    o.file_name = path.clone();
    o.changed = false;
    Ok(path)
}

/// Write a copy of the outline to `path`, as Leo's `save-to`. The outline's
/// file name and changed flag are left alone. Returns the path written.
pub fn write_leo_copy(o: &mut Outline, path: &str) -> Result<String> {
    let path = util::finalize(path);
    write_xml(o, &path)?;
    Ok(path)
}

/// Write the XML to a temporary file and rename it over `path`.
///
/// Leo backs the file up and writes in place. A rename leaves either the old
/// file or the new one after a crash or a full disk, never half of each.
fn write_xml(o: &mut Outline, path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(Error::NotALeoFile {
            detail: "no file name: pass one, or set outline.file_name".to_string(),
        });
    }
    // The prolog copies this, and the bytes below are UTF-8. A caller that
    // set it to anything else would write a file that lies about itself.
    if !crate::external::encoding_is_supported(&o.config.leo_file_encoding) {
        return Err(Error::UnsupportedEncoding {
            encoding: o.config.leo_file_encoding.clone(),
        });
    }
    let s = outline_to_xml_string(o);
    // A body can hold a '\r' of its own, so the compare stays byte for byte.
    crate::external::replace_file(path, &s, false)?;
    o.record_file_stamp(path, util::file_stamp(path));
    Ok(())
}

/// The hidden root's gnx, exported for tests that build outlines by hand.
pub const HIDDEN_ROOT: &str = HIDDEN_ROOT_GNX;

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(o: &mut Outline) -> Outline {
        let xml = outline_to_xml_string(o);
        let mut o2 = Outline::new("");
        read_leo_string(&mut o2, &xml).unwrap();
        o2
    }

    #[test]
    fn a_leo_file_that_is_not_utf8_is_refused() {
        // Lossily decoding it would put U+FFFD in a headline, and the next
        // save would write that back over the outline's own text.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin.leo");
        let mut bytes = b"<?xml version=\"1.0\" encoding=\"latin-1\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n<vnodes>\n<v t=\"a.1\"><vh>caf".to_vec();
        bytes.push(0xe9);
        bytes.extend_from_slice(b"</vh></v>\n</vnodes>\n<tnodes>\n</tnodes>\n</leo_file>\n");
        std::fs::write(&path, &bytes).unwrap();

        let err = read_leo_file(&path.to_string_lossy()).unwrap_err();
        assert!(matches!(err, Error::NotUtf8 { .. }), "{err}");
        assert!(err.to_string().contains("not UTF-8"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn writing_refuses_an_encoding_the_writer_cannot_produce() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.leo").to_string_lossy().to_string();
        let mut o = Outline::new_empty();
        o.config.leo_file_encoding = "latin-1".to_string();
        let err = write_leo_file(&mut o, &path).unwrap_err();
        assert!(matches!(err, Error::UnsupportedEncoding { .. }), "{err}");
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn a_tree_survives_a_round_trip() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        o.set_body(&root, "root body\n");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "child & <friends>");
        o.set_body(&child, "line 1\nline 2\n");
        let o2 = round_trip(&mut o);
        let heads: Vec<String> = o2
            .all_positions()
            .iter()
            .map(|p| p.h(&o2).to_string())
            .collect();
        assert_eq!(heads, vec!["root", "child & <friends>"]);
        assert_eq!(o2.all_positions()[1].b(&o2), "line 1\nline 2\n");
    }

    #[test]
    fn clones_survive_a_round_trip_as_one_vnode() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "shared");
        o.clone_node(&child);
        let o2 = round_trip(&mut o);
        assert_eq!(o2.all_positions().len(), 3);
        assert_eq!(o2.all_unique_positions().len(), 2);
        let a = &o2.all_positions()[1];
        let b = &o2.all_positions()[2];
        assert_eq!(a.v, b.v);
    }

    #[test]
    fn an_at_file_tree_is_not_stored_in_the_leo_file() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file x.py");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "in the external file");
        o.set_body(&child, "print(1)\n");
        let xml = outline_to_xml_string(&mut o);
        assert!(!xml.contains("in the external file"));
        assert!(!xml.contains("print(1)"));
        assert!(xml.contains("@file x.py"));
    }

    #[test]
    fn an_at_clean_tree_is_stored_in_the_leo_file() {
        // @clean files carry no sentinels, so the .leo file is the only record
        // of the tree's shape. Getting this backwards loses the outline.
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@clean x.py");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "kept");
        let xml = outline_to_xml_string(&mut o);
        assert!(xml.contains("kept"));
    }

    fn read(t: &str) -> Result<Outline> {
        let xml = format!(
            "<?xml version=\"1.0\"?>\n<leo_file><vnodes>\
             <v t=\"a.1\"><vh>h &amp; &#x41;</vh></v></vnodes>\
             <tnodes>{t}</tnodes></leo_file>\n"
        );
        let mut o = Outline::new("");
        read_leo_string(&mut o, &xml)?;
        Ok(o)
    }

    #[test]
    fn references_and_line_ends_are_read_as_written() {
        let o = read("<t tx=\"a.1\" str_k=\"x&#10;y\tz &lt;\">1 &lt; 2&#10;&#9;&gt;\r\n&apos;</t>")
            .unwrap();
        let p = &o.all_positions()[0];
        assert_eq!(p.h(&o), "h & A");
        assert_eq!(p.b(&o), "1 < 2\n\t>\r\n'");
        let ua = &o.node(p.v).uas["str_k"];
        assert_eq!(ua, &Ua::Text("x\ny\tz <".to_string()));
    }

    #[test]
    fn an_opaque_ua_is_written_so_the_file_reads_back() {
        // A pickled uA holds hex, which needs no escaping. One some other
        // writer put there does: the read unescapes it, and writing it raw
        // ended the attribute early and broke the file.
        let mut o = read("<t tx=\"a.1\" k=\"a &quot;b&quot; &lt;c&gt;\"></t>").unwrap();
        let p = &o.all_positions()[0];
        assert_eq!(o.node(p.v).uas["k"], Ua::Opaque("a \"b\" <c>".to_string()));
        let xml = outline_to_xml_string(&mut o);
        let mut o2 = Outline::new("");
        read_leo_string(&mut o2, &xml).unwrap();
        let q = &o2.all_positions()[0];
        assert_eq!(o2.node(q.v).uas["k"], o.node(p.v).uas["k"]);
    }

    #[test]
    fn a_pickled_ua_is_written_byte_for_byte() {
        let hex = "80049503000000000000008c0161942e";
        let mut o = read(&format!("<t tx=\"a.1\" k=\"{hex}\"></t>")).unwrap();
        assert!(
            outline_to_xml_string(&mut o).contains(&format!("k=\"{hex}\"")),
            "{}",
            outline_to_xml_string(&mut o)
        );
    }

    /// An `@auto` node carrying a blob of its descendants' pickled uAs.
    fn outline_with_a_descendent_blob() -> Outline {
        let xml = "<?xml version=\"1.0\"?>\n<leo_file><vnodes>\
             <v t=\"a.1\" descendentVnodeUnknownAttributes=\"80049501\">\
             <vh>@auto x.py</vh><v t=\"a.2\"><vh>one</vh></v></v></vnodes>\
             <tnodes></tnodes></leo_file>\n";
        let mut o = Outline::new("");
        read_leo_string(&mut o, xml).unwrap();
        o
    }

    #[test]
    fn a_descendent_ua_blob_is_written_back_when_nothing_moved() {
        let mut o = outline_with_a_descendent_blob();
        assert!(outline_to_xml_string(&mut o).contains("descendentVnodeUnknownAttributes="));
    }

    #[test]
    fn a_descendent_ua_blob_goes_when_its_subtree_is_restructured() {
        // The blob names its descendants by archived position. Leo would
        // restore a moved node's uAs onto whatever now sits at that position.
        let mut o = outline_with_a_descendent_blob();
        let root = o.root_position().unwrap();
        let child = root.children(&o)[0].clone();
        o.insert_before(&child);
        assert!(!outline_to_xml_string(&mut o).contains("descendentVnodeUnknownAttributes="));
    }

    #[test]
    fn a_descendent_ua_blob_survives_an_edit_outside_its_subtree() {
        let mut o = outline_with_a_descendent_blob();
        let root = o.root_position().unwrap();
        let after = o.insert_after(&root);
        o.set_headline(&after, "elsewhere");
        let sub = o.insert_as_last_child(&after);
        o.set_headline(&sub, "below elsewhere");
        assert!(outline_to_xml_string(&mut o).contains("descendentVnodeUnknownAttributes="));
    }

    #[test]
    fn an_unknown_entity_is_an_error() {
        assert!(matches!(
            read("<t tx=\"a.1\">&nope;</t>"),
            Err(Error::BadXml { .. })
        ));
    }
}
