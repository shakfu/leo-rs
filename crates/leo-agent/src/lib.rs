//! Agents in leo-rs, as a leoapp plugin. The design is in
//! `docs/dev/agents.md`; this crate holds its first command:
//!
//! `:ask [scope=N|all] [agent=NAME] [provider=NAME] [model=NAME] [reply=replace] PROMPT`
//! sends the prompt, and the outline text the scope names, to an agent's
//! stdin, and inserts its stdout below the cursor's line, as vim's `:r !cmd`.
//! From VISUAL, the selected lines go with the prompt, and the reply goes
//! below them, or in their place with `reply=replace`, as `:'<,'>!cmd`.
//!
//! `:agent-run` runs the selected node's headline, `@agent [params] PROMPT`,
//! and writes the reply into its body or a new child (`node.rs`).
//!
//! The agent runs as a child process, read on threads of its own, so a slow
//! reply does not stop the window drawing; `poll` collects it.
//!
//! An agent whose command names `{mcp}` gets an MCP server of its own for
//! the run, with the outline's tools, so it may read and edit the outline
//! (`mcp.rs`).

pub mod agents;
mod mcp;
mod node;

use std::io::{Read, Write};
use std::ops::Range;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use leoapp::app::{App, Focus, Mode};
use leoapp::commands::Command as AppCommand;
use leoapp::editor::change::Register;
use leoapp::plugins::AppPlugin;
use leolib::{Outline, Position, VnodeId};

pub use agents::{Agents, Choice, Invocation};

/// The plugin.
pub struct AgentPlugin;

impl AppPlugin for AgentPlugin {
    fn name(&self) -> &'static str {
        "agent"
    }
    fn commands(&self) -> &'static [AppCommand] {
        &COMMANDS
    }
    fn with_argument(&self, name: &str) -> Option<fn(&mut App, &str)> {
        match name {
            "ask" => Some(ask),
            _ => None,
        }
    }
    fn poll(&self, app: &mut App) -> bool {
        poll(app)
    }
    fn activate(&self, app: &mut App, p: &Position) -> bool {
        // The double-click selected `p` first; `:agent-run` runs the selection.
        if !node::is_agent_node(app.outline(), p) || app.current != *p {
            return false;
        }
        node::run(app, false);
        true
    }
    fn open_url(&self, app: &mut App) -> bool {
        // Enter or `gd` on a body's `@agent` line runs it.
        let row = app.editor.cursor.0;
        let o = app.outline();
        let on_line = app
            .current
            .b(o)
            .split('\n')
            .nth(row)
            .is_some_and(node::is_agent_line);
        if on_line {
            node::run(app, false);
        }
        on_line
    }
    fn links(&self, _o: &Outline, _p: &Position, lines: &[String]) -> Vec<(usize, Range<usize>)> {
        lines
            .iter()
            .enumerate()
            .filter(|(_, l)| node::is_agent_line(l))
            .map(|(i, l)| (i, 0..l.len()))
            .collect()
    }
    fn poll_after(&self, app: &App) -> Option<Duration> {
        busy(app).then_some(POLL)
    }
}

static COMMANDS: [AppCommand; 3] = [
    AppCommand {
        name: "ask",
        summary: "send a prompt to an agent and insert the reply below the cursor: :ask PROMPT",
        run: |app, _| app.message = USAGE.to_string(),
    },
    AppCommand {
        name: "agent-run",
        summary: "run the selected @agent node's prompt and write the reply into it",
        run: |app, _| node::run(app, false),
    },
    AppCommand {
        name: "agent-cancel",
        summary: "stop the agent's run",
        run: |app, _| cancel(app),
    },
];

const USAGE: &str = "usage: :ask [scope=0|N|all] [lines=all] [agent=NAME] [provider=NAME] [model=NAME] [reply=insert|replace] [--] PROMPT";

/// How soon a front end should look again while a run is going.
const POLL: Duration = Duration::from_millis(50);

/// The most bytes a run sends: `scope=all` on a large outline would send
/// megabytes.
pub const MAX_PROMPT_BYTES: usize = 256 * 1024;

