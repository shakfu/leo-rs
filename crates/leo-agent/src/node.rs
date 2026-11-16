//! `@agent` nodes, which `:agent-run` runs. The design is in
//! `docs/dev/agents.md`, section 4. Three forms say where the prompt is:
//!
//! - headline: `@agent [params] PROMPT` as the headline; the body is replies;
//! - block: an `@agent [params]` line in the body, the prompt on the lines
//!   below it, then `@agent-end`; the replies follow;
//! - line: an `@agent prompt=headline [params]` line in the body; the prompt
//!   is the headline, and the replies follow the line.

use std::hash::{Hash, Hasher};

use leoapp::app::App;
use leolib::{Place, Position, VnodeId};

use crate::{parse, prepare, prompt_text, refuse_busy, start, Form, Reply, Scope, State};

/// The directive, in a headline or at the start of a body line.
pub const DIRECTIVE: &str = "@agent";

/// The line that ends a block form's prompt.
pub const END: &str = "@agent-end";

/// The most characters of the prompt a child's headline keeps.
const HEADLINE_PROMPT_CHARS: usize = 60;

/// Where an `@agent` node's reply goes.
pub(crate) struct Target {
    v: VnodeId,
    reply: Reply,
    /// The body when the run started, for `replace`.
    body: String,
    /// The bytes of `body` before its replies, which `replace` keeps: the
    /// prose and prompt of a body form, none of a headline form's.
    keep: usize,
    /// `-- 2026-11-16 14:03Z claude --`, written before the reply.
    separator: String,
    /// The child's headline, for `reply=child`.
    child_headline: String,
}

/// The rest of the headline after `@agent`, if it is an `@agent` headline.
pub fn agent_headline(h: &str) -> Option<&str> {
    let rest = h.trim_start().strip_prefix(DIRECTIVE)?;
    match rest.chars().next() {
        None => Some(""),
        Some(c) if c.is_whitespace() => Some(rest.trim_start()),
        _ => None,
    }
}

