//! `agents.toml`: the agents a prompt may name, the providers each reaches,
//! and the models each allows. A prompt names these; only this file says
//! what they run. The keys are in `docs/dev/agents.md`, section 3.

use std::collections::BTreeMap;

/// The agent a prompt runs when neither it nor `agents.toml` names one.
pub const DEFAULT_AGENT: &str = "claude";

/// `~/.config/leo-rs/agents.toml`.
pub fn default_path() -> Option<std::path::PathBuf> {
    leoapp::config::dir().map(|d| d.join("agents.toml"))
}

/// The word in an agent's command that becomes the path of an MCP config
/// file, naming a server that lets the agent read and edit the outline.
pub const MCP_WORD: &str = "{mcp}";

/// How long a run may take before it is killed, when `timeout` is unset.
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Which replies ask before they go in: `confirm-reply` in agents.toml.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfirmReply {
    #[default]
    Never,
    /// A reply that replaces text: `reply=replace`.
    Replace,
    Always,
}

impl ConfirmReply {
    fn parse(value: &str) -> Result<ConfirmReply, String> {
        match value {
            "never" => Ok(ConfirmReply::Never),
            "replace" => Ok(ConfirmReply::Replace),
            "always" => Ok(ConfirmReply::Always),
            _ => Err(format!(
                "confirm-reply is never, replace or always, not `{value}`"
            )),
        }
    }

    /// Whether a reply asks first; `replace` says it replaces text.
    pub fn asks(self, replace: bool) -> bool {
        match self {
            ConfirmReply::Never => false,
            ConfirmReply::Replace => replace,
            ConfirmReply::Always => true,
        }
    }
}

/// The agents table.
#[derive(Clone, Debug, Default)]
pub struct Agents {
    default: Option<String>,
    timeout: Option<u64>,
    confirm_reply: Option<ConfirmReply>,
    /// `stamp = local`: separators in local time, not UTC.
    local_time: bool,
    agents: BTreeMap<String, Agent>,
    /// Where the table came from, for a message naming a missing agent.
    source: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Agent {
    command: Option<Vec<String>>,
    default_provider: Option<String>,
    confirm_reply: Option<ConfirmReply>,
    timeout: Option<u64>,
    /// An `@agent` node's `reply=` when it names none.
    reply: Option<String>,
    /// Text sent before every prompt.
    preamble: Option<String>,
    own: Models,
    providers: BTreeMap<String, Provider>,
}

#[derive(Clone, Debug, Default)]
struct Provider {
    env: Vec<(String, String)>,
    own: Models,
}

/// The model keys an agent and each of its providers may set.
#[derive(Clone, Debug, Default)]
struct Models {
    /// The words a model adds to the command, `{model}` in them.
    model: Option<Vec<String>>,
    default_model: Option<String>,
    models: Option<Vec<String>>,
}

/// What a prompt asked for: each `None` takes the table's default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub agent: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// A command to run, resolved from a [`Choice`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// `claude`, or `claude openrouter anthropic/claude-sonnet-4.5`.
    pub label: String,
    /// The command names [`MCP_WORD`]: the run may edit the outline.
    pub mcp: bool,
    pub confirm_reply: ConfirmReply,
    pub timeout: std::time::Duration,
    /// `append`, `replace` or `child`: an `@agent` node's default reply.
    pub reply: Option<String>,
    pub preamble: Option<String>,
}