/// The plugin's state in the app.
#[derive(Default)]
struct State {
    /// The table, read from `agents.toml`, and read again when the file
    /// changes.
    agents: Option<Agents>,
    /// The file's modification time and length when it was read; None if
    /// it was missing.
    stamp: Option<(std::time::SystemTime, u64)>,
    /// Read this file in place of `~/.config/leo-rs/agents.toml`.
    file: Option<std::path::PathBuf>,
    /// A table [`set_agents`] gave, never read again.
    fixed: bool,
    job: Option<Job>,
    /// A finished reply, waiting for NORMAL.
    reply: Option<(Target, String)>,
    /// The outlines a run was confirmed in, by file name, this session.
    trusted: std::collections::HashSet<String>,
    /// A hash of each `@agent` body as a run last wrote it, so `replace`
    /// refuses to overwrite an edit.
    written: std::collections::HashMap<VnodeId, u64>,
    /// The undo version after the run's last edit over MCP, so the next
    /// one, or the reply, joins it as one undo step.
    last_edit: Option<u64>,
}

/// Use `agents` in place of `agents.toml`, as a test does.
pub fn set_agents(app: &mut App, agents: Agents) {
    let state = app.plugin_data::<State>();
    state.agents = Some(agents);
    state.fixed = true;
}

/// Read `path` in place of `~/.config/leo-rs/agents.toml`, as a test does.
pub fn use_agents_file(app: &mut App, path: std::path::PathBuf) {
    let state = app.plugin_data::<State>();
    *state = State {
        file: Some(path),
        ..State::default()
    };
}

/// The table, read again if the file changed since it was last read.
fn table(app: &mut App) -> Agents {
    let state = app.plugin_data::<State>();
    let path = match state.fixed {
        true => None,
        false => state.file.clone().or_else(agents::default_path),
    };
    let stamp = path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| Some((m.modified().ok()?, m.len())));
    let stale = state.agents.is_none() || (!state.fixed && stamp != state.stamp);
    if stale {
        let again = state.agents.is_some();
        let (agents, warnings) = match &path {
            Some(p) => Agents::load(p),
            None => (Agents::default(), Vec::new()),
        };
        state.agents = Some(agents);
        state.stamp = stamp;
        if again {
            if let Some(p) = &path {
                app.log(format!("ask: read {} again", p.display()));
            }
        }
        for w in warnings {
            app.log(w);
        }
    }
    app.plugin_data::<State>().agents.clone().expect("read")
}

/// Whether a run is going, or its reply is waiting for NORMAL.
pub fn busy(app: &App) -> bool {
    app.plugin_data_ref::<State>()
        .is_some_and(|s| s.job.is_some() || s.reply.is_some())
}

/// Where a reply goes.
enum Target {
    /// `:ask`: the lines the cursor or selection was on, in the body as it
    /// was then.
    Lines {
        v: VnodeId,
        /// The selected lines, or the cursor's line alone.
        lines: (usize, usize),
        replace: bool,
        body: String,
    },
    /// `:agent-run`: the `@agent` node.
    Node(node::Target),
}

impl Target {
    /// What messages start with.
    fn verb(&self) -> &'static str {
        match self {
            Target::Lines { .. } => "ask",
            Target::Node(_) => "agent",
        }
    }
}

/// A run of the agent.
struct Job {
    child: Child,
    receive: Receiver<(Stream, std::io::Result<Vec<u8>>)>,
    stdout: Option<std::io::Result<Vec<u8>>>,
    stderr: Option<std::io::Result<Vec<u8>>>,
    status: Option<ExitStatus>,
    started: Instant,
    timeout: Duration,
    label: String,
    verb: &'static str,
    target: Option<Target>,
    /// The run's MCP server; dropped after the child is killed.
    mcp: Option<mcp::Served>,
}

#[derive(Clone, Copy)]
enum Stream {
    Out,
    Err,
}