/// Whether `line` is a body's `@agent` line: at the start of the line, as
/// a directive is, then a blank or nothing.
pub fn is_agent_line(line: &str) -> bool {
    line.strip_prefix(DIRECTIVE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
}

/// Whether `p` holds a prompt in any form.
pub fn is_agent_node(o: &leolib::Outline, p: &Position) -> bool {
    agent_headline(p.h(o)).is_some() || p.b(o).split('\n').any(is_agent_line)
}

/// A node's prompt, wherever its form puts it.
#[derive(Debug)]
struct Found {
    request: crate::Request,
    /// The bytes of the body before the replies.
    keep: usize,
}

/// The prompt node `h` and `body` hold, or why there is none.
fn find_prompt(h: &str, body: &str) -> Result<Found, String> {
    let lines: Vec<(usize, &str)> = body
        .split_inclusive('\n')
        .scan(0, |at, l| {
            let start = *at;
            *at += l.len();
            Some((start, l))
        })
        .collect();
    let agent_lines: Vec<usize> = (0..lines.len())
        .filter(|&i| is_agent_line(lines[i].1.trim_end_matches(['\n', '\r'])))
        .collect();
    if let Some(rest) = agent_headline(h) {
        if let Some(&i) = agent_lines.first() {
            return Err(format!(
                "the headline is an @agent prompt and so is body line {}: one prompt per node",
                i + 1
            ));
        }
        let request = parse(rest, Form::Headline)?;
        return Ok(Found { request, keep: 0 });
    }
    let (&i, more) = match agent_lines.split_first() {
        Some(x) => x,
        None => {
            return Err(format!(
                "`{h}` is not an @agent node: headline it @agent PROMPT, or put an @agent line in its body"
            ))
        }
    };
    if let Some(&j) = more.first() {
        return Err(format!(
            "body lines {} and {} are both @agent lines: one prompt per node",
            i + 1,
            j + 1
        ));
    }
    let (start, line) = lines[i];
    let params = line.trim_end_matches(['\n', '\r'])[DIRECTIVE.len()..].trim();
    let mut request = parse(params, Form::Body)?;
    let after_line = start + line.len();
    if request.prompt_is_headline {
        request.prompt = h.trim().to_string();
        if request.prompt.is_empty() {
            return Err("prompt=headline, but the headline is empty".to_string());
        }
        return Ok(Found {
            request,
            keep: after_line,
        });
    }
    let end = lines[i + 1..]
        .iter()
        .position(|(_, l)| l.trim_end() == END)
        .map(|k| i + 1 + k);
    let Some(end) = end else {
        return Err(format!(
            "the @agent line, body line {}, has no {END} line below it to end the prompt",
            i + 1
        ));
    };
    request.prompt = lines[i + 1..end]
        .iter()
        .map(|(_, l)| *l)
        .collect::<String>()
        .trim()
        .to_string();
    if request.prompt.is_empty() {
        return Err(format!(
            "no prompt between the @agent line and {END}; or use prompt=headline"
        ));
    }
    let (end_start, end_line) = lines[end];
    Ok(Found {
        request,
        keep: end_start + end_line.len(),
    })
}

/// `:agent-run`, and the y/n prompt's yes when `confirmed`.
pub(crate) fn run(app: &mut App, confirmed: bool) {
    if refuse_busy(app, "agent") {
        return;
    }
    let o = app.outline();
    let p = app.current.clone();
    let Found { request, keep } = match find_prompt(p.h(o), p.b(o)) {
        Ok(found) => found,
        Err(e) => {
            app.message = format!("agent: {e}");
            return;
        }
    };
    if let Some(why) = leolib::ext::kind_at(o, &p).and_then(|k| k.read_only(o, &p)) {
        app.message = format!("agent: {why}");
        return;
    }
    let body = p.b(o).to_string();
    if request.reply == Reply::Replace && edited_since_reply(app, p.v, &body) {
        app.message = "agent: the body changed since the last reply, and reply=replace would overwrite it; undo the change, clear the body, or use reply=append".to_string();
        return;
    }
    let (mut text, nodes) = prompt_text(app.outline(), &p, &request, None, false);
    let Some(invocation) = prepare(app, &request.choice, &text, "agent") else {
        return;
    };
    let reach = crate::mcp::Reach::new(app.outline(), &p, request.scope);
    if invocation.mcp {
        text.push_str(&reach.note);
    }
    // A hostile outline's prompt could send the outline to an account the
    // user holds: ask before the first run in an outline, before any run
    // that sends outline text, and before any run that may edit it.
    let outline = app.outline().file_name.clone();
    let trusted = app.plugin_data::<State>().trusted.contains(&outline);
    if !confirmed && (!trusted || request.scope != Scope::Prompt || invocation.mcp) {
        let sends = match (nodes, invocation.mcp) {
            (_, true) => match reach.within {
                Some(_) => format!(
                    ", with read and edit access to {} node{} of this outline",
                    reach.nodes,
                    plural(reach.nodes)
                ),
                None => ", with read and edit access to the whole outline".to_string(),
            },
            (0, false) => String::new(),
            (n, false) => format!(", with {n} node{} of this outline", plural(n)),
        };
        let question = format!(
            "agent: send {} KB to {}{sends}?",
            text.len().div_ceil(1024),
            invocation.label
        );
        app.confirm(question, |app| run(app, true));
        return;
    }
    app.plugin_data::<State>().trusted.insert(outline);
    let stamp = stamp(now());
    let target = Target {
        v: p.v,
        reply: request.reply,
        body,
        keep,
        separator: format!("-- {stamp} {} --", invocation.label),
        child_headline: format!("{stamp} {}", short_prompt(&request.prompt)),
    };
    let within = reach.within;
    start(
        app,
        invocation,
        text,
        nodes,
        crate::Target::Node(target),
        within,
    );
}

/// Whether `body` differs from what the last run wrote into node `v`. False
/// when no run has written it this session: undo restores what a replace
/// overwrites.
fn edited_since_reply(app: &App, v: VnodeId, body: &str) -> bool {
    app.plugin_data_ref::<State>()
        .and_then(|s| s.written.get(&v))
        .is_some_and(|&h| h != hash(body))
}

fn hash(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Write `reply` into the node: after its body, in its place, or into a new
/// last child, as one undo step.
pub(crate) fn deliver(app: &mut App, t: Target, reply: &str) {
    let o = app.outline();
    let Some(p) = o.all_positions().into_iter().find(|q| q.v == t.v) else {
        crate::to_register(app, "agent", reply, "the @agent node was deleted");
        return;
    };
    let body = p.b(o).to_string();
    let written = format!("{}\n{reply}\n", t.separator);
    let headline = p.h(o).to_string();
    app.doc.begin_group("agent-reply");
    let what = match t.reply {
        Reply::Child => {
            let Some(child) = add_last_child(app, &p) else {
                app.doc.end_group();
                crate::to_register(app, "agent", reply, "the @agent node moved");
                return;
            };
            app.doc.set_headline(&child, &t.child_headline);
            app.doc.set_body(&child, &written);
            "written to a new child of"
        }
        Reply::Replace if body != t.body => {
            app.doc.end_group();
            crate::to_register(app, "agent", reply, "the body changed during the run");
            return;
        }
        Reply::Replace => {
            let mut new = body[..t.keep].to_string();
            if !new.is_empty() && !new.ends_with('\n') {
                new.push('\n');
            }
            new.push_str(&written);
            app.doc.set_body(&p, &new);
            "in place of the replies of"
        }
        _ => {
            let mut new = body.clone();
            if !new.is_empty() {
                if !new.ends_with('\n') {
                    new.push('\n');
                }
                new.push('\n');
            }
            new.push_str(&written);
            app.doc.set_body(&p, &new);
            "appended to"
        }
    };
    app.doc.end_group();
    app.check_rules(true);
    if app.message.starts_with("refused: ") {
        let why = app.message.clone();
        crate::to_register(app, "agent", reply, &why);
        return;
    }
    if t.reply != Reply::Child {
        let now = p.b(app.outline()).to_string();
        app.plugin_data::<State>().written.insert(t.v, hash(&now));
    }
    app.message = format!("agent: the reply is {what} `{headline}`");
}

/// A new last child of `p`, undoably.
fn add_last_child(app: &mut App, p: &Position) -> Option<Position> {
    let first = app.doc.insert_child(p)?;
    Some(app.doc.move_node(&first, p, Place::Inside).unwrap_or(first))
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Seconds since the epoch.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `2026-11-16 14:03Z`: UTC, to the minute. The standard library has no
/// time zones, and a reply's time needs no dependency.
fn stamp(secs: i64) -> String {
    let (y, mo, d, h, mi, _) = leolib::gnx::civil_from_unix(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}Z")
}

/// The prompt's first non-blank line, cut at the last word boundary before
/// 60 characters, with `...` added.
fn short_prompt(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() <= HEADLINE_PROMPT_CHARS {
        return line.to_string();
    }
    let cut: String = line.chars().take(HEADLINE_PROMPT_CHARS).collect();
    let kept = match cut.rfind(char::is_whitespace) {
        Some(i) if i > 0 => &cut[..i],
        _ => &cut,
    };
    format!("{}...", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agent_headline_is_the_word_then_a_blank() {
        assert_eq!(agent_headline("@agent top 5"), Some("top 5"));
        assert_eq!(agent_headline("  @agent   x"), Some("x"));
        assert_eq!(agent_headline("@agent"), Some(""));
        assert_eq!(agent_headline("@agents x"), None);
        assert_eq!(agent_headline("@agent-end"), None);
        assert_eq!(agent_headline("Top 5"), None);
    }

    #[test]
    fn a_body_line_is_at_the_start_of_the_line() {
        assert!(is_agent_line("@agent scope=1"));
        assert!(is_agent_line("@agent"));
        assert!(!is_agent_line(" @agent"));
        assert!(!is_agent_line("@agent-end"));
        assert!(!is_agent_line("@agents"));
    }

    #[test]
    fn each_form_finds_its_prompt_and_the_replies_start() {
        let f = find_prompt("@agent scope=1 hi", "old reply\n").unwrap();
        assert_eq!((f.request.prompt.as_str(), f.keep), ("hi", 0));

        let body = "Notes.\n@agent reply=child\nList the\nsorts.\n@agent-end\nreply\n";
        let f = find_prompt("Sorting", body).unwrap();
        assert_eq!(f.request.prompt, "List the\nsorts.");
        assert_eq!(f.request.reply, Reply::Child);
        assert_eq!(&body[f.keep..], "reply\n");

        let body = "@agent prompt=headline\nreply\n";
        let f = find_prompt("Top 5 sort algorithms", body).unwrap();
        assert_eq!(f.request.prompt, "Top 5 sort algorithms");
        assert_eq!(&body[f.keep..], "reply\n");
    }

    #[test]
    fn one_prompt_per_node_and_a_block_needs_its_end() {
        let e = find_prompt("@agent hi", "@agent prompt=headline\n").unwrap_err();
        assert!(e.contains("body line 1: one prompt per node"), "{e}");
        let e = find_prompt("x", "@agent\na\n@agent-end\n@agent\n").unwrap_err();
        assert!(e.contains("lines 1 and 4"), "{e}");
        let e = find_prompt("x", "@agent\nno end\n").unwrap_err();
        assert!(e.contains("no @agent-end line"), "{e}");
        let e = find_prompt("x", "@agent\n@agent-end\n").unwrap_err();
        assert!(e.contains("no prompt between"), "{e}");
        let e = find_prompt("Plain", "text\n").unwrap_err();
        assert!(e.contains("is not an @agent node"), "{e}");
    }

    #[test]
    fn the_stamp_is_utc_to_the_minute() {
        // 2026-11-16 14:03:59 UTC.
        assert_eq!(stamp(1_794_837_839), "2026-11-16 14:03Z");
        assert_eq!(stamp(0), "1970-01-01 00:00Z");
    }

    #[test]
    fn a_long_prompt_is_cut_at_a_word() {
        assert_eq!(short_prompt("\n  short one\nsecond"), "short one");
        let long = "List the top five sort algorithms, with their average and worst-case cost";
        assert_eq!(
            short_prompt(long),
            "List the top five sort algorithms, with their average and..."
        );
        let word = "x".repeat(70);
        assert_eq!(short_prompt(&word), format!("{}...", "x".repeat(60)));
    }
}