impl Agents {
    /// The table in `text`, and a warning for each line it ignored.
    pub fn parse(text: &str) -> (Agents, Vec<String>) {
        let mut t = Agents::default();
        let mut warnings = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let n = i + 1;
            let Some((key, value)) = line.split_once('=') else {
                warnings.push(format!("agents.toml line {n}: expected key = value"));
                continue;
            };
            let (key, value) = (unquote(key.trim()), unquote(value.trim()));
            if let Err(e) = t.set(key, value) {
                warnings.push(format!("agents.toml line {n}: {e}"));
            }
        }
        (t, warnings)
    }

    /// The table in the file at `path`, or an empty one if there is none.
    pub fn load(path: &std::path::Path) -> (Agents, Vec<String>) {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let (mut t, w) = Agents::parse(&text);
                t.source = Some(format!("read {}", path.display()));
                (t, w)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let source = Some(format!("{} does not exist", path.display()));
                (
                    Agents {
                        source,
                        ..Agents::default()
                    },
                    Vec::new(),
                )
            }
            Err(e) => (
                Agents::default(),
                vec![format!("{} not read: {e}", path.display())],
            ),
        }
    }

    /// Whether separators are in local time.
    pub fn local_time(&self) -> bool {
        self.local_time
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let words = || {
            value
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let parts: Vec<&str> = key.split('.').collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err(format!("`{key}` is not a key"));
        }
        match parts.as_slice() {
            ["default"] => self.default = Some(value.to_string()),
            ["timeout"] => self.timeout = Some(seconds(value)?),
            ["stamp"] => {
                self.local_time = match value {
                    "utc" => false,
                    "local" => true,
                    _ => return Err(format!("stamp is utc or local, not `{value}`")),
                }
            }
            [name, "timeout"] => {
                self.agents.entry(name.to_string()).or_default().timeout = Some(seconds(value)?)
            }
            [name, "reply"] => {
                if !matches!(value, "append" | "replace" | "child") {
                    return Err(format!(
                        "{name}.reply is append, replace or child, an @agent node's reply, not `{value}`"
                    ));
                }
                self.agents.entry(name.to_string()).or_default().reply = Some(value.to_string())
            }
            [name, "preamble"] => {
                self.agents.entry(name.to_string()).or_default().preamble = Some(value.to_string())
            }
            ["confirm-reply"] => self.confirm_reply = Some(ConfirmReply::parse(value)?),
            [name] => {
                if value.is_empty() {
                    return Err(format!("{name} is empty"));
                }
                self.agents.entry(name.to_string()).or_default().command = Some(words());
            }
            [name, "confirm-reply"] => {
                self.agents
                    .entry(name.to_string())
                    .or_default()
                    .confirm_reply = Some(ConfirmReply::parse(value)?)
            }
            [name, "default-provider"] => {
                self.agents
                    .entry(name.to_string())
                    .or_default()
                    .default_provider = Some(value.to_string())
            }
            [name, attr] => {
                let agent = self.agents.entry(name.to_string()).or_default();
                agent.own.set(attr, words())?;
            }
            [name, provider, "env"] => {
                let mut env = Vec::new();
                for pair in value.split_whitespace() {
                    match pair.split_once('=') {
                        Some((k, v)) if !k.is_empty() => env.push((k.to_string(), v.to_string())),
                        _ => return Err(format!("`{pair}` is not VAR=value")),
                    }
                }
                self.provider(name, provider).env = env;
            }
            [name, provider, attr] => self.provider(name, provider).own.set(attr, words())?,
            _ => return Err(format!("`{key}` is not a key")),
        }
        Ok(())
    }

    fn provider(&mut self, agent: &str, provider: &str) -> &mut Provider {
        self.agents
            .entry(agent.to_string())
            .or_default()
            .providers
            .entry(provider.to_string())
            .or_default()
    }

    /// The agent `name` with the defaults built in for `claude`: its command
    /// and model flag.
    fn agent(&self, name: &str) -> Option<Agent> {
        let mut agent = self.agents.get(name).cloned();
        if name == DEFAULT_AGENT {
            let a = agent.get_or_insert_with(Agent::default);
            a.command
                .get_or_insert_with(|| vec!["claude".into(), "-p".into()]);
            a.own
                .model
                .get_or_insert_with(|| vec!["--model".into(), "{model}".into()]);
        }
        agent.filter(|a| a.command.is_some())
    }

    /// The command `choice` runs, with `var` reading the environment for
    /// `${VAR}`. Err says what is missing or not allowed.
    pub fn resolve(
        &self,
        choice: &Choice,
        var: impl Fn(&str) -> Option<String>,
    ) -> Result<Invocation, String> {
        let name = choice
            .agent
            .as_deref()
            .or(self.default.as_deref())
            .unwrap_or(DEFAULT_AGENT);
        let Some(agent) = self.agent(name) else {
            let mut known: Vec<&str> = self
                .agents
                .iter()
                .filter(|(_, a)| a.command.is_some())
                .map(|(n, _)| n.as_str())
                .collect();
            if !known.contains(&DEFAULT_AGENT) {
                known.push(DEFAULT_AGENT);
            }
            known.sort();
            let mut e = format!(
                "no agent `{name}` in agents.toml; agents: {}",
                known.join(", ")
            );
            if let Some(source) = &self.source {
                e.push_str(&format!(" ({source})"));
            }
            return Err(e);
        };
        let provider_name = choice
            .provider
            .as_deref()
            .or(agent.default_provider.as_deref());
        let provider = match provider_name {
            None => None,
            Some(p) => match agent.providers.get(p) {
                Some(provider) => Some(provider),
                None => {
                    let known: Vec<&str> = agent.providers.keys().map(String::as_str).collect();
                    return Err(match known.is_empty() {
                        true => format!("agent `{name}` has no providers in agents.toml"),
                        false => format!(
                            "agent `{name}` has no provider `{p}`; providers: {}",
                            known.join(", ")
                        ),
                    });
                }
            },
        };
        let pick = |f: fn(&Models) -> Option<&Vec<String>>| {
            provider.and_then(|p| f(&p.own)).or(f(&agent.own))
        };
        let model = choice.model.clone().or_else(|| {
            provider
                .and_then(|p| p.own.default_model.clone())
                .or_else(|| agent.own.default_model.clone())
        });
        let mut command = agent.command.clone().expect("checked");
        let program = command.remove(0);
        let mut args = command;
        if let Some(model) = &model {
            check_model(model)?;
            if let Some(allowed) = pick(|m| m.models.as_ref()) {
                if !allowed.contains(model) {
                    return Err(format!(
                        "model `{model}` is not in {name}'s models: {}",
                        allowed.join(", ")
                    ));
                }
            }
            let Some(words) = pick(|m| m.model.as_ref()) else {
                return Err(format!(
                    "agent `{name}` takes no model: set {name}.model in agents.toml"
                ));
            };
            args.extend(words.iter().map(|w| w.replace("{model}", model)));
        }
        let mut env = Vec::new();
        for (k, v) in provider.map(|p| p.env.as_slice()).unwrap_or_default() {
            env.push((k.clone(), expand(v, &var)?));
        }
        let mut label = vec![name.to_string()];
        label.extend(provider_name.map(str::to_string));
        label.extend(model);
        Ok(Invocation {
            mcp: args.iter().any(|a| a.contains(MCP_WORD)),
            confirm_reply: agent
                .confirm_reply
                .or(self.confirm_reply)
                .unwrap_or_default(),
            timeout: std::time::Duration::from_secs(
                agent
                    .timeout
                    .or(self.timeout)
                    .unwrap_or(DEFAULT_TIMEOUT_SECS),
            ),
            reply: agent.reply.clone(),
            preamble: agent.preamble.clone(),
            program,
            args,
            env,
            label: label.join(" "),
        })
    }
}