impl Drop for Job {
    /// A run left behind by a quit or a cancel must not keep running.
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// What `:ask` or an `@agent` headline was given.
#[derive(Debug, Default, PartialEq, Eq)]
struct Request {
    scope: Scope,
    choice: Choice,
    reply: Reply,
    /// `prompt=headline`: the body form's line form.
    prompt_is_headline: bool,
    /// `:ask lines=all`: the whole body, as if selected.
    all_lines: bool,
    prompt: String,
}

/// Where the reply goes.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
enum Reply {
    /// `:ask`: below the cursor's line or the selection.
    #[default]
    Insert,
    /// `:ask`: the selected lines; `@agent`: the body.
    Replace,
    /// `@agent`: after the body.
    Append,
    /// `@agent`: a new last child.
    Child,
}

/// Which command reads the parameters: each takes its own `reply` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Ask,
    Headline,
    /// An `@agent` line in a body: parameters only, the prompt below it or
    /// in the headline.
    Body,
}

#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
enum Scope {
    #[default]
    Prompt,
    Up(usize),
    All,
}

/// Read `:ask`'s argument, or an `@agent` headline after `@agent`. Leading
/// `key=value` words are parameters; the prompt starts at the first word
/// that is not one, or after `--`.
fn parse(arg: &str, form: Form) -> Result<Request, String> {
    let mut r = Request {
        reply: match form {
            Form::Ask => Reply::Insert,
            Form::Headline | Form::Body => Reply::Append,
        },
        ..Request::default()
    };
    let mut rest = arg.trim_start();
    loop {
        let word = rest.split_whitespace().next().unwrap_or("");
        if word == "--" {
            rest = rest[2..].trim_start();
            break;
        }
        let Some((key, value)) = word.split_once('=') else {
            break;
        };
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_lowercase() || c == '-') {
            break;
        }
        match key {
            "scope" => {
                r.scope = match value {
                    "0" => Scope::Prompt,
                    "all" => Scope::All,
                    n => match n.parse::<usize>() {
                        Ok(n) => Scope::Up(n),
                        Err(_) => return Err(format!("scope is 0, a number or all, not `{n}`")),
                    },
                }
            }
            "agent" => r.choice.agent = Some(value.to_string()),
            "provider" => r.choice.provider = Some(value.to_string()),
            "model" => r.choice.model = Some(value.to_string()),
            "reply" => {
                r.reply = match (form, value) {
                    (Form::Ask, "insert") => Reply::Insert,
                    (Form::Headline | Form::Body, "append") => Reply::Append,
                    (Form::Headline | Form::Body, "child") => Reply::Child,
                    (_, "replace") => Reply::Replace,
                    (Form::Ask, v) => {
                        return Err(format!("reply is insert or replace for :ask, not `{v}`"))
                    }
                    (Form::Headline | Form::Body, v) => {
                        return Err(format!(
                            "reply is append, replace or child for @agent, not `{v}`"
                        ))
                    }
                }
            }
            "prompt" if form == Form::Headline => {
                return Err("prompt= is for an @agent line in a body; in a headline the prompt follows the parameters".to_string())
            }
            "lines" if form == Form::Ask => match value {
                "all" => r.all_lines = true,
                v => {
                    return Err(format!(
                        "lines is all, not `{v}`; select lines in VISUAL for fewer"
                    ))
                }
            },
            "prompt" if form == Form::Body => {
                r.prompt_is_headline = match value {
                    "body" => false,
                    "headline" => true,
                    v => return Err(format!("prompt is body or headline, not `{v}`")),
                }
            }
            other => {
                return Err(format!(
                    "no parameter `{other}`: scope, lines, agent, provider, model, reply; `--` starts the prompt"
                ))
            }
        }
        if matches!(key, "agent" | "provider" | "model") && value.is_empty() {
            return Err(format!("{key} is empty"));
        }
        rest = rest[word.len()..].trim_start();
    }
    r.prompt = rest.trim_end().to_string();
    match (form, r.prompt.is_empty()) {
        (Form::Ask, true) => Err(USAGE.to_string()),
        (Form::Headline, true) => {
            Err("the headline has no prompt: @agent [params] PROMPT".to_string())
        }
        (Form::Body, false) => Err(format!(
            "the @agent line holds parameters only; `{}` goes on the lines below it, before @agent-end",
            r.prompt
        )),
        _ => Ok(r),
    }
}

