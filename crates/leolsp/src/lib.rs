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
    CodeActionOrCommand, Diagnostic, DiagnosticSeverity, DocumentChanges, GotoDefinitionResponse,
    Hover, HoverContents, MarkedString, OneOf, PublishDiagnosticsParams, WorkspaceEdit,
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
    /// The fixes and refactorings on offer at the position, with the
    /// diagnostics on its line.
    CodeActions,
    /// What could be typed at the position.
    Completion,
}

/// Something a server offers to type at a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    /// A signature or type, when the server gives one.
    pub detail: Option<String>,
    /// `Function`, `Variable` and so on.
    pub kind: Option<String>,
    /// What to type.
    pub text: String,
    /// Where in the body the text replaces, when the server says: on one
    /// row, from the start of the word to the position asked about.
    pub range: Option<(BodyPos, BodyPos)>,
}

/// How many completions a list keeps, best first.
const COMPLETIONS: usize = 200;

/// How many lines `Lsp::log` keeps.
const LOG: usize = 500;

/// Where a configured server is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerState {
    /// Nothing in its language has been visited yet.
    NotStarted,
    /// Started, waiting for its answer to `initialize`.
    Starting,
    Running,
    /// It would not start, or stopped; why.
    Failed(String),
}

/// A fix or refactoring a server offers.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeAction {
    pub title: String,
    /// `quickfix`, `refactor.extract` and so on, when the server says.
    pub kind: Option<String>,
    /// The server's pick when several fix the same thing.
    pub preferred: bool,
    /// Its edit in body rows, or why it cannot be mapped.
    pub edit: Option<Result<Vec<BodyEdit>, String>>,
    /// A command to run on the server after the edit: `execute` sends it.
    pub command: Option<lsp_types::Command>,
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

#[derive(Debug, PartialEq)]
pub enum Event {
    /// The hover text, by line; empty when the server has none.
    Hover(Vec<String>),
    Definition(Vec<Target>),
    Rename(Result<Vec<BodyEdit>, String>),
    CodeActions(Vec<CodeAction>),
    /// The completions on offer, best first.
    Completions(Vec<Completion>),
    /// An edit a server asks to make, as a command it ran asks; its label.
    Edit(String, Result<Vec<BodyEdit>, String>),
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
    /// The node it is rendered from, as of the last sync, and a fingerprint
    /// of what rendering reads: unchanged, it need not be rendered again.
    root: Position,
    fingerprint: u64,
}

/// A document as rendered from the outline, to open or update.
struct Rendered {
    source: Source,
    root: Position,
    fingerprint: u64,
    uri: String,
    language: String,
    doc: Doc,
}

struct Pending {
    request: Request,
    key: String,
    version: i32,
}

/// Makes the server for a language: `Lsp::new` spawns its command.
pub type Connect = Box<dyn FnMut(&ServerConfig, &Path, Wake) -> io::Result<Server>>;

pub struct Lsp {
    configs: Vec<ServerConfig>,
    root: PathBuf,
    wake: Wake,
    connect: Connect,
    /// By language.
    servers: HashMap<String, Server>,
    /// Languages whose server could not start or stopped, said once.
    failed: HashMap<String, String>,
    /// What the servers logged, and what happened to them, oldest first.
    log: std::collections::VecDeque<String>,
    /// By document key: the decoded path of a file, else the URI.
    docs: HashMap<String, Open>,
    /// Which document each synced node is in.
    node_docs: HashMap<String, String>,
    diagnostics: HashMap<String, Vec<Diagnostic>>,
    pending: HashMap<(String, i64), Pending>,
    /// Commands sent with `execute`, so a failure is said.
    executing: HashSet<(String, i64)>,
    /// The outline generation the open documents were last brought up to.
    synced: Option<u64>,
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

/// Which document node p is in, and the node it is rendered from, without
/// rendering it.
fn source_of(o: &Outline, p: &Position) -> Option<(Source, Position)> {
    match goto::find_root(o, p) {
        // A kind whose nodes are code in their own languages, as a markdown
        // file's fence nodes are: each node is a document.
        Some(root)
            if o.kinds()
                .find(root.h(o))
                .is_some_and(|(k, _)| k.nodes_are_documents()) =>
        {
            Some((Source::Node(p.gnx(o).to_string()), p.clone()))
        }
        Some(root) => {
            let path = std::path::absolute(o.full_path(&root)).ok()?;
            Some((Source::File(path), root))
        }
        None => Some((Source::Node(p.gnx(o).to_string()), p.clone())),
    }
}

/// Whether `root` is still the node document `source` is rendered from.
fn is_root(o: &Outline, root: &Position, source: &Source) -> bool {
    o.position_exists(root)
        && match source {
            Source::Node(gnx) => root.gnx(o) == gnx,
            Source::File(path) => {
                root.is_any_at_file_node(o)
                    && std::path::absolute(o.full_path(root)).is_ok_and(|q| q == *path)
            }
        }
}

/// A hash of everything rendering `root`'s document reads: the root's
/// ancestors, whose directives it inherits, and its tree, shape and text.
fn fingerprint(o: &Outline, root: &Position) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    o.file_name.hash(&mut h);
    for p in root.parents(o) {
        (p.h(o), p.b(o)).hash(&mut h);
    }
    for p in root.self_and_subtree(o) {
        (p.level(), p.gnx(o), p.h(o), p.b(o)).hash(&mut h);
    }
    h.finish()
}

