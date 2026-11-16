//! The outline over MCP: the tools a client may call, and the app's side of
//! `leomcp`'s server.
//!
//! Nodes are named by gnx, which an edit does not change. Reading is all a
//! client may do until the settings allow `mcp-edit`; saving needs
//! `mcp-save` too. Every edit goes through `Document`, so it is one undo step
//! the user can take back, and the status line says what the client did.
//! An edit waits for NORMAL: a client must not change text under the
//! user's typing.

use serde_json::{json, Value};

use leolib::{Place, Position, VnodeId};
use leomcp::{Call, Server, Tool};

use super::*;
use crate::config::Mcp as McpSettings;

/// Nodes `outline` returns at most, so one call cannot return the whole of
/// a 20,000-node outline.
const MOST_NODES: usize = 500;

/// What a client may do, from the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Access {
    pub edit: bool,
    pub save: bool,
    /// The node whose subtree is all the client may read or edit; None
    /// for the whole outline.
    pub within: Option<VnodeId>,
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

/// Every tool, whatever the client may use: the reply says what a setting
/// forbids, which a missing tool would not.
pub fn tools() -> Vec<Tool> {
    let gnx = json!({"type": "string", "description": "the node's gnx"});
    vec![
        Tool {
            name: "outline",
            description: "The outline as a tree of nodes: gnx, headline, whether it has a body, and its children. From the top, or from `gnx`, to `depth` levels (default 2); at most 500 nodes.",
            input_schema: schema(json!({"gnx": gnx, "depth": {"type": "integer"}}), &[]),
        },
        Tool {
            name: "read_node",
            description: "A node's headline, body, ancestors, children and, for an @file node, its file's path.",
            input_schema: schema(json!({ "gnx": gnx }), &["gnx"]),
        },
        Tool {
            name: "search",
            description: "Nodes whose headline or body matches `pattern`, a regex, case-insensitive unless it has a capital. `scope` is headlines, bodies or both (default both). At most `limit` matches (default 50).",
            input_schema: schema(
                json!({"pattern": {"type": "string"}, "scope": {"type": "string", "enum": ["headlines", "bodies", "both"]}, "limit": {"type": "integer"}}),
                &["pattern"],
            ),
        },
        Tool {
            name: "selection",
            description: "The node the user has selected, the pane with focus, the body cursor, and the mode.",
            input_schema: schema(json!({}), &[]),
        },
        Tool {
            name: "set_headline",
            description: "Change a node's headline; `note` is a warning about references it rewrote, if any. Needs mcp-edit.",
            input_schema: schema(json!({"gnx": gnx, "headline": {"type": "string"}}), &["gnx", "headline"]),
        },
        Tool {
            name: "set_body",
            description: "Replace a node's body. Needs mcp-edit.",
            input_schema: schema(json!({"gnx": gnx, "body": {"type": "string"}}), &["gnx", "body"]),
        },
        Tool {
            name: "insert_node",
            description: "Insert a node before, after or as the last child of the node `gnx`, with a headline and an optional body; returns the new node's gnx. Needs mcp-edit.",
            input_schema: schema(
                json!({"gnx": gnx, "place": {"type": "string", "enum": ["before", "after", "inside"]}, "headline": {"type": "string"}, "body": {"type": "string"}}),
                &["gnx", "place", "headline"],
            ),
        },
        Tool {
            name: "delete_node",
            description: "Delete a node and its subtree. Needs mcp-edit.",
            input_schema: schema(json!({ "gnx": gnx }), &["gnx"]),
        },
        Tool {
            name: "move_node",
            description: "Move the node `gnx` before, after or into the node `target`. Needs mcp-edit.",
            input_schema: schema(
                json!({"gnx": gnx, "target": {"type": "string"}, "place": {"type": "string", "enum": ["before", "after", "inside"]}}),
                &["gnx", "target", "place"],
            ),
        },
        Tool {
            name: "select_node",
            description: "Select a node in the user's outline, to show it. Needs mcp-edit.",
            input_schema: schema(json!({ "gnx": gnx }), &["gnx"]),
        },
        Tool {
            name: "save",
            description: "Save the .leo file and every changed external file, as Ctrl-S does; a file that would be overwritten or created waits for the user. Needs mcp-save.",
            input_schema: schema(json!({}), &[]),
        },
    ]
}

fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("missing argument: {key}"))
}

fn place(args: &Value) -> Result<Place, String> {
    match string(args, "place")? {
        "before" => Ok(Place::Before),
        "after" => Ok(Place::After),
        "inside" => Ok(Place::Inside),
        other => Err(format!("place is before, after or inside, not {other}")),
    }
}