/// The prompt, the selected lines, and the outline text the scope names, as
/// the agent reads it; and how many nodes the scope sent.
///
/// With `own_body` false, `p` is sent as its headline alone, without its
/// body or descendants: an `@agent` node's replies would otherwise feed
/// every later run.
/// The node whose subtree `scope` names from `p`: `p` itself for `0`, its
/// N-th ancestor, stopping at the top level, and None for `all`.
fn scope_node(o: &Outline, p: &Position, scope: Scope) -> Option<Position> {
    let n = match scope {
        Scope::Prompt => 0,
        Scope::Up(n) => n,
        Scope::All => return None,
    };
    let mut top = p.clone();
    for _ in 0..n {
        match top.parent(o) {
            Some(parent) => top = parent,
            None => break,
        }
    }
    Some(top)
}

fn prompt_text(
    o: &Outline,
    p: &Position,
    request: &Request,
    selected: Option<(usize, usize)>,
    own_body: bool,
) -> (String, usize) {
    let nodes: Vec<Position> = match (request.scope, scope_node(o, p, request.scope)) {
        (Scope::Prompt, _) => Vec::new(),
        (_, Some(top)) => top.self_and_subtree(o),
        (_, None) => o.all_positions(),
    };
    let mut text = request.prompt.clone();
    text.push('\n');
    if let Some((first, last)) = selected {
        let body = leoapp::editor::split(p.b(o));
        text.push_str(&format!(
            "\nThe text, lines {} to {} of the body:\n\n",
            first + 1,
            last + 1
        ));
        for line in &body[first..=last.min(body.len() - 1)] {
            text.push_str(line);
            text.push('\n');
        }
    }
    if nodes.is_empty() {
        return (text, 0);
    }
    text.push_str(
        "\nContext, from the outline: each node is a `*` headline, its body below it.\n\n",
    );
    let nodes: Vec<Position> = match own_body {
        true => nodes,
        false => nodes
            .into_iter()
            .filter(|q| !p.is_ancestor_of(o, q))
            .collect(),
    };
    let base = nodes.iter().map(|q| q.level()).min().unwrap_or(0);
    for q in &nodes {
        let indent = "  ".repeat(q.level() - base);
        text.push_str(&format!("{indent}* {}\n", q.h(o)));
        if !own_body && q == p {
            continue;
        }
        for line in q.b(o).lines() {
            text.push_str(&format!("{indent}  {line}\n"));
        }
    }
    (text, nodes.len())
}

/// `:ask ARG`: start a run whose reply goes below the cursor's line.
pub fn ask(app: &mut App, arg: &str) {
    if refuse_busy(app, "ask") {
        return;
    }
    let request = match parse(arg, Form::Ask) {
        Ok(r) => r,
        Err(e) => {
            app.message = format!("ask: {e}");
            return;
        }
    };
    if app.focus != Focus::Body {
        app.message =
            "ask: the reply goes below the body cursor; move to the body first".to_string();
        return;
    }
    // From VISUAL the selection is still set: the `:` line clears it after
    // the command, as `:extract` reads it. Ex ranges are linewise.
    // `lines=all` was typed, so it wins over a selection.
    let row = app.editor.cursor.0;
    let selected = match request.all_lines {
        true => Some((0, app.body_line_count().saturating_sub(1))),
        false => app
            .editor
            .visual
            .map(|(anchor, _)| (anchor.0.min(row), anchor.0.max(row))),
    };
    if request.reply == Reply::Replace && selected.is_none() {
        app.message =
            "ask: reply=replace replaces the selected lines; select them in VISUAL first, or add lines=all"
                .to_string();
        return;
    }
    let o = app.outline();
    let p = app.current.clone();
    if let Some(why) = leolib::ext::kind_at(o, &p).and_then(|k| k.read_only(o, &p)) {
        app.message = format!("ask: {why}");
        return;
    }
    let (mut text, nodes) = prompt_text(o, &p, &request, selected, true);
    let reach = mcp::Reach::new(o, &p, request.scope);
    let target = Target::Lines {
        v: p.v,
        lines: selected.unwrap_or((row, row)),
        replace: request.reply == Reply::Replace,
        body: p.b(o).to_string(),
    };
    let Some(run) = prepare(app, &request.choice, &text, "ask") else {
        return;
    };
    if run.mcp {
        text.push_str(&reach.note);
    }
    start(app, run, text, nodes, target, reach.within);
}

