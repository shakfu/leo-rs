//! leolsp: language servers over a leolib outline.
//!
//! A server sees files, not nodes. Each node in an external file is part of
//! that file's document, whose text is the file as leolib would write it; a
//! node in no file is a document of its own. `map` turns positions one way
//! and the other, so diagnostics, hovers, definitions and renames arrive in
//! body rows and columns.
//!
//! Servers start only when the user's settings name one for a language: a
//! server runs code from the project it is pointed at (`rust-analyzer` runs
//! build scripts), and a `.leo` file from someone else must not choose it.

pub mod map;
pub mod server;
pub mod uri;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use leolib::{goto, Outline, Position};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DocumentChanges, GotoDefinitionResponse, Hover, HoverContents,
    MarkedString, OneOf, PublishDiagnosticsParams, WorkspaceEdit,
};
use serde_json::{json, Value};

pub use map::BodyPos;
use map::{Doc, Encoding, Mapping};
use server::{Incoming, Server, Wake};

/// A server to start for one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConfig {
    /// Leo's name for the language, as `@language` spells it.
    pub language: String,
    /// The program and its arguments, split on blanks.
    pub command: String,
}

/// What a front end asks about a body position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Hover,
    Definition,
    Rename(String),
}

/// Where a definition is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Body(BodyPos),
    /// In a file this outline does not hold, 0-based line and column.
    File {
        path: PathBuf,
        line: u32,
        col: u32,
    },
}

/// One replacement in one body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyEdit {
    pub start: BodyPos,
    pub end: BodyPos,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

/// A diagnostic in one body: from (row, col) to (end_row, end_col).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyDiagnostic {
    pub row: usize,
    pub col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The hover text, by line; empty when the server has none.
    Hover(Vec<String>),
    Definition(Vec<Target>),
    Rename(Result<Vec<BodyEdit>, String>),
    /// A server's message, or why one could not start or answer.
    Message(String),
    /// Some document's diagnostics changed.
    Diagnostics,
}

/// Where a document's text comes from, to rebuild it after an edit.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    File(PathBuf),
    Node(String),
}

struct Open {
    source: Source,
    uri: String,
    language: String,
    version: i32,
    doc: Doc,
}

struct Pending {
    request: Request,
    key: String,
    version: i32,
}

type Connect = Box<dyn FnMut(&ServerConfig, &Path, Wake) -> io::Result<Server>>;

pub struct Lsp {
    configs: Vec<ServerConfig>,
    root: PathBuf,
    wake: Wake,
    connect: Connect,
    /// By language.
    servers: HashMap<String, Server>,
    /// Languages whose server could not start or stopped, said once.
    failed: HashSet<String>,
    /// By document key: the decoded path of a file, else the URI.
    docs: HashMap<String, Open>,
    /// Which document each synced node is in.
    node_docs: HashMap<String, String>,
    diagnostics: HashMap<String, Vec<Diagnostic>>,
    pending: HashMap<(String, i64), Pending>,
}

/// The key a URI is filed under. A server may spell a path's URI otherwise
/// than we did, so a file is known by its path.
fn key_of(uri: &str) -> String {
    match uri::to_path(uri) {
        Some(path) => path.to_string_lossy().into_owned(),
        None => uri.to_string(),
    }
}

/// The LSP language identifier of Leo's language name.
pub fn language_id(language: &str) -> &str {
    match language {
        "cplusplus" => "cpp",
        "shell" => "shellscript",
        "csharp" => "csharp",
        other => other,
    }
}

/// The document node p is in, if it maps.
fn document_for(o: &Outline, p: &Position) -> Option<(Source, String, String, Doc)> {
    match goto::find_root(o, p) {
        Some(root) => {
            let map = goto::line_map_of(o, &root)?;
            let path = std::path::absolute(o.full_path(&root)).ok()?;
            let doc = Doc {
                text: map.text.clone(),
                mapping: Mapping::File(map),
            };
            Some((
                Source::File(path.clone()),
                uri::from_path(&path),
                o.get_language(&root),
                doc,
            ))
        }
        None => {
            let gnx = p.gnx(o).to_string();
            let doc = Doc {
                text: goto::body_as_code(o, p),
                mapping: Mapping::Node(gnx.clone()),
            };
            let uri = format!("untitled:leo/{gnx}");
            Some((Source::Node(gnx), uri, o.get_language(p), doc))
        }
    }
}

/// The node whose document `source` is, now.
fn locate(o: &Outline, source: &Source) -> Option<Position> {
    let positions = o.all_unique_positions();
    match source {
        Source::Node(gnx) => positions.into_iter().find(|p| p.gnx(o) == gnx),
        Source::File(path) => positions.into_iter().find(|p| {
            p.is_any_at_file_node(o)
                && std::path::absolute(o.full_path(p)).is_ok_and(|q| q == *path)
        }),
    }
}

