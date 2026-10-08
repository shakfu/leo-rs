//! The entangled plugin: `:entangled-tangle` and `:entangled-check` run
//! entangled's own command in the outline's directory, so tangling follows
//! `entangled.toml` exactly. Phase 2 of `docs/dev/entangled_leo_backend.md`.
//! Built with the `leoapp` feature.
//!
//! The command runs on a thread of its own and `poll` collects it, so a slow
//! run does not stop the window drawing.

use std::process::{Command, Output};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use leolib::Position;

use leoapp::app::App;
use leoapp::commands::Command as AppCommand;
use leoapp::plugins::{AppPlugin, MenuEntry};

/// The plugin.
pub struct Entangled;

impl AppPlugin for Entangled {
    fn name(&self) -> &'static str {
        "entangled"
    }
    fn commands(&self) -> &'static [AppCommand] {
        &COMMANDS
    }
    fn with_argument(&self, name: &str) -> Option<fn(&mut App, &str)> {
        // Arguments go to entangled: `:entangled-tangle --force`.
        match name {
            "entangled-tangle" => Some(|app, args| run(app, "tangle", args)),
            "entangled-check" => Some(|app, args| run(app, "check", args)),
            _ => None,
        }
    }
    fn settings(&self) -> &'static [&'static str] {
        // `entangled = "/usr/local/bin/entangled"`: the command run, split on
        // blanks. `entangled` when unset.
        &["entangled"]
    }
    fn menu(&self) -> &'static [MenuEntry] {
        &MENU
    }
    fn poll(&self, app: &mut App) -> bool {
        poll(app)
    }
    fn poll_after(&self, app: &App) -> Option<Duration> {
        app.plugin_data_ref::<State>()
            .and_then(|s| s.job.as_ref())
            .map(|_| JOB_POLL)
    }
}

static COMMANDS: [AppCommand; 2] = [
    AppCommand {
        name: "entangled-tangle",
        summary: "write the @entangled files, then run `entangled tangle`",
        run: |app, _| run(app, "tangle", ""),
    },
    AppCommand {
        name: "entangled-check",
        summary: "write the @entangled files, then run `entangled check`",
        run: |app, _| run(app, "check", ""),
    },
];

static MENU: [MenuEntry; 2] = [
    MenuEntry {
        menu: "Body",
        label: "Tangle with entangled",
        command: "entangled-tangle",
    },
    MenuEntry {
        menu: "Body",
        label: "Check with entangled",
        command: "entangled-check",
    },
];

/// The plugin's state in the app: a run, until its result is collected.
#[derive(Default)]
struct State {
    job: Option<Job>,
}

/// A run of the entangled command.
struct Job {
    /// What the status line calls it: `entangled tangle`.
    name: String,
    /// The program, to name it if it is missing.
    program: String,
    receive: Receiver<std::io::Result<Output>>,
}

/// How soon a front end should look again while a run is going.
const JOB_POLL: Duration = Duration::from_millis(50);

/// Run `entangled SUB ARGS` in the outline's directory, after writing any
/// unsaved `@entangled` file, so entangled reads what the outline holds.
pub fn run(app: &mut App, sub: &str, args: &str) {
    if let Some(job) = &app.plugin_data::<State>().job {
        app.message = format!("{} is still running", job.name);
        return;
    }
    if !write_entangled_files(app) {
        return;
    }
    let command = app
        .settings
        .plugin
        .get("entangled")
        .cloned()
        .unwrap_or_else(|| "entangled".to_string());
    let mut words = command.split_whitespace();
    let Some(program) = words.next().map(str::to_string) else {
        app.message = "the entangled setting is empty".to_string();
        return;
    };
    let mut cmd = Command::new(&program);
    // Its log lines are coloured for a terminal; the log here is not one.
    cmd.args(words)
        .arg(sub)
        .args(args.split_whitespace())
        .current_dir(project_dir(app))
        .env("NO_COLOR", "1");
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(cmd.output());
    });
    let name = format!("entangled {sub}");
    app.message = format!("{name}: running");
    app.plugin_data::<State>().job = Some(Job {
        name,
        program,
        receive,
    });
}

/// Write the unsaved `@entangled` files. False, and why on the status line,
/// if one could not be written: entangled would read the old text.
fn write_entangled_files(app: &mut App) -> bool {
    let o = app.outline();
    let mut seen = std::collections::HashSet::new();
    let files: Vec<Position> = o
        .all_positions()
        .into_iter()
        .filter(|p| crate::is_entangled(p.h(o)) && p.is_dirty(o))
        .filter(|p| seen.insert(p.v))
        .collect();
    if files.is_empty() {
        return true;
    }
    let result = app.doc.write_files(files);
    for path in &result.written {
        app.log(format!("wrote {path}"));
    }
    match result.errors.first() {
        None => true,
        Some(e) => {
            app.message = format!("not run: {} was not written: {}", e.headline, e.error);
            false
        }
    }
}