/// Refuse, with why, while a run is going or its reply waits. True if
/// refused.
fn refuse_busy(app: &mut App, verb: &str) -> bool {
    let busy = app
        .plugin_data_ref::<State>()
        .map(|s| (s.job.is_some(), s.reply.is_some()));
    app.message = match busy {
        Some((true, _)) => format!("{verb}: a run is going; :agent-cancel stops it"),
        Some((_, true)) => format!("{verb}: a reply is waiting for NORMAL mode"),
        _ => return false,
    };
    true
}

/// The command `choice` runs, if the prompt is within the size limit and the
/// table resolves it; else None, with why on the status line.
fn prepare(app: &mut App, choice: &Choice, text: &str, verb: &str) -> Option<Invocation> {
    if text.len() > MAX_PROMPT_BYTES {
        app.message = format!(
            "{verb}: the prompt and its scope are {} KB, over the {} KB limit",
            text.len() / 1024,
            MAX_PROMPT_BYTES / 1024
        );
        return None;
    }
    let agents = table(app);
    match agents.resolve(choice, |v| std::env::var(v).ok()) {
        Ok(run) => Some(run),
        Err(e) => {
            app.message = format!("{verb}: {e}");
            None
        }
    }
}

/// Start `run` on `text`, its reply bound for `target`. An MCP run's tools
/// reach only `within`'s subtree, or the whole outline if None.
fn start(
    app: &mut App,
    mut run: Invocation,
    text: String,
    nodes: usize,
    target: Target,
    within: Option<VnodeId>,
) {
    let verb = target.verb();
    let dir = outline_dir(app);
    let timeout = table(app).timeout();
    let served = match run.mcp {
        false => None,
        true => match mcp::serve(within) {
            Ok(served) => {
                let path = served.config.to_string_lossy().to_string();
                for arg in &mut run.args {
                    *arg = arg.replace(agents::MCP_WORD, &path);
                }
                Some(served)
            }
            Err(e) => {
                app.message = format!(
                    "{verb}: the MCP server for {} did not start: {e}",
                    run.label
                );
                return;
            }
        },
    };
    let job = match spawn(&run, text.clone(), &dir) {
        Ok((child, receive)) => Job {
            child,
            receive,
            stdout: None,
            stderr: None,
            status: None,
            started: Instant::now(),
            timeout,
            label: run.label.clone(),
            verb,
            target: Some(target),
            mcp: served,
        },
        Err(e) => {
            app.message = match e.kind() {
                std::io::ErrorKind::NotFound => format!(
                    "{verb}: no `{}` command found; name its full path in agents.toml",
                    run.program
                ),
                _ => format!("{verb}: {} did not start: {e}", run.label),
            };
            return;
        }
    };
    if nodes > 0 {
        app.log(format!(
            "{verb}: sent {} nodes, {} bytes, to {}",
            nodes,
            text.len(),
            run.label
        ));
    }
    app.message = match run.mcp {
        true => format!("{verb}: {} is running, and may edit the outline", run.label),
        false => format!("{verb}: {} is running", run.label),
    };
    let state = app.plugin_data::<State>();
    state.job = Some(job);
    state.last_edit = None;
}

/// Start `run` with `text` on its stdin, its output read on threads.
#[allow(clippy::type_complexity)]
fn spawn(
    run: &Invocation,
    text: String,
    dir: &std::path::Path,
) -> std::io::Result<(Child, Receiver<(Stream, std::io::Result<Vec<u8>>)>)> {
    let mut child = Command::new(&run.program)
        .args(&run.args)
        .envs(run.env.iter().map(|(k, v)| (k, v)))
        .env("NO_COLOR", "1")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped");
    std::thread::spawn(move || {
        // An agent that exits without reading closes the pipe; its exit says why.
        let _ = stdin.write_all(text.as_bytes());
    });
    let (send, receive) = mpsc::channel();
    let streams: [(Stream, Box<dyn Read + Send>); 2] = [
        (Stream::Out, Box::new(child.stdout.take().expect("piped"))),
        (Stream::Err, Box::new(child.stderr.take().expect("piped"))),
    ];
    for (which, mut stream) in streams {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream.read_to_end(&mut bytes).map(|_| bytes);
            let _ = send.send((which, result));
        });
    }
    Ok((child, receive))
}