/// A timeout's seconds: a whole number above 0.
fn seconds(value: &str) -> Result<u64, String> {
    match value.parse::<u64>() {
        Ok(s) if s > 0 => Ok(s),
        _ => Err(format!("timeout is not a number of seconds: {value}")),
    }
}

impl Models {
    fn set(&mut self, attr: &str, words: Vec<String>) -> Result<(), String> {
        match attr {
            "model" => self.model = Some(words),
            "default-model" => match words.as_slice() {
                [m] => self.default_model = Some(m.clone()),
                _ => return Err("default-model takes one model".to_string()),
            },
            "models" => self.models = Some(words),
            other => return Err(format!("`{other}` is not a key")),
        }
        Ok(())
    }
}

/// Refuse a model name that could be read as a flag or hold a blank: it is
/// passed as one argument, never through a shell.
pub fn check_model(model: &str) -> Result<(), String> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || "._-:/".contains(c);
    if model.is_empty() || model.starts_with('-') || !model.chars().all(allowed) {
        return Err(format!(
            "model `{model}` may hold only letters, digits and . _ - : /, and not start with -"
        ));
    }
    Ok(())
}

/// `value` with each `${VAR}` read from `var`.
fn expand(value: &str, var: &impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find('}') else {
            return Err(format!("`{value}` has an unclosed ${{"));
        };
        let name = &after[..j];
        match var(name) {
            Some(v) => out.push_str(&v),
            None => {
                return Err(format!(
                    "`{name}` is not set; leogui started from the desktop may not have the shell's environment"
                ))
            }
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Agents {
        let (t, warnings) = Agents::parse(text);
        assert!(warnings.is_empty(), "{warnings:?}");
        t
    }

    fn choice(agent: Option<&str>, provider: Option<&str>, model: Option<&str>) -> Choice {
        Choice {
            agent: agent.map(str::to_string),
            provider: provider.map(str::to_string),
            model: model.map(str::to_string),
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn with_no_table_claude_runs_with_its_own_model() {
        let run = Agents::default()
            .resolve(&Choice::default(), no_env)
            .unwrap();
        assert_eq!(run.program, "claude");
        assert_eq!(run.args, ["-p"]);
        assert_eq!(run.label, "claude");
        let run = Agents::default()
            .resolve(&choice(None, None, Some("opus")), no_env)
            .unwrap();
        assert_eq!(run.args, ["-p", "--model", "opus"]);
        assert_eq!(run.label, "claude opus");
    }

    #[test]
    fn an_mcp_word_in_the_command_marks_the_run() {
        let t = table("edit = claude -p --mcp-config {mcp}\n");
        assert!(
            t.resolve(&choice(Some("edit"), None, None), no_env)
                .unwrap()
                .mcp
        );
        assert!(!t.resolve(&Choice::default(), no_env).unwrap().mcp);
    }

    #[test]
    fn the_default_agent_and_its_defaults_are_used() {
        let t =
            table("default = llm\nllm = llm\nllm.model = -m {model}\nllm.default-model = gpt-4o\n");
        let run = t.resolve(&Choice::default(), no_env).unwrap();
        assert_eq!(run.program, "llm");
        assert_eq!(run.args, ["-m", "gpt-4o"]);
        assert_eq!(run.label, "llm gpt-4o");
    }

    #[test]
    fn a_provider_sets_the_environment_and_its_own_model_words() {
        let t = table(
            "claude.openrouter.env = ANTHROPIC_BASE_URL=https://openrouter.ai/api ANTHROPIC_AUTH_TOKEN=${KEY} ANTHROPIC_API_KEY=\n\
             llm = llm\nllm.model = -m {model}\nllm.openrouter.model = -m openrouter/{model}\n",
        );
        let var = |n: &str| (n == "KEY").then(|| "sk-1".to_string());
        let run = t
            .resolve(&choice(None, Some("openrouter"), None), var)
            .unwrap();
        assert_eq!(
            run.env,
            [
                (
                    "ANTHROPIC_BASE_URL".into(),
                    "https://openrouter.ai/api".into()
                ),
                ("ANTHROPIC_AUTH_TOKEN".into(), "sk-1".into()),
                ("ANTHROPIC_API_KEY".into(), String::new()),
            ]
        );
        assert_eq!(run.label, "claude openrouter");
        let run = t
            .resolve(
                &choice(
                    Some("llm"),
                    Some("openrouter"),
                    Some("anthropic/claude-sonnet-4.5"),
                ),
                var,
            )
            .unwrap();
        assert_eq!(run.args, ["-m", "openrouter/anthropic/claude-sonnet-4.5"]);
    }

    #[test]
    fn an_unset_variable_refuses_and_names_it() {
        let t = table("claude.openrouter.env = TOKEN=${OPENROUTER_API_KEY}\n");
        let e = t
            .resolve(&choice(None, Some("openrouter"), None), no_env)
            .unwrap_err();
        assert!(e.contains("`OPENROUTER_API_KEY` is not set"), "{e}");
    }

    #[test]
    fn unknown_names_refuse_and_list_the_known_ones() {
        let t = table("llm = llm\nclaude.bedrock.env = X=1\n");
        let e = t
            .resolve(&choice(Some("gpt"), None, None), no_env)
            .unwrap_err();
        assert_eq!(e, "no agent `gpt` in agents.toml; agents: claude, llm");
        let e = t
            .resolve(&choice(None, Some("openai"), None), no_env)
            .unwrap_err();
        assert_eq!(
            e,
            "agent `claude` has no provider `openai`; providers: bedrock"
        );
        let e = t
            .resolve(&choice(Some("llm"), None, Some("x")), no_env)
            .unwrap_err();
        assert!(e.contains("takes no model"), "{e}");
    }

    #[test]
    fn a_model_outside_the_list_or_shaped_like_a_flag_refuses() {
        let t = table("claude.models = sonnet opus\n");
        assert!(t.resolve(&choice(None, None, Some("opus")), no_env).is_ok());
        let e = t
            .resolve(&choice(None, None, Some("haiku")), no_env)
            .unwrap_err();
        assert!(e.contains("not in claude's models"), "{e}");
        for bad in ["--dangerously-skip-permissions", "a b", "x;rm", ""] {
            assert!(check_model(bad).is_err(), "{bad}");
        }
        assert!(check_model("anthropic/claude-sonnet-4.5:free").is_ok());
    }

    #[test]
    fn bad_lines_are_warned_and_skipped() {
        let (t, warnings) =
            Agents::parse("# a comment\nnonsense\nclaude.colour = red\ntimeout = soon\nx. = 1\n");
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert!(warnings[0].starts_with("agents.toml line 2"));
        assert!(
            warnings[2].contains("timeout is not a number"),
            "{warnings:?}"
        );
        let run = t.resolve(&Choice::default(), no_env).unwrap();
        assert_eq!(run.timeout.as_secs(), DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn an_agents_timeout_reply_and_preamble_and_the_stamp_are_read() {
        let t = table(
            "timeout = 60\nstamp = local\necho = cat\nslow = cat\nslow.timeout = 900\n\
             slow.reply = child\nslow.preamble = Reply in plain text.\n",
        );
        let run = |agent: &str| {
            let choice = Choice {
                agent: Some(agent.to_string()),
                ..Choice::default()
            };
            t.resolve(&choice, no_env).unwrap()
        };
        assert_eq!(run("echo").timeout.as_secs(), 60);
        assert_eq!(run("slow").timeout.as_secs(), 900);
        assert_eq!(run("echo").reply, None);
        assert_eq!(run("slow").reply.as_deref(), Some("child"));
        assert_eq!(
            run("slow").preamble.as_deref(),
            Some("Reply in plain text.")
        );
        assert!(t.local_time());
        let (_, warnings) = Agents::parse("stamp = gmt\necho.reply = insert\necho.timeout = 0\n");
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].contains("utc or local"));
        assert!(warnings[1].contains("append, replace or child"));
    }

    #[test]
    fn confirm_reply_is_never_unless_set_and_an_agent_overrides_the_table() {
        let confirm = |t: &Agents, agent: &str| {
            let choice = Choice {
                agent: Some(agent.to_string()),
                ..Choice::default()
            };
            t.resolve(&choice, no_env).unwrap().confirm_reply
        };
        let t = table("echo = cat\n");
        assert_eq!(confirm(&t, "echo"), ConfirmReply::Never);
        let t =
            table("confirm-reply = replace\necho = cat\nloud = cat\nloud.confirm-reply = always\n");
        assert_eq!(confirm(&t, "echo"), ConfirmReply::Replace);
        assert_eq!(confirm(&t, "loud"), ConfirmReply::Always);
        assert_eq!(confirm(&t, "claude"), ConfirmReply::Replace);
        let (_, warnings) = Agents::parse("confirm-reply = sometimes\n");
        assert!(
            warnings[0].contains("never, replace or always"),
            "{warnings:?}"
        );
        assert!(!ConfirmReply::Replace.asks(false) && ConfirmReply::Replace.asks(true));
    }
}