impl App {
    /// Start, stop or restart the MCP server as `settings` say. A token is
    /// made if there is none; the caller saves it.
    pub fn set_mcp(&mut self, settings: &McpSettings) -> Result<(), String> {
        self.mcp_access = Access {
            edit: settings.edit,
            save: settings.save && settings.edit,
            within: None,
        };
        let want = settings.enabled.then_some(settings.port);
        let token = settings.token.clone().unwrap_or_default();
        if want.is_some() && token.is_empty() {
            self.mcp = None;
            return Err("MCP needs a token; the settings make one".into());
        }
        // A restart on the same port would race the old listener, which
        // frees it only after its next accept, so change the token in place.
        if let Some(server) = self.mcp.as_ref().filter(|s| Some(s.port()) == want) {
            server.set_token(&token);
            return Ok(());
        }
        self.mcp = None;
        let Some(port) = want else { return Ok(()) };
        match Server::start(port, &token, tools(), "leo") {
            Ok(server) => {
                self.mcp = Some(server);
                Ok(())
            }
            Err(e) => Err(format!("MCP could not listen on 127.0.0.1:{port}: {e}")),
        }
    }

    /// Answer the tool calls waiting. True if any came.
    pub(super) fn poll_mcp(&mut self) -> bool {
        let calls: Vec<Call> = match &self.mcp {
            Some(server) => server.poll(),
            None => return false,
        };
        let any = !calls.is_empty();
        for call in calls {
            let result = self.mcp_tool(&call.name, &call.arguments);
            call.reply(result);
        }
        any
    }

    fn by_gnx(&self, gnx: &str) -> Result<Position, String> {
        let o = self.outline();
        if self.current.gnx(o) == gnx && o.position_exists(&self.current) {
            return Ok(self.current.clone());
        }
        o.all_unique_positions()
            .into_iter()
            .find(|p| p.gnx(o) == gnx)
            .ok_or_else(|| format!("no node with gnx {gnx}"))
    }

    /// The node a client with `access` is kept under, if any. A clone's
    /// positions share their subtree, so the first serves.
    fn scope_top(&self, access: Access) -> Option<Result<Position, String>> {
        let v = access.within?;
        let o = self.outline();
        Some(
            o.all_positions()
                .into_iter()
                .find(|q| q.v == v)
                .ok_or_else(|| "the node this client was kept under is gone".to_string()),
        )
    }

    /// The node `gnx`, under the scope's node if `access` has one.
    fn by_gnx_in(&self, access: Access, gnx: &str) -> Result<Position, String> {
        let Some(top) = self.scope_top(access) else {
            return self.by_gnx(gnx);
        };
        let o = self.outline();
        top?.self_and_subtree(o)
            .into_iter()
            .find(|p| p.gnx(o) == gnx)
            .ok_or_else(|| format!("no node with gnx {gnx} in this client's subtree"))
    }

    /// The nodes a client with `access` may change: those whose every
    /// position is under a position of its scope's node. A clone with a
    /// copy outside is not one: an edit to it would change that copy. None
    /// if `access` has no scope.
    fn editable(&self, access: Access) -> Option<std::collections::HashSet<VnodeId>> {
        let v = access.within?;
        let o = self.outline();
        let all = o.all_positions();
        let tops: Vec<&Position> = all.iter().filter(|q| q.v == v).collect();
        let under = |q: &Position| tops.iter().any(|t| *t == q || t.is_ancestor_of(o, q));
        let (inside, outside): (Vec<&Position>, Vec<&Position>) =
            all.iter().partition(|q| under(q));
        let outside: std::collections::HashSet<VnodeId> = outside.iter().map(|q| q.v).collect();
        Some(
            inside
                .iter()
                .map(|q| q.v)
                .filter(|v| !outside.contains(v))
                .collect(),
        )
    }

    /// Point `current` at the user's node again after a client's structural
    /// edit. A position is a path of child indexes, so an insert before it
    /// left it naming another node, and the user's next delete removed that.
    fn refind_current(&mut self) {
        let o = self.outline();
        if o.position_exists(&self.current) {
            return;
        }
        let v = self.current.v;
        match o.all_positions().into_iter().find(|q| q.v == v) {
            Some(q) => self.current = q,
            None => self.clamp_current(),
        }
    }

    /// May a client with `access` change the outline now?
    fn may_edit(&self, access: Access) -> Result<(), String> {
        if !access.edit {
            return Err(
                "editing is off: the user can allow it with mcp-edit in the settings".into(),
            );
        }
        if self.mode != Mode::Normal || self.buffer.is_some() {
            return Err("the user is typing; try again in a moment".into());
        }
        Ok(())
    }

    fn node_summary(&self, p: &Position) -> Value {
        let o = self.outline();
        json!({
            "gnx": p.gnx(o),
            "headline": p.h(o),
            "has_body": !p.b(o).is_empty(),
            "has_children": p.has_children(o),
        })
    }

