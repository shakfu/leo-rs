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

use crate::node::{status, Ua, VnodeId};
use crate::outline::{Outline, HIDDEN_ROOT_GNX};
use crate::position::Position;
use crate::util;

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

#[derive(Debug)]
pub enum LeoFileError {
    Io(std::io::Error),
    Xml(String),
    NotALeoFile(String),
    /// The file is not UTF-8. See [`read_leo_file`].
    NotUtf8(String),
}

impl std::fmt::Display for LeoFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeoFileError::Io(e) => write!(f, "{e}"),
            LeoFileError::Xml(s) => write!(f, "bad XML in .leo file: {s}"),
            LeoFileError::NotALeoFile(s) => write!(f, "not a readable .leo file: {s}"),
            LeoFileError::NotUtf8(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for LeoFileError {}

impl From<std::io::Error> for LeoFileError {
    fn from(e: std::io::Error) -> Self {
        LeoFileError::Io(e)
    }
}

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
fn parse(contents: &str) -> Result<(Element, Element), LeoFileError> {
    let mut reader = Reader::from_reader(contents.as_bytes());
    reader.config_mut().trim_text(false);
    reader.config_mut().check_end_names = false;
    let mut buf = Vec::new();
    // A stack of elements under construction. The root is a sentinel.
    let mut stack: Vec<Element> = vec![Element::default()];
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(LeoFileError::Xml(e.to_string())),
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
                let s = unescape(&r).map_err(|e| LeoFileError::Xml(e.to_string()))?;
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
        .ok_or_else(|| LeoFileError::NotALeoFile("no <leo_file> element".to_string()))?;
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
) -> Result<Element, LeoFileError> {
    let name = e.name().as_ref().to_owned();
    let mut attrs = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|e| LeoFileError::Xml(e.to_string()))?;
        let key = a.key.as_ref().to_owned();
        // Not `normalized_value()`: it turns newlines and tabs into spaces.
        let val = unescape(&a.value)
            .map_err(|e| LeoFileError::Xml(e.to_string()))?
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
pub fn read_leo_file(path: &str) -> Result<Outline, LeoFileError> {
    let bytes = std::fs::read(path)?;
    let contents = String::from_utf8(bytes).map_err(|e| {
        LeoFileError::NotUtf8(format!(
            "{path}: not UTF-8 (byte {}); this port reads and writes UTF-8 only",
            e.utf8_error().valid_up_to()
        ))
    })?;
    let mut o = Outline::new(path);
    read_leo_string(&mut o, &contents)?;
    Ok(o)
}

/// Rebuild `o` from the text of a `.leo` file.
pub fn read_leo_string(o: &mut Outline, contents: &str) -> Result<(), LeoFileError> {
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
                            for key in [
                                "descendentTnodeUnknownAttributes",
                                "descendentVnodeUnknownAttributes",
                            ] {
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
    if let Some(root) = o.root_position() {
        for p in root.self_and_siblings(o) {
            let ignore = p.is_at_ignore_node(o);
            put_v_element(o, out, &p, ignore, &mut written);
        }
    }
    out.push_str("</vnodes>\n");
}

fn put_v_element(
    o: &mut Outline,
    out: &mut String,
    p: &Position,
    is_ignore: bool,
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
    let attrs = descendent_ua_attrs(o, p);
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
            put_v_element(o, out, &child, is_ignore, written);
        }
        out.push_str("</v>\n");
    } else {
        out.push_str(&v_head);
        out.push_str("</v>\n");
    }
}

/// The `descendent*UnknownAttributes` blobs this node was read with.
///
/// Leo regenerates them by pickling the descendants' uAs. Nothing here reads a
/// pickle, so the blob is written back as it arrived. That is exact while the
/// uAs and the subtree shape are untouched, and this crate never changes
/// either; a caller that restructures an `@auto` tree should expect Leo to
/// rebuild the blob on its next save.
fn descendent_ua_attrs(o: &Outline, p: &Position) -> String {
    let mut out = String::new();
    for key in [
        "descendentTnodeUnknownAttributes",
        "descendentVnodeUnknownAttributes",
    ] {
        if let Some(ua) = o.node(p.v).uas.get(&format!("__native__{key}")) {
            out.push_str(&format!(" {key}=\"{}\"", ua.as_file_text()));
        }
    }
    out
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
        match val {
            Ua::Text(s) => out.push_str(&format!(" {key}={}", util::xml_quoteattr(s))),
            Ua::Opaque(s) => out.push_str(&format!(" {key}=\"{s}\"")),
        }
    }
    out
}

/// Write the outline to `path` in `.leo` format. Returns the path written.
pub fn write_leo_file(o: &mut Outline, path: &str) -> Result<String, LeoFileError> {
    let path = if path.is_empty() {
        o.file_name.clone()
    } else {
        util::finalize(path)
    };
    if path.is_empty() {
        return Err(LeoFileError::NotALeoFile(
            "no file name: pass one, or set outline.file_name".to_string(),
        ));
    }
    // The prolog copies this, and the bytes below are UTF-8. A caller that
    // set it to anything else would write a file that lies about itself.
    if !crate::external::encoding_is_supported(&o.config.leo_file_encoding) {
        return Err(LeoFileError::NotUtf8(format!(
            "leo_file_encoding is {}; this port reads and writes UTF-8 only",
            o.config.leo_file_encoding
        )));
    }
    o.file_name = path.clone();
    let s = outline_to_xml_string(o);
    std::fs::write(&path, s.as_bytes())?;
    o.changed = false;
    Ok(path)
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
        assert!(matches!(err, LeoFileError::NotUtf8(_)), "{err}");
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
        assert!(matches!(err, LeoFileError::NotUtf8(_)), "{err}");
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

    fn read(t: &str) -> Result<Outline, LeoFileError> {
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
    fn an_unknown_entity_is_an_error() {
        assert!(matches!(
            read("<t tx=\"a.1\">&nope;</t>"),
            Err(LeoFileError::Xml(_))
        ));
    }
}
