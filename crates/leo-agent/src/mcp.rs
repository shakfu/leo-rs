//! A run's own MCP server, for an agent whose command names `{mcp}`.
//!
//! The server listens on a free port of 127.0.0.1 for the run alone, with a
//! token made for it, and stops when the run ends. The agent may read and
//! edit the outline, never save it. The settings' `mcp` server is not used:
//! it would need `mcp = true`, and its token outlives the run.
//!
//! The config file holds the token, so it is the user's alone, and the
//! command line, which other users can list, holds only its path.

use std::path::PathBuf;

use serde_json::{json, Value};

use leoapp::app::{Access, App};
use leolib::{Outline, Position, VnodeId};

use crate::{Scope, State};

/// Tools the settings' server offers that a run does not: `select_node`
/// moves the user's selection mid-run, and a run may never save.
const NOT_OFFERED: [&str; 2] = ["select_node", "save"];

/// The outline's tools, as a run offers them: without [`NOT_OFFERED`], and
/// without the settings each needs, which a run does not read.
fn tools() -> Vec<leomcp::Tool> {
    leoapp::app::mcp_tools()
        .into_iter()
        .filter(|t| !NOT_OFFERED.contains(&t.name))
        .map(|mut t| {
            t.description = t
                .description
                .strip_suffix(" Needs mcp-edit.")
                .unwrap_or(t.description);
            t
        })
        .collect()
}

/// The undo step a run's edits make.
const UNDO_NAME: &str = "agent-run";

/// A run's server and its config file, removed when dropped.
pub(crate) struct Served {
    server: leomcp::Server,
    pub(crate) config: PathBuf,
    /// The node whose subtree the tools reach; None for the whole outline.
    within: Option<VnodeId>,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.config);
    }
}

/// Start a server whose tools reach `within`'s subtree, and write the
/// config file that names it.
pub(crate) fn serve(within: Option<VnodeId>) -> std::io::Result<Served> {
    let token = leomcp::new_token();
    let server = leomcp::Server::start(0, &token, tools(), "leo")?;
    let config = json!({"mcpServers": {"leo": {
        "type": "http",
        "url": format!("http://127.0.0.1:{}/mcp", server.port()),
        "headers": {"Authorization": format!("Bearer {token}")},
    }}});
    // A name no other process can guess, created only if absent, so a
    // planted file or link is not written through.
    let path = std::env::temp_dir().join(format!("leo-agent-{}.json", leomcp::new_token()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path)?;
    let served = Served {
        server,
        config: path,
        within,
    };
    std::io::Write::write_all(&mut file, config.to_string().as_bytes())?;
    Ok(served)
}

/// What a run's tools reach, from its scope: the scope's subtree, as the
/// text sent is.
pub(crate) struct Reach {
    pub(crate) within: Option<VnodeId>,
    /// Nodes in the subtree.
    pub(crate) nodes: usize,
    /// What the prompt adds, so the agent knows where it is.
    pub(crate) note: String,
}

impl Reach {
    pub(crate) fn new(o: &Outline, p: &Position, scope: Scope) -> Reach {
        let top = crate::scope_node(o, p, scope);
        let mut note = format!("\nThe `leo` MCP tools read and edit this outline; they name nodes by gnx. This prompt is from the node with gnx {}.", p.gnx(o));
        match &top {
            Some(t) if t == p => note.push_str(" The tools reach only that node and its subtree."),
            Some(t) => note.push_str(&format!(
                " The tools reach only the node with gnx {} and its subtree.",
                t.gnx(o)
            )),
            None => {}
        }
        note.push('\n');
        Reach {
            within: top.as_ref().map(|t| t.v),
            nodes: top.map_or(0, |t| t.self_and_subtree(o).len()),
            note,
        }
    }
}

/// Answer the run's waiting tool calls. True if any came.
pub(crate) fn answer(app: &mut App) -> bool {
    let (calls, within) = match app
        .plugin_data_ref::<State>()
        .and_then(|s| s.job.as_ref())
        .and_then(|j| j.mcp.as_ref())
    {
        Some(served) => (served.server.poll(), served.within),
        None => return false,
    };
    let any = !calls.is_empty();
    for call in calls {
        let result = run_call(app, within, &call.name, &call.arguments);
        call.reply(result);
    }
    any
}

fn run_call(
    app: &mut App,
    within: Option<VnodeId>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    // The server lists only the run's tools; this holds if that changes.
    if NOT_OFFERED.contains(&name) {
        return Err(format!("{name} is not offered to an agent's run"));
    }
    let access = Access {
        edit: true,
        save: false,
        within,
    };
    let before = app.doc.undoer().version();
    let result = app.mcp_tool_as(access, name, args);
    if result.is_ok() && app.doc.undoer().version() != before {
        // A save asks before it creates a file an edit named, as the
        // `save` tool does.
        app.hold_new_files();
        join(app, before);
    }
    result
}

/// Join the undo step made since `before` to the run's last one, if
/// nothing else changed the history between them: the user's edit or undo
/// keeps the steps apart.
pub(crate) fn join(app: &mut App, before: u64) {
    let after = app.doc.undoer().version();
    let state = app.plugin_data::<State>();
    let last = state.last_edit;
    if after != before + 1 {
        // Nothing recorded, or more than one step: a refusal undid it.
        return;
    }
    state.last_edit = Some(after);
    if last == Some(before) {
        app.doc.join_undo(UNDO_NAME);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_run_offers_neither_select_node_nor_save_nor_the_settings() {
        let tools = super::tools();
        assert_eq!(tools.len(), 9);
        for t in &tools {
            assert!(!super::NOT_OFFERED.contains(&t.name), "{}", t.name);
            assert!(
                !t.description.contains("mcp-"),
                "{}: {}",
                t.name,
                t.description
            );
        }
    }
}