    fn subtree(&self, p: &Position, depth: usize, budget: &mut usize) -> Value {
        let o = self.outline();
        let mut node = self.node_summary(p);
        *budget = budget.saturating_sub(1);
        if depth > 0 && *budget > 0 {
            let mut children = Vec::new();
            for c in p.children(o) {
                if *budget == 0 {
                    break;
                }
                children.push(self.subtree(&c, depth - 1, budget));
            }
            node["children"] = Value::Array(children);
        }
        node
    }

    /// Run one tool, as a client of the settings' server asked.
    pub fn mcp_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        self.mcp_tool_as(self.mcp_access, name, args)
    }

    /// Run one tool for a client allowed `access`, such as a plugin's own
    /// server.
    pub fn mcp_tool_as(
        &mut self,
        access: Access,
        name: &str,
        args: &Value,
    ) -> Result<Value, String> {
        match name {
            "outline" => {
                let depth = args["depth"].as_u64().unwrap_or(2) as usize;
                let mut budget = MOST_NODES;
                let o = self.outline();
                let tops: Vec<Position> = match (args["gnx"].as_str(), self.scope_top(access)) {
                    (Some(g), _) => vec![self.by_gnx_in(access, g)?],
                    (None, Some(top)) => vec![top?],
                    (None, None) => o
                        .root_position()
                        .map(|r| r.self_and_siblings(o))
                        .unwrap_or_default(),
                };
                let mut nodes = Vec::new();
                for p in &tops {
                    if budget == 0 {
                        break;
                    }
                    nodes.push(self.subtree(p, depth, &mut budget));
                }
                Ok(json!({"nodes": nodes, "truncated": budget == 0}))
            }
            "read_node" => {
                let p = self.by_gnx_in(access, string(args, "gnx")?)?;
                let top = self.scope_top(access).transpose()?;
                let o = self.outline();
                // Above the scope's node is outside it.
                let mut path: Vec<String> = p
                    .parents(o)
                    .iter()
                    .filter(|a| top.as_ref().is_none_or(|t| a.level() >= t.level()))
                    .map(|a| a.h(o).to_string())
                    .collect();
                path.reverse();
                let children: Vec<Value> =
                    p.children(o).iter().map(|c| self.node_summary(c)).collect();
                let mut out = json!({
                    "gnx": p.gnx(o),
                    "headline": p.h(o),
                    "body": p.b(o),
                    "path": path,
                    "children": children,
                    "clones": o.all_positions().iter().filter(|q| q.v == p.v).count(),
                });
                if p.is_any_at_file_node(o) {
                    out["file"] = json!(o.full_path(&p));
                }
                Ok(out)
            }
            "search" => {
                let pattern = string(args, "pattern")?;
                let smart = !pattern.chars().any(char::is_uppercase);
                let re = regex::RegexBuilder::new(pattern)
                    .case_insensitive(smart)
                    .build()
                    .map_err(|e| format!("not a regex: {e}"))?;
                let scope = args["scope"].as_str().unwrap_or("both");
                let limit = args["limit"].as_u64().unwrap_or(50) as usize;
                let nodes = match self.scope_top(access) {
                    Some(top) => {
                        let mut seen = std::collections::HashSet::new();
                        let o = self.outline();
                        top?.self_and_subtree(o)
                            .into_iter()
                            .filter(|p| seen.insert(p.v))
                            .collect()
                    }
                    None => self.outline().all_unique_positions(),
                };
                let o = self.outline();
                let mut hits = Vec::new();
                for p in nodes {
                    if hits.len() >= limit {
                        break;
                    }
                    if scope != "bodies" && re.is_match(p.h(o)) {
                        hits.push(json!({"gnx": p.gnx(o), "headline": p.h(o), "in": "headline"}));
                        continue;
                    }
                    if scope != "headlines" {
                        if let Some((row, line)) =
                            p.b(o).lines().enumerate().find(|(_, l)| re.is_match(l))
                        {
                            hits.push(json!({"gnx": p.gnx(o), "headline": p.h(o), "in": "body", "line": row + 1, "text": line}));
                        }
                    }
                }
                Ok(json!({ "matches": hits }))
            }
            "selection" => {
                let o = self.outline();
                let v = self.current.v;
                let shown = match self.scope_top(access) {
                    Some(top) => top?.self_and_subtree(o).iter().any(|q| q.v == v),
                    None => true,
                };
                let (gnx, headline) = match shown {
                    true => (json!(self.current.gnx(o)), json!(self.current.h(o))),
                    false => (Value::Null, Value::Null),
                };
                Ok(json!({
                    "gnx": gnx,
                    "headline": headline,
                    "pane": match self.focus { Focus::Tree => "outline", Focus::Body => "body" },
                    "cursor": {"line": self.editor.cursor.0 + 1, "column": self.editor.cursor.1 + 1},
                    "mode": self.mode.label(),
                }))
            }
            "set_headline" | "set_body" | "insert_node" | "delete_node" | "move_node"
            | "select_node" => {
                self.may_edit(access)?;
                let result = self.mcp_edit(access, name, args)?;
                self.check_rules(true);
                if self.message.starts_with("refused: ") {
                    return Err(self.message.clone());
                }
                self.message = format!("MCP: {name}");
                self.log_message();
                Ok(result)
            }
            "save" => {
                if !access.save {
                    return Err(
                        "saving is off: the user can allow it with mcp-save in the settings".into(),
                    );
                }
                self.may_edit(access)?;
                self.hold_new_files();
                self.run("save", 1);
                let waiting = self.mode == Mode::Confirm;
                let message = self.message.clone();
                self.log_message();
                Ok(json!({"message": message, "waiting_for_the_user": waiting}))
            }
            other => Err(format!("no such tool: {other}")),
        }
    }

    /// Make the next save ask before it creates any file the outline names.
    /// A client must not create files unasked: `@file
    /// ~/.ssh/authorized_keys` would pass `may_overwrite`, which guards only
    /// files that exist.
    pub fn hold_new_files(&mut self) {
        let o = self.outline();
        let new: Vec<String> = leolib::external::find_files_to_write(o, true)
            .0
            .iter()
            .map(|p| o.full_path(p))
            .filter(|path| !std::path::Path::new(path).exists())
            .collect();
        for path in &new {
            self.doc.outline_mut_untracked().hold_new_file(path);
        }
    }

    fn mcp_edit(&mut self, access: Access, name: &str, args: &Value) -> Result<Value, String> {
        let p = self.by_gnx_in(access, string(args, "gnx")?)?;
        let is_top = access.within == Some(p.v);
        let outside = "it would put a node outside this client's subtree";
        let editable = self.editable(access);
        if let Some(set) = &editable {
            // The nodes whose headline, body or children the edit changes.
            let o = self.outline();
            let parent = |q: &Position| q.parent(o).ok_or(outside);
            let changes: Vec<Position> = match name {
                "set_headline" | "set_body" => vec![p.clone()],
                "insert_node" if args["place"] == "inside" => vec![p.clone()],
                "insert_node" | "delete_node" if !is_top => vec![parent(&p)?],
                "move_node" => {
                    let target = self.by_gnx_in(access, string(args, "target")?)?;
                    let to = match args["place"] == "inside" {
                        true => target,
                        false => parent(&target)?,
                    };
                    vec![parent(&p)?, to]
                }
                _ => Vec::new(),
            };
            if let Some(q) = changes.iter().find(|q| !set.contains(&q.v)) {
                return Err(format!(
                    "node {} has a clone outside this client's subtree, which the edit would change too",
                    q.gnx(o)
                ));
            }
        }
        match name {
            "set_headline" => {
                let headline = string(args, "headline")?;
                match self.doc.rename_block(&p, headline)? {
                    // A wiki rename rewrites references in other pages.
                    Some(plan)
                        if editable.as_ref().is_some_and(|set| {
                            plan.edits.iter().any(|(q, ..)| !set.contains(&q.v))
                        }) =>
                    {
                        self.doc.undo();
                        self.refind_current();
                        return Err(
                            "the rename would change nodes outside this client's subtree".into(),
                        );
                    }
                    // The kind's warning, such as links a wiki rename made ambiguous.
                    Some(plan) => return Ok(json!({ "ok": true, "note": plan.note })),
                    None => self.doc.set_headline(&p, headline),
                }
            }
            "set_body" => self.doc.set_body(&p, string(args, "body")?),
            "select_node" => {
                self.select(p);
                self.focus = Focus::Tree;
            }
            "delete_node" if is_top => {
                return Err("the node this client is kept under cannot be deleted".into());
            }
            "delete_node" => {
                let current = self.current.clone();
                let next = self
                    .doc
                    .delete_node(&p)
                    .ok_or("the last node cannot be deleted")?;
                // The user's selection stays, unless it was what went.
                match current == p || p.is_ancestor_of(self.outline(), &current) {
                    true => self.select(next),
                    false => self.refind_current(),
                }
            }
            "insert_node" => {
                let place = place(args)?;
                if is_top && place != Place::Inside {
                    return Err(outside.into());
                }
                self.doc.begin_group("mcp-insert-node");
                let Some(new) = self.doc.insert_node(&p) else {
                    self.doc.end_group();
                    return Err("the node moved; read the outline again".into());
                };
                let new = self.doc.move_node(&new, &p, place).unwrap_or(new);
                self.doc.set_headline(&new, string(args, "headline")?);
                if let Some(body) = args["body"].as_str() {
                    self.doc.set_body(&new, body);
                }
                self.doc.end_group();
                self.refind_current();
                return Ok(json!({ "gnx": new.gnx(self.outline()) }));
            }
            "move_node" => {
                let target = self.by_gnx_in(access, string(args, "target")?)?;
                let place = place(args)?;
                if access.within == Some(target.v) && place != Place::Inside {
                    return Err(outside.into());
                }
                let moved = self
                    .doc
                    .move_node(&p, &target, place)
                    .ok_or("a node cannot move into its own tree")?;
                self.refind_current();
                return Ok(json!({ "gnx": moved.gnx(self.outline()) }));
            }
            _ => unreachable!("listed above"),
        }
        Ok(json!({ "ok": true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut doc = Document::new_empty("");
        let a = doc.outline().root_position().unwrap();
        doc.set_headline(&a, "alpha");
        doc.set_body(&a, "first line\nneedle here\n");
        let b = doc.outline_mut_untracked().insert_after(&a);
        doc.set_headline(&b, "beta");
        doc.clear_undo();
        App::new(doc)
    }

    fn gnx_of(app: &App, head: &str) -> String {
        let o = app.outline();
        let p = o
            .all_unique_positions()
            .into_iter()
            .find(|p| p.h(o) == head)
            .unwrap();
        p.gnx(o).to_string()
    }

    #[test]
    fn a_client_kept_under_a_node_may_not_change_a_clone_with_a_copy_outside() {
        let mut app = app();
        let alpha = app.outline().root_position().unwrap();
        let beta = app.by_gnx(&gnx_of(&app, "beta")).unwrap();
        let o = app.doc.outline_mut_untracked();
        let inner = o.insert_as_last_child(&alpha);
        o.set_headline(&inner, "inner");
        let shared = o.insert_as_last_child(&alpha);
        o.set_headline(&shared, "shared");
        let deep = o.insert_as_last_child(&shared);
        o.set_headline(&deep, "deep");
        let copy = o.clone_node(&shared);
        app.doc.move_node(&copy, &beta, Place::Inside).unwrap();
        app.doc.clear_undo();
        let access = Access {
            edit: true,
            save: false,
            within: Some(alpha.v),
        };
        let (a, i, s, d) = (
            gnx_of(&app, "alpha"),
            gnx_of(&app, "inner"),
            gnx_of(&app, "shared"),
            gnx_of(&app, "deep"),
        );
        let call = |app: &mut App, name: &str, args: Value| app.mcp_tool_as(access, name, &args);
        // Reading a clone is reading the subtree.
        assert!(call(&mut app, "read_node", json!({ "gnx": d })).is_ok());
        for (name, args) in [
            ("set_body", json!({"gnx": s, "body": "x\n"})),
            ("set_headline", json!({"gnx": d, "headline": "x"})),
            (
                "insert_node",
                json!({"gnx": s, "place": "inside", "headline": "x"}),
            ),
            (
                "insert_node",
                json!({"gnx": d, "place": "after", "headline": "x"}),
            ),
            (
                "move_node",
                json!({"gnx": i, "target": s, "place": "inside"}),
            ),
            (
                "move_node",
                json!({"gnx": d, "target": i, "place": "inside"}),
            ),
            ("delete_node", json!({ "gnx": d })),
        ] {
            let err = call(&mut app, name, args.clone()).unwrap_err();
            assert!(err.contains("clone outside"), "{name} {args}: {err}");
        }
        assert!(!app.doc.undoer().can_undo(), "nothing was changed");
        // Unlinking this copy leaves the other: alpha's children change, not shared.
        assert!(call(&mut app, "delete_node", json!({ "gnx": s })).is_ok());
        assert!(call(&mut app, "read_node", json!({ "gnx": s })).is_err());
        assert_eq!(app.by_gnx(&s).unwrap().h(app.outline()), "shared");
        app.doc.undo();
        // With both copies under alpha, the clone may change.
        let copy = app
            .by_gnx(&gnx_of(&app, "beta"))
            .unwrap()
            .children(app.outline())[0]
            .clone();
        let alpha = app.by_gnx(&a).unwrap();
        app.doc.move_node(&copy, &alpha, Place::Inside).unwrap();
        assert!(call(&mut app, "set_body", json!({"gnx": s, "body": "x\n"})).is_ok());
    }

    #[test]
    fn a_client_kept_under_a_node_reaches_only_its_subtree() {
        let mut app = app();
        let alpha = app.outline().root_position().unwrap();
        let inner = app.doc.outline_mut_untracked().insert_as_last_child(&alpha);
        app.doc.set_headline(&inner, "inner");
        let access = Access {
            edit: true,
            save: false,
            within: Some(alpha.v),
        };
        let (a, i, b) = (
            gnx_of(&app, "alpha"),
            gnx_of(&app, "inner"),
            gnx_of(&app, "beta"),
        );
        let call = |app: &mut App, name: &str, args: Value| app.mcp_tool_as(access, name, &args);
        let tree = call(&mut app, "outline", json!({})).unwrap();
        assert_eq!(tree["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(tree["nodes"][0]["headline"], "alpha");
        assert!(call(&mut app, "read_node", json!({ "gnx": b })).is_err());
        let node = call(&mut app, "read_node", json!({ "gnx": i })).unwrap();
        assert_eq!(node["path"], json!(["alpha"]));
        let found = call(&mut app, "search", json!({"pattern": "beta|inner"})).unwrap();
        assert_eq!(found["matches"].as_array().unwrap().len(), 1);
        let beta = app.by_gnx(&b).unwrap();
        app.select(beta);
        let sel = call(&mut app, "selection", json!({})).unwrap();
        assert!(sel["gnx"].is_null() && sel["headline"].is_null(), "{sel}");
        assert!(call(&mut app, "set_body", json!({"gnx": i, "body": "x\n"})).is_ok());
        assert!(call(&mut app, "set_body", json!({"gnx": b, "body": "x\n"})).is_err());
        for (name, args) in [
            (
                "insert_node",
                json!({"gnx": a, "place": "after", "headline": "out"}),
            ),
            (
                "move_node",
                json!({"gnx": i, "target": a, "place": "before"}),
            ),
            (
                "move_node",
                json!({"gnx": i, "target": b, "place": "inside"}),
            ),
            ("delete_node", json!({ "gnx": a })),
        ] {
            assert!(call(&mut app, name, args.clone()).is_err(), "{name} {args}");
        }
        let new = call(
            &mut app,
            "insert_node",
            json!({"gnx": a, "place": "inside", "headline": "in"}),
        );
        assert!(new.is_ok());
        let heads: Vec<String> = {
            let o = app.outline();
            o.all_positions()
                .iter()
                .map(|p| p.h(o).to_string())
                .collect()
        };
        assert_eq!(heads, ["alpha", "inner", "in", "beta"]);
    }

    #[test]
    fn a_client_reads_the_outline_nodes_and_search_results() {
        let mut app = app();
        let tree = app.mcp_tool("outline", &json!({})).unwrap();
        let heads: Vec<&str> = tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["headline"].as_str().unwrap())
            .collect();
        assert_eq!(heads, ["alpha", "beta"]);
        let gnx = gnx_of(&app, "alpha");
        let node = app.mcp_tool("read_node", &json!({ "gnx": gnx })).unwrap();
        assert_eq!(node["body"], "first line\nneedle here\n");
        let found = app
            .mcp_tool("search", &json!({"pattern": "NEEDLE|beta"}))
            .unwrap();
        assert_eq!(
            found["matches"].as_array().unwrap().len(),
            1,
            "a capital makes it case-sensitive"
        );
        let found = app
            .mcp_tool("search", &json!({"pattern": "needle|beta"}))
            .unwrap();
        let m = found["matches"].as_array().unwrap();
        assert_eq!((m.len(), m[0]["line"].as_u64()), (2, Some(2)));
        assert!(app.mcp_tool("read_node", &json!({"gnx": "nope"})).is_err());
    }

    #[test]
    fn editing_needs_the_setting_and_undoes_as_one_step() {
        let mut app = app();
        let gnx = gnx_of(&app, "beta");
        let err = app
            .mcp_tool("set_body", &json!({"gnx": gnx, "body": "x"}))
            .unwrap_err();
        assert!(err.contains("mcp-edit"));
        app.mcp_access = Access {
            edit: true,
            save: false,
            within: None,
        };
        let new = app
            .mcp_tool(
                "insert_node",
                &json!({"gnx": gnx, "place": "inside", "headline": "child", "body": "text\n"}),
            )
            .unwrap();
        let child = app.by_gnx(new["gnx"].as_str().unwrap()).unwrap();
        assert_eq!((child.h(app.outline()), child.level()), ("child", 1));
        assert!(app.message.contains("insert_node"));
        app.doc.undo();
        assert!(app.by_gnx(new["gnx"].as_str().unwrap()).is_err());
        let err = app.mcp_tool("save", &json!({})).unwrap_err();
        assert!(err.contains("mcp-save"));
    }

    #[test]
    fn a_save_writes_the_outline_and_asks_before_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let leo = dir.path().join("s.leo").to_string_lossy().to_string();
        let mut doc = Document::new_empty(&leo);
        let root = doc.outline().root_position().unwrap();
        doc.set_headline(&root, "@file a.py");
        doc.set_body(&root, "x = 1\n");
        let mut app = App::new(doc);
        app.mcp_access = Access {
            edit: true,
            save: true,
            within: None,
        };
        let py = dir.path().join("a.py");
        let reply = app.mcp_tool("save", &json!({})).unwrap();
        // The .leo file is saved; the new file waits for the user.
        assert_eq!(reply["waiting_for_the_user"], true, "{reply}");
        assert!(std::fs::read_to_string(&leo)
            .unwrap()
            .contains("@file a.py"));
        assert!(!py.exists());
        let label = app.mini_label();
        assert!(
            label.contains("create") && label.contains(&*py.to_string_lossy()),
            "{label}"
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(std::fs::read_to_string(&py).unwrap().contains("x = 1\n"));
        assert!(!app.outline().changed);

        // A file the user approved is the outline's now: no question.
        let gnx = app.outline().root_position().unwrap().gnx(app.outline());
        app.mcp_tool("set_body", &json!({"gnx": gnx, "body": "x = 2\n"}))
            .unwrap();
        let reply = app.mcp_tool("save", &json!({})).unwrap();
        assert_eq!(reply["waiting_for_the_user"], false, "{reply}");
        assert!(std::fs::read_to_string(&py).unwrap().contains("x = 2\n"));
    }

    #[test]
    fn a_declined_file_stays_held_for_the_users_own_save() {
        let dir = tempfile::tempdir().unwrap();
        let leo = dir.path().join("s.leo").to_string_lossy().to_string();
        let mut doc = Document::new_empty(&leo);
        let root = doc.outline().root_position().unwrap();
        doc.set_headline(&root, "@file b.py");
        doc.set_body(&root, "y = 1\n");
        let mut app = App::new(doc);
        app.mcp_access = Access {
            edit: true,
            save: true,
            within: None,
        };
        let py = dir.path().join("b.py");
        app.mcp_tool("save", &json!({})).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(!py.exists());
        // Ctrl-S asks again rather than writing what the client named.
        app.save();
        assert_eq!(app.mode, Mode::Confirm);
        assert!(!py.exists());
    }

    #[test]
    fn an_edit_waits_while_the_user_types() {
        let mut app = app();
        app.mcp_access = Access {
            edit: true,
            save: true,
            within: None,
        };
        app.mode = Mode::Insert;
        app.buffer = Some(vec!["x\n".into()]);
        let gnx = gnx_of(&app, "alpha");
        let err = app
            .mcp_tool("set_headline", &json!({"gnx": gnx, "headline": "y"}))
            .unwrap_err();
        assert!(err.contains("typing"));
        // Reading is never refused.
        assert!(app.mcp_tool("selection", &json!({})).is_ok());
    }

    /// A kind whose every rename comes with a note.
    struct Noting;

    impl leolib::ext::FileKind for Noting {
        fn directive(&self) -> &'static str {
            "@noting"
        }
        fn read(&self, _o: &mut leolib::Outline, _p: &leolib::Position) -> leolib::Result<bool> {
            Ok(false)
        }
        fn write(&self, o: &leolib::Outline, p: &leolib::Position) -> leolib::Result<String> {
            Ok(p.b(o).to_string())
        }
        fn plan_rename(
            &self,
            o: &leolib::Outline,
            p: &leolib::Position,
            headline: &str,
        ) -> Option<Result<leolib::ext::Rename, String>> {
            Some(Ok(leolib::ext::Rename {
                old: p.h(o).to_string(),
                new: headline.to_string(),
                fences: 0,
                references: 0,
                note: Some("[[x]] now names several pages".into()),
                edits: vec![(p.clone(), Some(headline.to_string()), None)],
            }))
        }
    }

    #[test]
    fn a_rename_returns_its_note() {
        let mut app = app();
        app.mcp_access = Access {
            edit: true,
            save: false,
            within: None,
        };
        let kinds = leolib::ext::Kinds::empty().with(Noting).unwrap();
        (app.doc.outline_mut_untracked()).set_kinds(std::sync::Arc::new(kinds));
        let gnx = gnx_of(&app, "alpha");
        app.mcp_tool(
            "set_headline",
            &json!({"gnx": gnx, "headline": "@noting a"}),
        )
        .unwrap();
        let reply = app
            .mcp_tool(
                "set_headline",
                &json!({"gnx": gnx, "headline": "@noting b"}),
            )
            .unwrap();
        assert_eq!(reply["note"], "[[x]] now names several pages");
        assert_eq!(gnx_of(&app, "@noting b"), gnx);
    }

    /// A kind whose rename also rewrites the next node's body, as a wiki
    /// rename rewrites links in other pages.
    struct Reaching;

    impl leolib::ext::FileKind for Reaching {
        fn directive(&self) -> &'static str {
            "@reaching"
        }
        fn read(&self, _o: &mut leolib::Outline, _p: &leolib::Position) -> leolib::Result<bool> {
            Ok(false)
        }
        fn write(&self, o: &leolib::Outline, p: &leolib::Position) -> leolib::Result<String> {
            Ok(p.b(o).to_string())
        }
        fn plan_rename(
            &self,
            o: &leolib::Outline,
            p: &leolib::Position,
            headline: &str,
        ) -> Option<Result<leolib::ext::Rename, String>> {
            let next = p.next(o)?;
            Some(Ok(leolib::ext::Rename {
                old: p.h(o).to_string(),
                new: headline.to_string(),
                fences: 0,
                references: 1,
                note: None,
                edits: vec![
                    (p.clone(), Some(headline.to_string()), None),
                    (next, None, Some("[[b]]\n".into())),
                ],
            }))
        }
    }

    #[test]
    fn a_rename_that_reaches_outside_the_subtree_is_refused_and_undone() {
        let mut app = app();
        let kinds = leolib::ext::Kinds::empty().with(Reaching).unwrap();
        (app.doc.outline_mut_untracked()).set_kinds(std::sync::Arc::new(kinds));
        let alpha = app.outline().root_position().unwrap();
        app.doc.set_headline(&alpha, "@reaching a");
        app.doc.clear_undo();
        let access = Access {
            edit: true,
            save: false,
            within: Some(alpha.v),
        };
        let gnx = gnx_of(&app, "@reaching a");
        let err = app
            .mcp_tool_as(
                access,
                "set_headline",
                &json!({"gnx": gnx, "headline": "@reaching b"}),
            )
            .unwrap_err();
        assert!(err.contains("outside"), "{err}");
        assert_eq!(gnx_of(&app, "@reaching a"), gnx);
        assert_eq!(
            app.by_gnx(&gnx_of(&app, "beta")).unwrap().b(app.outline()),
            ""
        );
        assert!(!app.doc.undoer().can_undo());
    }

    #[test]
    fn a_move_into_its_own_tree_is_refused() {
        let mut app = app();
        app.mcp_access = Access {
            edit: true,
            save: false,
            within: None,
        };
        let (a, b) = (gnx_of(&app, "alpha"), gnx_of(&app, "beta"));
        app.mcp_tool(
            "move_node",
            &json!({"gnx": b, "target": a, "place": "inside"}),
        )
        .unwrap();
        let err = app
            .mcp_tool(
                "move_node",
                &json!({"gnx": a, "target": b, "place": "inside"}),
            )
            .unwrap_err();
        assert!(err.contains("own tree"));
    }

    #[test]
    fn the_server_starts_with_a_token_and_stops() {
        let mut app = app();
        let settings = McpSettings {
            enabled: true,
            port: 0,
            token: Some("t".into()),
            ..McpSettings::default()
        };
        app.set_mcp(&settings).unwrap();
        assert!(app.mcp.is_some());
        app.set_mcp(&McpSettings::default()).unwrap();
        assert!(app.mcp.is_none());
        let no_token = McpSettings {
            enabled: true,
            port: 0,
            ..McpSettings::default()
        };
        assert!(app.set_mcp(&no_token).is_err());
    }

    /// The status a ping with `token` gets from 127.0.0.1:`port`.
    fn ping(port: u16, token: &str) -> String {
        use std::io::{BufRead, Write};
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut status = String::new();
        std::io::BufReader::new(s).read_line(&mut status).unwrap();
        status
    }

    /// `set_mcp` returned early on an unchanged port, so the old token
    /// stayed valid until a restart.
    #[test]
    fn a_changed_token_takes_effect_on_the_same_port() {
        let mut app = app();
        let mut settings = McpSettings {
            enabled: true,
            port: 0,
            token: Some("old".into()),
            ..McpSettings::default()
        };
        app.set_mcp(&settings).unwrap();
        settings.port = app.mcp.as_ref().unwrap().port();
        settings.token = Some("new".into());
        app.set_mcp(&settings).unwrap();
        assert_eq!(app.mcp.as_ref().unwrap().port(), settings.port);
        assert!(ping(settings.port, "old").contains("401"));
        assert!(ping(settings.port, "new").contains("200"));
    }

    /// An insert before the user's node left `current` naming the new node,
    /// so the user's next delete removed it instead of their own.
    #[test]
    fn a_client_insert_or_move_keeps_the_users_selection() {
        let mut app = app();
        app.mcp_access = Access {
            edit: true,
            save: false,
            within: None,
        };
        let alpha = app.current.gnx(app.outline()).to_string();
        let beta = app.rows()[1].position.gnx(app.outline()).to_string();
        app.mcp_tool(
            "insert_node",
            &json!({"gnx": alpha, "place": "before", "headline": "new"}),
        )
        .unwrap();
        assert_eq!(app.current.h(app.outline()), "alpha");
        assert!(app.outline().position_exists(&app.current));
        app.mcp_tool(
            "move_node",
            &json!({"gnx": beta, "target": alpha, "place": "before"}),
        )
        .unwrap();
        assert_eq!(app.current.h(app.outline()), "alpha");
        assert!(app.outline().position_exists(&app.current));
    }
}