/// Render the document `source`, from `root`, in `language`.
fn render(o: &Outline, source: Source, root: Position, language: String) -> Option<Rendered> {
    let (uri, doc) = match &source {
        Source::File(path) => {
            let map = goto::line_map_of(o, &root)?;
            let doc = Doc::new(map.text.clone(), Mapping::File(map));
            (uri::from_path(path), doc)
        }
        Source::Node(gnx) => {
            let doc = Doc::new(goto::body_as_code(o, &root), Mapping::Node(gnx.clone()));
            (format!("untitled:leo/{gnx}"), doc)
        }
    };
    Some(Rendered {
        fingerprint: fingerprint(o, &root),
        source,
        root,
        uri,
        language,
        doc,
    })
}

/// The node whose document `source` is, now.
fn locate(o: &Outline, source: &Source) -> Option<Position> {
    match source {
        Source::Node(gnx) => o.position_of_gnx(gnx),
        Source::File(path) => o.all_unique_positions().into_iter().find(|p| {
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

    /// As `new`, with servers made by `connect`: a test's or a benchmark's
    /// stand-in, connected with `Server::connect`.
    pub fn with_connect(
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
            failed: HashMap::new(),
            log: Default::default(),
            docs: HashMap::new(),
            node_docs: HashMap::new(),
            diagnostics: HashMap::new(),
            pending: HashMap::new(),
            executing: HashSet::new(),
            synced: None,
        }
    }

    /// Call `wake` when a server spawned from now on sends a message.
    pub fn set_wake(&mut self, wake: Wake) {
        self.wake = wake;
    }

    /// What is called when a server sends a message.
    pub fn wake(&self) -> Wake {
        self.wake.clone()
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
            if self.failed.contains_key(language) {
                return None;
            }
            match (self.connect)(&cfg, &self.root, self.wake.clone()) {
                Ok(server) => {
                    self.note(language, &format!("started {}", cfg.command));
                    self.servers.insert(language.to_string(), server);
                }
                Err(e) => {
                    let why = format!("{}: {e}", cfg.command);
                    self.note(language, &why);
                    self.failed.insert(language.to_string(), why.clone());
                    events.push(Event::Message(why));
                    return None;
                }
            }
        }
        self.servers.get_mut(language)
    }

    /// Open the document p is in, and update every open document the
    /// outline changed. Returns messages for servers that would not start.
    ///
    /// A document is rendered only when it opens or its fingerprint changes:
    /// rendering a 6,000-line file takes 4.5 ms, and this runs each time the
    /// selection moves.
    pub fn sync(&mut self, o: &Outline, p: &Position) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some((source, root)) = source_of(o, p) {
            let language = o.get_language(&root);
            let open = self.docs.values().any(|d| d.source == source);
            let served =
                self.config_for(&language).is_some() && !self.failed.contains_key(&language);
            if !open && served {
                if let Some(r) = render(o, source, root, language) {
                    self.update(r, &mut events);
                }
            }
        }
        if self.synced == Some(o.generation) {
            return events;
        }
        self.synced = Some(o.generation);
        let open: Vec<(Source, Position, u64)> = self
            .docs
            .values()
            .map(|d| (d.source.clone(), d.root.clone(), d.fingerprint))
            .collect();
        for (source, root, seen) in open {
            let root = match is_root(o, &root, &source) {
                true => root,
                false => match locate(o, &source) {
                    Some(root) => root,
                    None => continue,
                },
            };
            if fingerprint(o, &root) == seen {
                continue;
            }
            let language = o.get_language(&root);
            if let Some(r) = render(o, source, root, language) {
                self.update(r, &mut events);
            }
        }
        events
    }

    fn update(&mut self, r: Rendered, events: &mut Vec<Event>) {
        let Rendered {
            source,
            root,
            fingerprint,
            uri,
            language,
            doc,
        } = r;
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
        if let Some(open) = self.docs.get_mut(&key) {
            open.root = root.clone();
            open.fingerprint = fingerprint;
        }
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
                    root,
                    fingerprint,
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
            Request::Completion => "textDocument/completion",
            Request::CodeActions => {
                let here = params["position"].clone();
                let on_line: Vec<&Diagnostic> = self
                    .diagnostics
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .filter(|d| d.range.start.line <= line && line <= d.range.end.line)
                    .collect();
                params = json!({
                    "textDocument": {"uri": open.uri},
                    "range": {"start": here, "end": here},
                    "context": {"diagnostics": on_line, "triggerKind": 1},
                });
                "textDocument/codeAction"
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

    /// Render the document p is in and send it now, whatever the outline's
    /// generation says: for a caller that changed p's body in place, as the
    /// INSERT working copy is put there for a completion.
    pub fn refresh(&mut self, o: &Outline, p: &Position) -> Vec<Event> {
        let mut events = Vec::new();
        let Some((source, root)) = source_of(o, p) else {
            return events;
        };
        let language = o.get_language(&root);
        if self.config_for(&language).is_some() && !self.failed.contains_key(&language) {
            if let Some(r) = render(o, source, root, language) {
                self.update(r, &mut events);
            }
        }
        events
    }

    /// Run a code action's command on the server of node `gnx`'s document.
    /// An edit it makes arrives from `poll` as `Event::Edit`.
    pub fn execute(&mut self, gnx: &str, command: &lsp_types::Command) -> Result<(), String> {
        let language = self
            .node_docs
            .get(gnx)
            .and_then(|k| self.docs.get(k))
            .map(|d| d.language.clone())
            .ok_or("no language server for this node")?;
        let server = self
            .servers
            .get_mut(&language)
            .ok_or("no language server for this node")?;
        let params = json!({"command": command.command, "arguments": command.arguments});
        let id = server.request("workspace/executeCommand", params);
        self.executing.insert((language, id));
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
                self.note(&language, &error);
                self.failed.insert(language.clone(), error.clone());
                events.push(Event::Message(error));
            }
            for msg in incoming {
                match msg {
                    Incoming::Notification { method, params } => {
                        if let Some(event) = self.notification(&language, &method, params) {
                            events.push(event);
                        }
                    }
                    Incoming::Request { id, method, params } => {
                        let (event, result) = self.server_request(&method, params, encoding);
                        let server = self.servers.get_mut(&language).expect("listed");
                        server.respond(id, result);
                        events.extend(event);
                    }
                    Incoming::Response { id, result } => {
                        if self.executing.remove(&(language.clone(), id)) {
                            if let Err(e) = result {
                                events.push(Event::Message(format!("language server: {e}")));
                            }
                            continue;
                        }
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

    /// A request a server makes that the front end acts on, and the answer.
    ///
    /// `workspace/applyEdit` is answered applied when its edit maps to
    /// bodies: the front end applies it on the next poll unless the user is
    /// typing, and the next sync sends the server the text either way.
    fn server_request(&self, method: &str, params: Value, enc: Encoding) -> (Option<Event>, Value) {
        match method {
            "workspace/applyEdit" => {
                let label = params["label"].as_str().unwrap_or("edit").to_string();
                let edits = serde_json::from_value::<WorkspaceEdit>(params["edit"].clone())
                    .map_err(|e| format!("a malformed edit: {e}"))
                    .and_then(|edit| self.body_edits(edit, enc));
                let answer = match &edits {
                    Ok(_) => json!({"applied": true}),
                    Err(e) => json!({"applied": false, "failureReason": e}),
                };
                (Some(Event::Edit(label, edits)), answer)
            }
            _ => (None, Value::Null),
        }
    }

    /// Add a line to the log, as `language: text`, dropping the oldest past
    /// `LOG` lines.
    fn note(&mut self, language: &str, text: &str) {
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            self.log.push_back(format!("{language}: {line}"));
        }
        while self.log.len() > LOG {
            self.log.pop_front();
        }
    }

    /// The log, oldest first: servers started and stopped, what they wrote
    /// to stderr, and their `window/logMessage` and `window/showMessage`.
    pub fn log(&self) -> impl Iterator<Item = &String> {
        self.log.iter()
    }

    /// Each configured server's state, in the settings' order.
    pub fn status(&self) -> Vec<(ServerConfig, ServerState)> {
        self.configs
            .iter()
            .map(|cfg| {
                let state = match (
                    self.servers.get(&cfg.language),
                    self.failed.get(&cfg.language),
                ) {
                    (Some(s), _) if s.initialized => ServerState::Running,
                    (Some(_), _) => ServerState::Starting,
                    (None, Some(why)) => ServerState::Failed(why.clone()),
                    (None, None) => ServerState::NotStarted,
                };
                (cfg.clone(), state)
            })
            .collect()
    }

    fn notification(&mut self, language: &str, method: &str, params: Value) -> Option<Event> {
        match method {
            "textDocument/publishDiagnostics" => {
                let params: PublishDiagnosticsParams = serde_json::from_value(params).ok()?;
                self.diagnostics
                    .insert(key_of(params.uri.as_str()), params.diagnostics);
                Some(Event::Diagnostics)
            }
            "window/showMessage" => {
                let message = params["message"].as_str()?.to_string();
                self.note(language, &message);
                Some(Event::Message(message))
            }
            "window/logMessage" | server::STDERR => {
                self.note(language, params["message"].as_str()?);
                None
            }
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
            Request::Completion => {
                let found: Option<lsp_types::CompletionResponse> =
                    serde_json::from_value(value).unwrap_or(None);
                let mut items = match found {
                    None => vec![],
                    Some(lsp_types::CompletionResponse::Array(items)) => items,
                    Some(lsp_types::CompletionResponse::List(list)) => list.items,
                };
                items.sort_by(|a, b| {
                    let key = |i: &lsp_types::CompletionItem| {
                        i.sort_text.clone().unwrap_or_else(|| i.label.clone())
                    };
                    key(a).cmp(&key(b))
                });
                items.truncate(COMPLETIONS);
                let doc = self.docs.get(&pending.key).map(|d| &d.doc);
                let completions = items
                    .into_iter()
                    .map(|item| {
                        let (text, range) = match item.text_edit {
                            Some(lsp_types::CompletionTextEdit::Edit(e)) => {
                                (e.new_text, Some(e.range))
                            }
                            Some(lsp_types::CompletionTextEdit::InsertAndReplace(e)) => {
                                (e.new_text, Some(e.insert))
                            }
                            None => (item.insert_text.unwrap_or_else(|| item.label.clone()), None),
                        };
                        let range = range.zip(doc).and_then(|(r, doc)| {
                            let (s, e) = (r.start, r.end);
                            doc.range_to_body((s.line, s.character), (e.line, e.character), enc)
                                .filter(|(a, b)| a.row == b.row)
                        });
                        Completion {
                            label: item.label,
                            detail: item.detail,
                            kind: item.kind.map(|k| format!("{k:?}")),
                            text,
                            range,
                        }
                    })
                    .collect();
                Event::Completions(completions)
            }
            Request::CodeActions => {
                if self.docs.get(&pending.key).map(|d| d.version) != Some(pending.version) {
                    return Event::Message(
                        "the text changed while the server worked; ask again".into(),
                    );
                }
                let list: Option<Vec<CodeActionOrCommand>> =
                    serde_json::from_value(value).unwrap_or(None);
                let actions = list
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|a| match a {
                        CodeActionOrCommand::Command(c) => Some(CodeAction {
                            title: c.title.clone(),
                            kind: None,
                            preferred: false,
                            edit: None,
                            command: Some(c),
                        }),
                        CodeActionOrCommand::CodeAction(a) if a.disabled.is_none() => {
                            Some(CodeAction {
                                title: a.title,
                                kind: a.kind.map(|k| k.as_str().to_string()),
                                preferred: a.is_preferred.unwrap_or(false),
                                edit: a.edit.map(|e| self.body_edits(e, enc)),
                                command: a.command,
                            })
                        }
                        CodeActionOrCommand::CodeAction(_) => None,
                    })
                    .collect();
                Event::CodeActions(actions)
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
            .and_then(|map| body(&Doc::new(map.text.clone(), Mapping::File(map))));
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
                let pieces = open
                    .doc
                    .edit_to_body(
                        (r.line, r.character),
                        (s.line, s.character),
                        &e.new_text,
                        enc,
                    )
                    .ok_or("the edit crosses lines no single body writes")?;
                out.extend(pieces.into_iter().map(|(start, end, text)| BodyEdit {
                    start,
                    end,
                    text,
                }));
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