/// The directory the outline's file is in, where the agent runs.
fn outline_dir(app: &App) -> std::path::PathBuf {
    let file = &app.outline().file_name;
    let dir = match file.is_empty() {
        true => None,
        false => std::path::absolute(file)
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
    };
    dir.or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into())
}

/// `:agent-cancel`.
pub fn cancel(app: &mut App) {
    let state = app.plugin_data::<State>();
    let job = state.job.take();
    let reply = state.reply.take();
    app.message = match (job, reply) {
        (Some(job), _) => format!("{}: {} stopped", job.verb, job.label),
        (None, Some((target, _))) => format!("{}: the waiting reply was dropped", target.verb()),
        (None, None) => "agent: nothing is running".to_string(),
    };
}

/// Collect a finished run, and insert a reply once the user is in NORMAL.
/// True if anything changed.
fn poll(app: &mut App) -> bool {
    let mut changed = mcp::answer(app);
    changed |= collect(app);
    let typing = app.mode != Mode::Normal || app.buffer.is_some();
    if !typing {
        if let Some((target, text)) = app.plugin_data::<State>().reply.take() {
            let before = app.doc.undoer().version();
            match target {
                Target::Lines {
                    v,
                    lines,
                    replace,
                    body,
                } => deliver(app, v, lines, replace, &body, &text),
                Target::Node(t) => node::deliver(app, t, &text),
            }
            mcp::join(app, before);
            changed = true;
        }
    }
    changed
}

