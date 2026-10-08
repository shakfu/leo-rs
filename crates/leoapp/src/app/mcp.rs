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

use leolib::{Place, Position};
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
            description: "Change a node's headline. Needs mcp-edit.",
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
            description: "Save the .leo file and every changed external file, as Ctrl-S does; a file that would be overwritten waits for the user. Needs mcp-save.",
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
        };
        let want = settings.enabled.then_some(settings.port);
        if self.mcp.as_ref().map(|s| s.port()) == want && self.mcp.is_some() {
            return Ok(());
        }
        self.mcp = None;
        let Some(port) = want else { return Ok(()) };
        let token = settings.token.clone().unwrap_or_default();
        if token.is_empty() {
            return Err("MCP needs a token; the settings make one".into());
        }
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
        if self.current.gnx(o) == gnx {
            return Ok(self.current.clone());
        }
        o.all_unique_positions()
            .into_iter()
            .find(|p| p.gnx(o) == gnx)
            .ok_or_else(|| format!("no node with gnx {gnx}"))
    }

    /// May a client change the outline now?
    fn may_edit(&self) -> Result<(), String> {
        if !self.mcp_access.edit {
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

    /// Run one tool, as a client asked.
    pub fn mcp_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        match name {
            "outline" => {
                let depth = args["depth"].as_u64().unwrap_or(2) as usize;
                let mut budget = MOST_NODES;
                let o = self.outline();
                let tops: Vec<Position> = match args["gnx"].as_str() {
                    Some(g) => vec![self.by_gnx(g)?],
                    None => o
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
                let p = self.by_gnx(string(args, "gnx")?)?;
                let o = self.outline();
                let mut path: Vec<String> =
                    p.parents(o).iter().map(|a| a.h(o).to_string()).collect();
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
                let o = self.outline();
                let mut hits = Vec::new();
                for p in o.all_unique_positions() {
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
                Ok(json!({
                    "gnx": self.current.gnx(o),
                    "headline": self.current.h(o),
                    "pane": match self.focus { Focus::Tree => "outline", Focus::Body => "body" },
                    "cursor": {"line": self.editor.cursor.0 + 1, "column": self.editor.cursor.1 + 1},
                    "mode": self.mode.label(),
                }))
            }
            "set_headline" | "set_body" | "insert_node" | "delete_node" | "move_node"
            | "select_node" => {
                self.may_edit()?;
                let result = self.mcp_edit(name, args)?;
                self.message = format!("MCP: {name}");
                self.log_message();
                Ok(result)
            }
            "save" => {
                if !self.mcp_access.save {
                    return Err(
                        "saving is off: the user can allow it with mcp-save in the settings".into(),
                    );
                }
                self.may_edit()?;
                self.run("save", 1);
                let waiting = self.mode == Mode::Confirm;
                let message = self.message.clone();
                self.log_message();
                Ok(json!({"message": message, "waiting_for_the_user": waiting}))
            }
            other => Err(format!("no such tool: {other}")),
        }
    }

    fn mcp_edit(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let p = self.by_gnx(string(args, "gnx")?)?;
        match name {
            "set_headline" => {
                let headline = string(args, "headline")?;
                if self.doc.rename_block(&p, headline)?.is_none() {
                    self.doc.set_headline(&p, headline);
                }
            }
            "set_body" => self.doc.set_body(&p, string(args, "body")?),
            "select_node" => {
                self.select(p);
                self.focus = Focus::Tree;
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
                    false => {
                        if let Some(q) = self
                            .outline()
                            .all_positions()
                            .into_iter()
                            .find(|q| q.v == current.v)
                        {
                            self.current = q;
                        }
                    }
                }
            }
            "insert_node" => {
                let place = place(args)?;
                self.doc.begin_group("mcp-insert-node");
                let new = self.doc.insert_node(&p);
                let new = self.doc.move_node(&new, &p, place).unwrap_or(new);
                self.doc.set_headline(&new, string(args, "headline")?);
                if let Some(body) = args["body"].as_str() {
                    self.doc.set_body(&new, body);
                }
                self.doc.end_group();
                return Ok(json!({ "gnx": new.gnx(self.outline()) }));
            }
            "move_node" => {
                let target = self.by_gnx(string(args, "target")?)?;
                let moved = self
                    .doc
                    .move_node(&p, &target, place(args)?)
                    .ok_or("a node cannot move into its own tree")?;
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
    fn an_edit_waits_while_the_user_types() {
        let mut app = app();
        app.mcp_access = Access {
            edit: true,
            save: true,
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

    #[test]
    fn a_move_into_its_own_tree_is_refused() {
        let mut app = app();
        app.mcp_access = Access {
            edit: true,
            save: false,
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
}