impl Lsp {
    /// Servers from `configs`, started on first use, with `root` as their
    /// workspace. `wake` is called when a message arrives.
    pub fn new(configs: Vec<ServerConfig>, root: PathBuf, wake: Wake) -> Lsp {
        let connect: Connect = Box::new(|cfg, root, wake| Server::spawn(&cfg.command, root, wake));
        Lsp::with_connect(configs, root, wake, connect)
    }

    fn with_connect(
        configs: Vec<ServerConfig>,
        root: PathBuf,
        wake: Wake,
        connect: Connect,
    ) -> Lsp {
        Lsp {
            configs,
            root,
            wake,
            connect,
            servers: HashMap::new(),
            failed: HashSet::new(),
            docs: HashMap::new(),
            node_docs: HashMap::new(),
            diagnostics: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Call `wake` when a server spawned from now on sends a message.
    pub fn set_wake(&mut self, wake: Wake) {
        self.wake = wake;
    }

    fn config_for(&self, language: &str) -> Option<ServerConfig> {
        self.configs
            .iter()
            .find(|c| c.language == language || c.language == language_id(language))
            .cloned()
    }

    /// The running server for `language`, started if need be. A server that
    /// cannot start is reported once, in `events`.
    fn server(&mut self, language: &str, events: &mut Vec<Event>) -> Option<&mut Server> {
        if !self.servers.contains_key(language) {
            let cfg = self.config_for(language)?;
            if self.failed.contains(language) {
                return None;
            }
            match (self.connect)(&cfg, &self.root, self.wake.clone()) {
                Ok(server) => {
                    self.servers.insert(language.to_string(), server);
                }
                Err(e) => {
                    self.failed.insert(language.to_string());
                    events.push(Event::Message(format!("{}: {e}", cfg.command)));
                    return None;
                }
            }
        }
        self.servers.get_mut(language)
    }

    /// Open or update the document p is in, and every open document the
    /// outline changed. Returns messages for servers that would not start.
    pub fn sync(&mut self, o: &Outline, p: &Position) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some((source, uri, language, doc)) = document_for(o, p) {
            if self.config_for(&language).is_some() {
                self.update(source, uri, language, doc, &mut events);
            }
        }
        let open: Vec<Source> = self.docs.values().map(|d| d.source.clone()).collect();
        for source in open {
            let Some(q) = locate(o, &source) else {
                continue;
            };
            if let Some((source, uri, language, doc)) = document_for(o, &q) {
                self.update(source, uri, language, doc, &mut events);
            }
        }
        events
    }

    fn update(
        &mut self,
        source: Source,
        uri: String,
        language: String,
        doc: Doc,
        events: &mut Vec<Event>,
    ) {
        let key = key_of(&uri);
        match &doc.mapping {
            Mapping::File(map) => {
                for line in &map.lines {
                    self.node_docs.insert(line.gnx.clone(), key.clone());
                }
            }
            Mapping::Node(gnx) => {
                self.node_docs.insert(gnx.clone(), key.clone());
            }
        }
        if self.server(&language, events).is_none() {
            return;
        }
        let server = self.servers.get_mut(&language).expect("started");
        match self.docs.get_mut(&key) {
            Some(open) if open.doc.text == doc.text => open.doc = doc,
            Some(open) => {
                open.version += 1;
                server.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {"uri": open.uri, "version": open.version},
                        "contentChanges": [{"text": doc.text}],
                    }),
                );
                open.doc = doc;
            }
            None => {
                server.notify(
                    "textDocument/didOpen",
                    json!({"textDocument": {
                        "uri": uri, "languageId": language_id(&language),
                        "version": 1, "text": doc.text,
                    }}),
                );
                let open = Open {
                    source,
                    uri,
                    language,
                    version: 1,
                    doc,
                };
                self.docs.insert(key, open);
            }
        }
    }

    /// Whether node `gnx` is in a document a server has.
    pub fn serves(&self, gnx: &str) -> bool {
        self.node_docs
            .get(gnx)
            .and_then(|k| self.docs.get(k))
            .is_some_and(|d| self.servers.contains_key(&d.language))
    }

    /// Ask about row `row`, column `col` of node `gnx`'s body. The answer
    /// arrives from `poll`. `sync` first, so the server has the text.
    pub fn request(
        &mut self,
        gnx: &str,
        row: usize,
        col: usize,
        request: Request,
    ) -> Result<(), String> {
        let key = self
            .node_docs
            .get(gnx)
            .cloned()
            .ok_or("no language server for this node")?;
        let open = self
            .docs
            .get(&key)
            .ok_or("no language server for this node")?;
        let server = self
            .servers
            .get_mut(&open.language)
            .ok_or("no language server for this node")?;
        let at = BodyPos {
            gnx: gnx.to_string(),
            row,
            col,
        };
        let (line, character) = open
            .doc
            .to_doc(&at, server.encoding)
            .ok_or("this line is not in the file as written")?;
        let mut params = json!({
            "textDocument": {"uri": open.uri},
            "position": {"line": line, "character": character},
        });
        let method = match &request {
            Request::Hover => "textDocument/hover",
            Request::Definition => "textDocument/definition",
            Request::Rename(name) => {
                params["newName"] = json!(name);
                "textDocument/rename"
            }
        };
        let id = server.request(method, params);
        let pending = Pending {
            request,
            key,
            version: open.version,
        };
        self.pending.insert((open.language.clone(), id), pending);
        Ok(())
    }

    /// What the servers have sent since the last poll.
    pub fn poll(&mut self, o: &Outline) -> Vec<Event> {
        let mut events = Vec::new();
        let languages: Vec<String> = self.servers.keys().cloned().collect();
        for language in languages {
            let server = self.servers.get_mut(&language).expect("listed");
            let incoming = server.poll();
            let encoding = server.encoding;
            if let Some(error) = server.error.take() {
                self.servers.remove(&language);
                self.failed.insert(language.clone());
                events.push(Event::Message(error));
            }
            for msg in incoming {
                match msg {
                    Incoming::Notification { method, params } => {
                        if let Some(event) = self.notification(&method, params) {
                            events.push(event);
                        }
                    }
                    Incoming::Response { id, result } => {
                        let Some(pending) = self.pending.remove(&(language.clone(), id)) else {
                            continue;
                        };
                        events.push(match result {
                            Ok(value) => self.response(o, pending, value, encoding),
                            Err(e) => Event::Message(format!("language server: {e}")),
                        });
                    }
                }
            }
        }
        events
    }

    fn notification(&mut self, method: &str, params: Value) -> Option<Event> {
        match method {
            "textDocument/publishDiagnostics" => {
                let params: PublishDiagnosticsParams = serde_json::from_value(params).ok()?;
                self.diagnostics
                    .insert(key_of(params.uri.as_str()), params.diagnostics);
                Some(Event::Diagnostics)
            }
            "window/showMessage" => Some(Event::Message(params["message"].as_str()?.to_string())),
            _ => None,
        }
    }

    fn response(&self, o: &Outline, pending: Pending, value: Value, enc: Encoding) -> Event {
        match pending.request {
            Request::Hover => {
                let hover: Option<Hover> = serde_json::from_value(value).unwrap_or(None);
                Event::Hover(hover.map_or_else(Vec::new, |h| hover_lines(h.contents)))
            }
            Request::Definition => {
                let found: Option<GotoDefinitionResponse> =
                    serde_json::from_value(value).unwrap_or(None);
                let locations: Vec<(String, lsp_types::Position)> = match found {
                    None => vec![],
                    Some(GotoDefinitionResponse::Scalar(l)) => {
                        vec![(l.uri.as_str().to_string(), l.range.start)]
                    }
                    Some(GotoDefinitionResponse::Array(ls)) => ls
                        .into_iter()
                        .map(|l| (l.uri.as_str().to_string(), l.range.start))
                        .collect(),
                    Some(GotoDefinitionResponse::Link(ls)) => ls
                        .into_iter()
                        .map(|l| {
                            (
                                l.target_uri.as_str().to_string(),
                                l.target_selection_range.start,
                            )
                        })
                        .collect(),
                };
                let targets = locations
                    .into_iter()
                    .filter_map(|(uri, at)| self.target(o, &uri, at, enc))
                    .collect();
                Event::Definition(targets)
            }
            Request::Rename(_) => {
                if self.docs.get(&pending.key).map(|d| d.version) != Some(pending.version) {
                    return Event::Rename(Err(
                        "the text changed while the server worked; rename again".into(),
                    ));
                }
                let edit: Option<WorkspaceEdit> = serde_json::from_value(value).unwrap_or(None);
                Event::Rename(match edit {
                    Some(edit) => self.body_edits(edit, enc),
                    None => Err("the server has nothing to rename here".into()),
                })
            }
        }
    }

    /// A definition's place: a body, if the outline holds its file.
    fn target(
        &self,
        o: &Outline,
        uri: &str,
        at: lsp_types::Position,
        enc: Encoding,
    ) -> Option<Target> {
        let key = key_of(uri);
        let body = |doc: &Doc| doc.to_body(at.line, at.character, enc).map(Target::Body);
        if let Some(open) = self.docs.get(&key) {
            if let Some(t) = body(&open.doc) {
                return Some(t);
            }
        }
        let path = uri::to_path(uri)?;
        let held = locate(o, &Source::File(path.clone()))
            .and_then(|root| goto::line_map_of(o, &root))
            .and_then(|map| {
                body(&Doc {
                    text: map.text.clone(),
                    mapping: Mapping::File(map),
                })
            });
        Some(held.unwrap_or(Target::File {
            path,
            line: at.line,
            col: at.character,
        }))
    }

    /// A workspace edit as body edits, all or none: an edit outside the
    /// documents this outline holds, or across what one body writes, refuses
    /// the whole.
    fn body_edits(&self, edit: WorkspaceEdit, enc: Encoding) -> Result<Vec<BodyEdit>, String> {
        let mut by_uri: Vec<(String, Vec<lsp_types::TextEdit>)> = Vec::new();
        if let Some(changes) = edit.changes {
            by_uri.extend(
                changes
                    .into_iter()
                    .map(|(u, e)| (u.as_str().to_string(), e)),
            );
        }
        match edit.document_changes {
            None => {}
            Some(DocumentChanges::Edits(edits)) => {
                for e in edits {
                    let edits = e
                        .edits
                        .into_iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e,
                            OneOf::Right(a) => a.text_edit,
                        })
                        .collect();
                    by_uri.push((e.text_document.uri.as_str().to_string(), edits));
                }
            }
            Some(DocumentChanges::Operations(_)) => {
                return Err("the server would create, rename or delete files".into());
            }
        }
        let mut out = Vec::new();
        for (uri, edits) in by_uri {
            let open = self.docs.get(&key_of(&uri)).ok_or_else(|| {
                format!("the edit reaches a file this outline does not hold: {uri}")
            })?;
            for e in edits {
                let (r, s) = (e.range.start, e.range.end);
                let (start, end) = open
                    .doc
                    .range_to_body((r.line, r.character), (s.line, s.character), enc)
                    .ok_or("the edit crosses lines no single body writes")?;
                out.push(BodyEdit {
                    start,
                    end,
                    text: e.new_text,
                });
            }
        }
        Ok(out)
    }

    /// The diagnostics in node `gnx`'s body, first first. One that starts on
    /// a line no body row writes, such as a sentinel, is not shown.
    pub fn diagnostics(&self, gnx: &str) -> Vec<BodyDiagnostic> {
        let Some(key) = self.node_docs.get(gnx) else {
            return vec![];
        };
        let (Some(open), Some(list)) = (self.docs.get(key), self.diagnostics.get(key)) else {
            return vec![];
        };
        let enc = self
            .servers
            .get(&open.language)
            .map_or(Encoding::Utf16, |s| s.encoding);
        let mut out: Vec<BodyDiagnostic> = list
            .iter()
            .filter_map(|d| {
                let (r, s) = (d.range.start, d.range.end);
                let start = open.doc.to_body(r.line, r.character, enc)?;
                if start.gnx != gnx {
                    return None;
                }
                // An end in another body is cut at the end of the first row.
                let end = open
                    .doc
                    .to_body(s.line, s.character, enc)
                    .filter(|e| e.gnx == gnx && e.row >= start.row)
                    .unwrap_or(BodyPos {
                        row: start.row,
                        col: usize::MAX,
                        ..start.clone()
                    });
                Some(BodyDiagnostic {
                    row: start.row,
                    col: start.col,
                    end_row: end.row,
                    end_col: end.col,
                    severity: match d.severity {
                        Some(DiagnosticSeverity::WARNING) => Severity::Warning,
                        Some(DiagnosticSeverity::INFORMATION) => Severity::Information,
                        Some(DiagnosticSeverity::HINT) => Severity::Hint,
                        _ => Severity::Error,
                    },
                    message: d.message.clone(),
                })
            })
            .collect();
        out.sort_by_key(|d| (d.row, d.col));
        out
    }
}

/// Hover contents as lines of plain text. Markdown is shown as written.
fn hover_lines(contents: HoverContents) -> Vec<String> {
    let marked = |m: MarkedString| match m {
        MarkedString::String(s) => s,
        MarkedString::LanguageString(l) => l.value,
    };
    let text = match contents {
        HoverContents::Scalar(m) => marked(m),
        HoverContents::Array(ms) => ms.into_iter().map(marked).collect::<Vec<_>>().join("\n\n"),
        HoverContents::Markup(m) => m.value,
    };
    text.lines().map(str::to_string).collect()
}

#[cfg(test)]
mod tests;