/// Move a finished run's output into the state. True if the run ended.
fn collect(app: &mut App) -> bool {
    let Some(job) = app.plugin_data::<State>().job.as_mut() else {
        return false;
    };
    loop {
        match job.receive.try_recv() {
            Ok((Stream::Out, r)) => job.stdout = Some(r),
            Ok((Stream::Err, r)) => job.stderr = Some(r),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    if job.status.is_none() {
        match job.child.try_wait() {
            Ok(Some(status)) => job.status = Some(status),
            Ok(None) if job.started.elapsed() >= job.timeout => {
                let job = app.plugin_data::<State>().job.take().expect("checked");
                app.message = format!(
                    "{}: {} took over {} s and was stopped; agents.toml's timeout sets the limit",
                    job.verb,
                    job.label,
                    job.timeout.as_secs()
                );
                return true;
            }
            Ok(None) => return false,
            Err(e) => {
                let job = app.plugin_data::<State>().job.take().expect("checked");
                app.message = format!("{}: {}: {e}", job.verb, job.label);
                return true;
            }
        }
    }
    if job.stdout.is_none() || job.stderr.is_none() {
        return false;
    }
    let mut job = app.plugin_data::<State>().job.take().expect("checked");
    let status = job.status.expect("checked");
    let text = |r: Option<std::io::Result<Vec<u8>>>| match r {
        Some(Ok(bytes)) => String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
        _ => String::new(),
    };
    let (stdout, stderr) = (text(job.stdout.take()), text(job.stderr.take()));
    let verb = job.verb;
    for line in stderr.lines().filter(|l| !l.trim().is_empty()) {
        app.log(format!("{verb}: {line}"));
    }
    if !status.success() {
        let why = stderr
            .lines()
            .chain(stdout.lines())
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no output");
        let code = status
            .code()
            .map_or_else(|| "a signal".to_string(), |c| format!("exit {c}"));
        app.message = format!("{verb}: {} failed ({code}): {why}", job.label);
        return true;
    }
    let reply = stdout.trim_end_matches(['\n', ' ', '\t']).to_string();
    if reply.trim().is_empty() {
        app.message = format!("{verb}: {} replied with nothing", job.label);
        return true;
    }
    app.message = format!(
        "{verb}: {} replied; it goes in once you are in NORMAL",
        job.label
    );
    let target = job.target.take().expect("set at spawn");
    app.plugin_data::<State>().reply = Some((target, reply));
    true
}

/// Insert `reply` below the target line, or put it in the register if the
/// body changed since `:ask` ran: the line it named may have moved.
fn deliver(
    app: &mut App,
    v: VnodeId,
    (first, last): (usize, usize),
    replace: bool,
    old: &str,
    reply: &str,
) {
    let o = app.outline();
    let found = o
        .all_positions()
        .into_iter()
        .find(|q| q.v == v)
        .filter(|q| q.b(o) == old);
    let lines = reply.split('\n').count();
    let Some(p) = found else {
        to_register(app, "ask", reply, "the body changed during the run");
        return;
    };
    let mut body = leoapp::editor::split(old);
    let last = last.min(body.len() - 1);
    let new: Vec<String> = reply.split('\n').map(str::to_string).collect();
    let (text, row) = if old.is_empty() {
        (format!("{reply}\n"), 0)
    } else if replace {
        body.splice(first..=last, new);
        (leoapp::editor::rejoin(&body, old), first)
    } else {
        let row = last + 1;
        body.splice(row..row, new);
        (leoapp::editor::rejoin(&body, old), row)
    };
    app.doc.set_body(&p, &text);
    app.check_rules(true);
    if app.message.starts_with("refused: ") {
        let why = app.message.clone();
        to_register(app, "ask", reply, &why);
        return;
    }
    if app.current.v == v {
        app.editor.cursor = (row, 0);
    }
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    app.message = match replace {
        true => format!(
            "ask: {} line{} replaced by {} line{}",
            last - first + 1,
            plural(last - first + 1),
            lines,
            plural(lines)
        ),
        false => format!("ask: {} line{} inserted", lines, plural(lines)),
    };
}

fn to_register(app: &mut App, verb: &str, reply: &str, why: &str) {
    app.editor.register = Register {
        text: format!("{reply}\n"),
        linewise: true,
    };
    for line in reply.lines() {
        app.log(format!("{verb} reply: {line}"));
    }
    app.message = format!("{verb}: not written, {why}; `p` puts the reply");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ask(arg: &str) -> Result<Request, String> {
        parse(arg, Form::Ask)
    }

    #[test]
    fn parameters_come_first_and_the_prompt_is_the_rest() {
        let r = parse_ask("scope=2 model=opus  list sort algorithms ").unwrap();
        assert_eq!(r.scope, Scope::Up(2));
        assert_eq!(r.choice.model.as_deref(), Some("opus"));
        assert_eq!(r.prompt, "list sort algorithms");
        let r = parse_ask("what is x=1 here").unwrap();
        assert_eq!(r.prompt, "what is x=1 here");
        let r = parse_ask("-- scope=all is a word here").unwrap();
        assert_eq!(r.scope, Scope::Prompt);
        assert_eq!(r.prompt, "scope=all is a word here");
        assert_eq!(parse_ask("scope=all x").unwrap().scope, Scope::All);
    }

    #[test]
    fn a_bad_parameter_refuses() {
        assert!(parse_ask("scoep=1 x")
            .unwrap_err()
            .starts_with("no parameter `scoep`"));
        assert!(parse_ask("scope=up x").is_err());
        assert_eq!(parse_ask("reply=replace x").unwrap().reply, Reply::Replace);
        assert!(parse_ask("reply=append x").is_err());
        let h = |a| parse(a, Form::Headline);
        assert_eq!(h("x").unwrap().reply, Reply::Append);
        assert_eq!(h("reply=child x").unwrap().reply, Reply::Child);
        assert!(h("reply=insert x").is_err());
        assert!(h("prompt=headline x")
            .unwrap_err()
            .starts_with("prompt= is for"));
        assert!(h("scope=1").unwrap_err().contains("no prompt"));
        let b = |a| parse(a, Form::Body);
        assert!(b("prompt=headline").unwrap().prompt_is_headline);
        assert!(!b("scope=1").unwrap().prompt_is_headline);
        assert!(b("prompt=title").is_err());
        assert!(b("scope=1 tell me")
            .unwrap_err()
            .contains("parameters only"));
        assert!(parse_ask("lines=all x").unwrap().all_lines);
        assert!(parse_ask("lines=3 x")
            .unwrap_err()
            .starts_with("lines is all"));
        assert!(h("lines=all x")
            .unwrap_err()
            .starts_with("no parameter `lines`"));
        assert!(parse_ask("reply=child x").is_err());
        assert!(parse_ask("model= x").is_err());
        assert_eq!(parse_ask("scope=1").unwrap_err(), USAGE);
        assert_eq!(parse_ask("  ").unwrap_err(), USAGE);
    }
}