/// The directory the outline's file is in, where `entangled.toml` is looked
/// for; the current directory for an unsaved outline.
fn project_dir(app: &App) -> std::path::PathBuf {
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

/// Collect a finished run: its output in `:messages`, a summary on the status
/// line. True if one finished.
fn poll(app: &mut App) -> bool {
    let Some(job) = &app.plugin_data::<State>().job else {
        return false;
    };
    let result = match job.receive.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return false,
        Err(TryRecvError::Disconnected) => Err(std::io::Error::other("the run stopped")),
    };
    let job = app.plugin_data::<State>().job.take().expect("checked");
    app.message = match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => format!(
            "{}: no `{}` command found; set `entangled` in the settings to its full path",
            job.name, job.program
        ),
        Err(e) => format!("{}: {e}", job.name),
        Ok(out) => {
            let stdout = strip_ansi(&String::from_utf8_lossy(&out.stdout));
            let stderr = strip_ansi(&String::from_utf8_lossy(&out.stderr));
            let lines: Vec<&str> = stdout
                .lines()
                .chain(stderr.lines())
                .filter(|l| !l.trim().is_empty())
                .collect();
            for line in &lines {
                app.log(format!("{}: {line}", job.name));
            }
            match out.status.success() {
                true => match lines.last() {
                    Some(last) => format!("{}: {last}", job.name),
                    None => format!("{}: done", job.name),
                },
                false => {
                    let why = stderr
                        .lines()
                        .chain(stdout.lines())
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("no output");
                    let code = out
                        .status
                        .code()
                        .map_or_else(|| "a signal".to_string(), |c| format!("exit {c}"));
                    format!(
                        "{} failed ({code}): {why}; :messages has the output",
                        job.name
                    )
                }
            }
        }
    };
    true
}

/// `text` without terminal colour codes, which a program may write even
/// when told not to.
fn strip_ansi(text: &str) -> String {
    static ANSI: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap());
    ANSI.replace_all(text, "").to_string()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use leolib::Document;

    /// Offer the plugin and the kind, as `leo-plugins` does at startup.
    fn register() -> leolib::ext::Kinds {
        leoapp::plugins::register(vec![&Entangled]);
        let kinds = leolib::ext::Kinds::empty().with(crate::Entangled).unwrap();
        leoapp::plugins::register_kinds(kinds.clone());
        kinds
    }

    /// An app whose entangled command is `command`, over an empty outline in
    /// a directory of its own.
    fn app(command: &str, dir: &std::path::Path) -> App {
        register();
        let leo = dir.join("doc.leo").to_string_lossy().to_string();
        let mut app = App::new(Document::new_empty(&leo));
        app.settings
            .plugin
            .insert("entangled".to_string(), command.to_string());
        app
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("leoapp-entangled-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Poll until the run is collected.
    fn finish(app: &mut App) {
        let start = std::time::Instant::now();
        while !app.poll() {
            assert!(start.elapsed().as_secs() < 10, "the run never finished");
            assert!(app.poll_after().is_some(), "a front end would not wake");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_output_goes_to_the_log_and_its_last_line_to_the_status_line() {
        let dir = scratch("echo");
        let mut app = app("echo", &dir);
        run(&mut app, "tangle", "--force");
        assert_eq!(app.message, "entangled tangle: running");
        finish(&mut app);
        assert_eq!(app.message, "entangled tangle: tangle --force");
        assert_eq!(
            app.messages.last().unwrap(),
            "entangled tangle: tangle --force"
        );
        assert!(app.poll_after().is_none());
    }

    #[test]
    fn it_runs_in_the_outlines_directory() {
        let dir = scratch("pwd");
        let mut app = app("sh -c pwd", &dir);
        run(&mut app, "check", "");
        finish(&mut app);
        let here = std::fs::canonicalize(&dir).unwrap();
        let ran = app
            .message
            .trim_start_matches("entangled check: ")
            .to_string();
        assert_eq!(std::fs::canonicalize(ran).unwrap(), here);
    }

    #[test]
    fn a_failure_and_a_missing_command_are_said() {
        let dir = scratch("fail");
        let mut app = app("false", &dir);
        run(&mut app, "tangle", "");
        finish(&mut app);
        assert!(
            app.message.starts_with("entangled tangle failed (exit 1)"),
            "{}",
            app.message
        );

        let mut app = self::app("no-such-entangled-command", &dir);
        run(&mut app, "tangle", "");
        finish(&mut app);
        assert!(
            app.message
                .contains("no `no-such-entangled-command` command found"),
            "{}",
            app.message
        );
    }

    #[test]
    fn colour_codes_are_taken_out_of_the_output() {
        let dir = scratch("ansi");
        let mut app = app("printf \\033[32mTangled\\033[0m%s", &dir);
        run(&mut app, "tangle", "");
        finish(&mut app);
        assert_eq!(app.message, "entangled tangle: Tangledtangle");
    }

    #[test]
    fn a_second_run_waits_for_the_first() {
        let dir = scratch("busy");
        let mut app = app("sleep 1", &dir);
        run(&mut app, "tangle", "");
        run(&mut app, "check", "");
        assert_eq!(app.message, "entangled tangle is still running");
    }

    #[test]
    fn an_edited_entangled_file_is_written_before_entangled_reads_it() {
        let dir = scratch("write");
        std::fs::write(dir.join("doc.md"), "# A\n\n```python #add\nx = 1\n```\n").unwrap();
        let leo = dir.join("doc.leo");
        std::fs::write(
            &leo,
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n\
             <vnodes>\n<v t=\"a.1\"><vh>@entangled doc.md</vh></v>\n</vnodes>\n\
             <tnodes>\n<t tx=\"a.1\"></t>\n</tnodes>\n</leo_file>\n",
        )
        .unwrap();
        let doc = Document::open_with(&leo.to_string_lossy(), true, register()).unwrap();
        let mut app = App::new(doc);
        app.settings
            .plugin
            .insert("entangled".to_string(), "true".to_string());
        let add = app
            .outline()
            .all_positions()
            .into_iter()
            .find(|p| p.h(app.outline()) == "<< add >>")
            .unwrap();
        app.doc.set_body(&add, "x = 2\n");
        run(&mut app, "tangle", "");
        assert_eq!(
            std::fs::read_to_string(dir.join("doc.md")).unwrap(),
            "# A\n\n```python #add\nx = 2\n```\n"
        );
        finish(&mut app);
        assert_eq!(app.message, "entangled tangle: done");
    }
}
