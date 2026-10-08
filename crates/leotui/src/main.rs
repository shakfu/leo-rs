//! leotui: a terminal front end for leolib.
//!
//!     leotui FILE.leo            edit an outline
//!     leotui FILE.leo --dump     print one composed frame and exit
//!     leotui --keys              print the binding table and exit
//!     leotui --key-specs         print one binding spec per line and exit
//!     leotui F.leo --no-kitty-keys           plain terminal key decoding
//!     leotui F.leo --dump --press "l,l,F1"   press keys, then dump
//!
//! `--dump` renders through the same code the terminal uses, and `--keys`
//! prints the same table the dispatcher and the help overlay read, so both
//! can be exercised in a test or a pipe with no terminal at all.

mod ui;

use std::io;

use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;

use clap::Parser;
use leoapp::app::{self, App};
use leoapp::{bindings, keys};

/// A terminal front end for leolib.
#[derive(Parser)]
#[command(name = "leotui", version)]
struct Args {
    /// The outline to open. Without one, leotui starts an unsaved outline.
    #[arg(value_name = "FILE.leo")]
    path: Option<String>,
    /// Print one composed frame and exit, with no terminal.
    #[arg(long)]
    dump: bool,
    /// Print the binding table and exit.
    #[arg(long = "keys")]
    list_keys: bool,
    /// Print one binding spec per line and exit.
    #[arg(long = "key-specs")]
    list_key_specs: bool,
    /// Press keys before drawing: binding specs separated by commas. May be repeated.
    #[arg(long, value_name = "KEYS", action = clap::ArgAction::Append)]
    press: Vec<String>,
    /// Decode keys as a plain terminal sends them, without the kitty protocol.
    #[arg(long)]
    no_kitty_keys: bool,
    /// Width of the `--dump` frame.
    #[arg(long, value_name = "N", default_value_t = 100)]
    width: u16,
    /// Height of the `--dump` frame.
    #[arg(long, value_name = "N", default_value_t = 30)]
    height: u16,
    /// Open the outline without reading its external files.
    #[arg(long)]
    no_external: bool,
    /// Use this theme for one launch, without saving it.
    #[arg(long, value_name = "NAME")]
    theme: Option<String>,
}

fn main() {
    // Leo's myLeoSettings.leo, beneath each outline's own @settings.
    leolib::settings::use_user_settings(&leolib::settings::user_settings_path());
    if !leo_plugins::register() {
        eprintln!(
            "leotui: plugins were looked up before they were registered; @qmd and @rmd are off"
        );
    }
    let args = Args::parse();
    if args.list_keys {
        print_keys();
        std::process::exit(0);
    }
    if args.list_key_specs {
        for spec in bindings::normal_mode_specs() {
            println!("{spec}");
        }
        std::process::exit(0);
    }
    let mut app = match app::launch(
        args.path.as_deref(),
        !args.no_external,
        args.theme.as_deref(),
    ) {
        Ok((app, _)) => app,
        Err(e) => {
            eprintln!("leotui: {e}");
            // A source file named by mistake for its outline.
            if matches!(e, leolib::Error::NotALeoFile { .. })
                && !args.path.as_deref().is_some_and(|p| p.ends_with(".leo"))
            {
                eprintln!("leotui opens .leo outlines; to bring in another file, open an outline and use :import-at-file or :import-auto");
            }
            std::process::exit(1);
        }
    };

    // Pressing keys before drawing makes any state reachable headlessly,
    // which is how the help overlay and the modes are checked.
    let specs = args
        .press
        .iter()
        .flat_map(|arg| arg.split(','))
        .map(str::trim);
    for spec in specs {
        for key in keys::parse(spec) {
            app.handle_key(leoapp::keys::KeyEvent::new(key.code, key.mods));
        }
    }

    let code = if args.dump {
        dump(&mut app, args.width, args.height)
    } else {
        run(&mut app, !args.no_kitty_keys)
    };
    std::process::exit(code);
}

/// Print the binding table, grouped by pane. The same data the dispatcher and
/// the help overlay read, so a missing binding shows up here first.
fn print_keys() {
    for (name, focus) in [("outline", app::Focus::Tree), ("body", app::Focus::Body)] {
        println!("# {name} pane");
        for line in leoapp::view::help_lines(focus) {
            println!("{}", line.trim_end());
        }
        println!();
    }
}

/// Render one frame to a buffer and print it. No terminal is involved.
fn dump(app: &mut App, width: u16, height: u16) -> i32 {
    let mut terminal = match Terminal::new(TestBackend::new(width, height)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("leotui: {e}");
            return 1;
        }
    };
    if let Err(e) = terminal.draw(|f| ui::draw(f, app)) {
        eprintln!("leotui: {e}");
        return 1;
    }
    let buffer = terminal.backend().buffer().clone();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x, y)].symbol());
        }
        println!("{}", line.trim_end());
    }
    0
}

/// Owns the terminal's raw mode, alternate screen and keyboard flags.
///
/// Restoring these is not optional: leaving raw mode on, or the enhancement
/// flags pushed, hands the user a shell that no longer understands their
/// keyboard. The guard runs on every exit, including a panic.
struct TerminalGuard {
    kitty_keys: bool,
}

impl TerminalGuard {
    /// Take over the terminal.
    ///
    /// The keyboard enhancement flags are pushed without asking whether the
    /// terminal supports them. `supports_keyboard_enhancement` waits up to two
    /// seconds for a reply and reports an error when none comes, so asking
    /// would cost every terminal that does not support it a two-second stall
    /// at startup. The protocol is designed for this: a terminal that does not
    /// understand the sequence ignores it, and the bindings that depend on it
    /// simply never fire.
    fn new(kitty_keys: bool) -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        // Without bracketed paste a pasted newline is Enter, and the rest of
        // the paste runs as commands. A terminal without either ignores them.
        let _ = execute!(stdout, EnableBracketedPaste, EnableFocusChange);
        if kitty_keys {
            let _ = execute!(
                stdout,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            );
        }
        Ok(Self { kitty_keys })
    }

    /// Undo everything `new` did, in reverse.
    fn restore(kitty_keys: bool) {
        let mut stdout = io::stdout();
        if kitty_keys {
            let _ = execute!(stdout, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(stdout, DisableFocusChange, DisableBracketedPaste);
        let _ = execute!(stdout, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        Self::restore(self.kitty_keys);
    }
}

fn run(app: &mut App, kitty_keys: bool) -> i32 {
    // A panic inside the event loop must not leave the terminal in raw mode
    // and the alternate screen: the message would be invisible and the shell
    // unusable.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        TerminalGuard::restore(kitty_keys);
        previous_hook(info);
    }));

    use std::io::IsTerminal;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!("leotui: needs a terminal; --dump draws one frame to stdout instead");
        return 1;
    }
    let guard = match TerminalGuard::new(kitty_keys) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("leotui: {e}");
            return 1;
        }
    };
    // A hangup (a dropped ssh session) or a kill ends the loop, not the
    // process, so the work can be kept.
    let ended = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    for signal in [signal_hook::consts::SIGHUP, signal_hook::consts::SIGTERM] {
        let _ = signal_hook::flag::register(signal, std::sync::Arc::clone(&ended));
    }
    // The handler replaces dying, so a loop stuck on a dead terminal would
    // never end: once asked, the process goes within five seconds regardless.
    let watched = std::sync::Arc::clone(&ended);
    std::thread::spawn(move || {
        while !watched.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
        std::process::exit(1);
    });
    let result =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| event_loop(app, &ended))) {
            Ok(result) => result,
            // The hook has restored the terminal and printed the panic; keep what
            // the outline holds, then go on dying.
            Err(panic) => {
                drop(guard);
                if let Some(path) = app.write_recovery() {
                    use std::io::Write;
                    let _ = writeln!(
                        io::stderr(),
                        "leotui: unsaved work written to {}",
                        path.display()
                    );
                }
                std::panic::resume_unwind(panic);
            }
        };
    drop(guard);
    let lost = result.is_err() || ended.load(std::sync::atomic::Ordering::SeqCst);
    if lost {
        if let Some(path) = app.write_recovery() {
            // After a hangup stderr may be gone; `eprintln!` would panic.
            use std::io::Write;
            let _ = writeln!(
                io::stderr(),
                "leotui: unsaved work written to {}",
                path.display()
            );
        }
    }
    match result {
        Ok(()) if !lost => 0,
        Ok(()) => 1,
        Err(e) => {
            use std::io::Write;
            let _ = writeln!(io::stderr(), "leotui: {e}");
            1
        }
    }
}

fn event_loop(app: &mut App, ended: &std::sync::atomic::AtomicBool) -> io::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut redraw = true;
    while !app.quit && !ended.load(std::sync::atomic::Ordering::SeqCst) {
        if redraw {
            terminal.draw(|f| ui::draw(f, app))?;
        }
        // Wake to hear from a language server, or when a colouring is due;
        // otherwise a key is the only thing that changes the screen.
        // Never longer than half a second, to see a hangup.
        let check = std::time::Duration::from_millis(500);
        let wake = match app.lsp.is_some() || app.mcp.is_some() {
            true => std::time::Duration::from_millis(50),
            false => app.poll_after().map_or(check, |w| w.min(check)),
        };
        if !event::poll(wake)? {
            redraw = app.poll();
            continue;
        }
        redraw = true;
        // Key *press* only: on Windows crossterm also reports releases, which
        // would run every command twice.
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key_event(key)),
            Event::Paste(text) => {
                app.handle_paste(&text);
                app.log_message();
            }
            Event::FocusGained => {
                app.check_disk();
                app.log_message();
            }
            _ => {}
        }
    }
    Ok(())
}

/// A crossterm key as leoapp names it. Every modifier is kept; a key leoapp
/// has no name for becomes `Null`, which nothing binds.
fn key_event(event: event::KeyEvent) -> leoapp::keys::KeyEvent {
    use event::{KeyCode as C, KeyModifiers as M};
    use leoapp::keys::{KeyCode, KeyModifiers};
    let code = match event.code {
        C::Char(c) => KeyCode::Char(c),
        C::F(n) => KeyCode::F(n),
        C::Enter => KeyCode::Enter,
        C::Esc => KeyCode::Esc,
        C::Tab => KeyCode::Tab,
        C::BackTab => KeyCode::BackTab,
        C::Backspace => KeyCode::Backspace,
        C::Delete => KeyCode::Delete,
        C::Insert => KeyCode::Insert,
        C::Left => KeyCode::Left,
        C::Right => KeyCode::Right,
        C::Up => KeyCode::Up,
        C::Down => KeyCode::Down,
        C::Home => KeyCode::Home,
        C::End => KeyCode::End,
        C::PageUp => KeyCode::PageUp,
        C::PageDown => KeyCode::PageDown,
        _ => KeyCode::Null,
    };
    let mut mods = KeyModifiers::NONE;
    for (from, to) in [
        (M::SHIFT, KeyModifiers::SHIFT),
        (M::CONTROL, KeyModifiers::CONTROL),
        (M::ALT, KeyModifiers::ALT),
        (M::SUPER, KeyModifiers::SUPER),
        (M::HYPER, KeyModifiers::HYPER),
        (M::META, KeyModifiers::META),
    ] {
        if event.modifiers.contains(from) {
            mods |= to;
        }
    }
    leoapp::keys::KeyEvent::new(code, mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use leoapp::keys::{KeyCode, KeyEvent, KeyModifiers};
    use leolib::{Document, Outline};
    use ratatui::style::Color;

    #[test]
    fn a_crossterm_key_converts_with_every_modifier() {
        use crossterm::event::{KeyCode as C, KeyEvent as E, KeyModifiers as M};
        let k = key_event(E::new(C::Char('z'), M::CONTROL | M::SHIFT | M::SUPER));
        assert_eq!(k.code, KeyCode::Char('z'));
        assert_eq!(
            k.modifiers,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT | KeyModifiers::SUPER
        );
        // A key with no leoapp name still arrives, so it clears the message.
        let k = key_event(E::new(C::CapsLock, M::NONE));
        assert_eq!(k, KeyEvent::new(KeyCode::Null, KeyModifiers::NONE));
    }

    /// Render one frame and return its lines, as `--dump` prints them.
    fn render(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn the_frame_shows_the_outline_and_the_body() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        o.set_body(&root, "the body text\n");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "child");
        o.expand(&root);
        let mut app = App::new(Document::new(o));

        let lines = render(&mut app, 60, 8);
        let text = lines.join("\n");
        assert!(text.contains("root"), "{text}");
        assert!(text.contains("child"), "{text}");
        assert!(text.contains("the body text"), "{text}");
        // The cursor column marks the selected row, inside the focused
        // pane's thick border.
        assert!(
            lines.iter().any(|l| l.starts_with("\u{2503}>")),
            "no selected row: {lines:#?}"
        );
        // The mode is named, and the breadcrumb is above the panes.
        assert!(text.contains("NORMAL"), "{text}");
        assert!(lines[0].contains("root"), "{:?}", lines[0]);
    }

    /// Press a sequence of binding specs, as `--press` does.
    fn press(app: &mut App, specs: &[&str]) {
        for spec in specs {
            for key in keys::parse(spec) {
                app.handle_key(KeyEvent::new(key.code, key.mods));
            }
        }
    }

    #[test]
    fn the_help_overlay_lists_the_bindings_it_dispatches() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        let mut app = App::new(Document::new(o));
        press(&mut app, &["F1"]);
        let text = render(&mut app, 100, 20).join("\n");
        assert!(text.contains("HELP"), "{text}");
        assert!(text.contains("goto-next-visible"), "{text}");
        assert!(text.contains("q closes"), "{text}");
    }

    #[test]
    fn the_focused_pane_is_the_one_with_the_thick_border() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        let mut app = App::new(Document::new(o));
        let tree_focused = render(&mut app, 60, 8);
        assert!(
            tree_focused[1].starts_with('\u{250f}'),
            "{:?}",
            tree_focused[1]
        );
        press(&mut app, &["Tab"]);
        let body_focused = render(&mut app, 60, 8);
        assert!(
            body_focused[1].starts_with('\u{250c}'),
            "{:?}",
            body_focused[1]
        );
        assert!(
            body_focused[1].contains('\u{250f}'),
            "{:?}",
            body_focused[1]
        );
    }

    #[test]
    fn the_key_listing_covers_both_panes() {
        let tree = leoapp::view::help_lines(app::Focus::Tree).join("\n");
        let body = leoapp::view::help_lines(app::Focus::Body).join("\n");
        assert!(tree.contains("move-outline-down"), "{tree}");
        assert!(!tree.contains("body-operators"), "{tree}");
        assert!(body.contains("body-operators"), "{body}");
        // An outline-only command is not in the body's listing. Leo's
        // Alt-Shift arrows move a node from either pane, so this is gp.
        assert!(!body.contains("goto-parent"), "{body}");
        // Bindings that apply to both panes appear in both listings.
        assert!(tree.contains("undo") && body.contains("undo"));
    }

    #[test]
    fn completions_drop_down_above_the_status_line() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_body(&root, "pr\n");
        let mut app = App::new(Document::new(o));
        app.focus = app::Focus::Body;
        app.begin_body_edit();
        let item = |label: &str| app::Completion {
            label: label.into(),
            detail: Some(format!("def {label}()")),
            kind: None,
            text: label.into(),
            range: None,
        };
        app.completion = Some(app::CompletionMenu {
            items: vec![item("print"), item("property")],
            shown: vec![0, 1],
            selected: 1,
            start: (0, 0),
        });
        let lines = render(&mut app, 60, 10);
        let screen = lines.join("\n");
        assert!(screen.contains("2 completions; Tab takes one"), "{screen}");
        assert!(screen.contains("property  def property()"), "{screen}");
        // Just above the status line.
        assert!(lines[8].contains("\u{2570}"), "{screen}");
    }

    /// The background of the last row, which is the status line.
    fn status_backgrounds(app: &mut App, width: u16, height: u16) -> Vec<Color> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..width).map(|x| buffer[(x, height - 1)].bg).collect()
    }

    #[test]
    fn a_diagnostic_is_underlined_in_its_severitys_colour() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_body(&root, "x = nope\n");
        let mut app = App::new(Document::new(o));
        app.focus = app::Focus::Body;
        app.diagnostics = vec![leoapp::view::BodyDiagnostic {
            row: 0,
            col: 4,
            end_row: 0,
            end_col: 8,
            severity: leoapp::view::Severity::Error,
            message: "undefined: nope".into(),
        }];
        let (width, height) = (60, 8);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row: Vec<_> = (0..width).map(|x| buffer[(x, 2)].clone()).collect();
        let start = row.iter().position(|c| c.symbol() == "x").unwrap();
        let marked = |i: usize| {
            let c = &row[start + i];
            c.fg == Color::Red && c.modifier.contains(ratatui::style::Modifier::UNDERLINED)
        };
        assert!((4..8).all(marked), "`nope` is not marked");
        assert!(!(0..4).any(marked), "`x = ` is marked");
        // The status line names it while the cursor is on its line.
        let status: String = (0..width)
            .map(|x| buffer[(x, height - 1)].symbol().to_string())
            .collect();
        assert!(status.contains("E: undefined: nope"), "{status}");
    }

    #[test]
    fn the_command_line_is_not_painted_like_the_status_bar() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        let mut app = App::new(Document::new(o));

        // The status line is a readout, and carries the bar's colours.
        let status = status_backgrounds(&mut app, 60, 8);
        assert!(
            status.iter().any(|c| *c != Color::Reset),
            "the status line lost its background"
        );

        // The `:` line is being typed into, so it is plain.
        press(&mut app, &[":"]);
        let command = status_backgrounds(&mut app, 60, 8);
        assert!(
            command.iter().all(|c| *c == Color::Reset),
            "the command line is painted: {command:?}"
        );

        // So are the other lines that take input.
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        for spec in ["/", "e"] {
            press(&mut app, &[spec]);
            let line = status_backgrounds(&mut app, 60, 8);
            assert!(
                line.iter().all(|c| *c == Color::Reset),
                "the {spec} line is painted: {line:?}"
            );
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        }
    }

    /// The foreground colours of one rendered row, left to right.
    fn row_colours(app: &mut App, width: u16, height: u16, row: u16) -> Vec<(String, Color)> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out: Vec<(String, Color)> = Vec::new();
        for x in 0..width {
            let cell = &buffer[(x, row)];
            // Pane borders share DarkGray with comments, and would merge into
            // the run beside them.
            if cell
                .symbol()
                .chars()
                .all(|c| ('\u{2500}'..='\u{257f}').contains(&c))
            {
                out.push((String::new(), Color::Reset));
                continue;
            }
            match out.last_mut() {
                Some((text, colour)) if *colour == cell.fg => text.push_str(cell.symbol()),
                _ => out.push((cell.symbol().to_string(), cell.fg)),
            }
        }
        out.into_iter()
            .map(|(t, c)| (t.trim().to_string(), c))
            .filter(|(t, _)| !t.is_empty())
            .collect()
    }

    /// An app showing one node whose body is `text`, in a `.py` file.
    fn highlighted(text: &str) -> App {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file demo.py");
        o.set_body(&root, text);
        App::new(Document::new(o))
    }

    /// The frame's rows as plain text, so an overlay can be looked for.
    fn frame_text(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    /// An app with `:` open, a fixed theme list, and `text` typed.
    fn with_theme_line(text: &str) -> App {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file demo.py");
        o.set_body(&root, "x = 1\n");
        let mut app = App::new(Document::new(o));
        app.handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE));
        // A list of its own, so the test does not depend on what is installed.
        app.theme_names = std::rc::Rc::new(
            ["onedark", "onedarker", "onelight"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        for ch in text.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app
    }

    #[test]
    fn the_theme_names_are_listed_above_the_command_line() {
        let mut app = with_theme_line("theme o");
        let rows = frame_text(&mut app, 60, 14);
        let joined = rows.join("\n");
        assert!(joined.contains("onedarker"), "no drop-down:\n{joined}");
        assert!(joined.contains("3 themes"), "no count:\n{joined}");
        // The line being typed is still the bottom row.
        assert!(rows.last().unwrap().starts_with(":theme o"));
    }

    #[test]
    fn a_command_name_gets_no_drop_down() {
        // A list over the whole command table would cover the outline every
        // time `:` is pressed.
        let mut app = with_theme_line("goto");
        let joined = frame_text(&mut app, 60, 14).join("\n");
        assert!(!joined.contains("themes"), "{joined}");
    }

    #[test]
    fn tab_highlights_the_name_it_lands_on() {
        let mut app = with_theme_line("theme o");
        // The first Tab takes the common prefix; the second takes a match.
        for _ in 0..2 {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let highlighted: String = (0..14)
            .flat_map(|y| (0..60).map(move |x| (x, y)))
            .filter(|&(x, y)| buffer[(x, y)].bg == Color::Blue)
            .map(|(x, y)| buffer[(x, y)].symbol().to_string())
            .collect();
        assert!(
            highlighted.contains("onedark"),
            "nothing was highlighted: {highlighted:?}"
        );
    }

    #[test]
    fn search_matches_are_highlighted_in_both_panes() {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        doc.set_headline(&root, "find the needle");
        doc.set_body(&root, "a needle here\n");
        let mut app = App::new(doc);
        app.hlsearch = Some(regex::Regex::new("needle").unwrap());
        // Wide enough that the outline pane shows the whole headline.
        let mut terminal = Terminal::new(TestBackend::new(100, 8)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let marked: String = (0..8)
            .flat_map(|y| (0..100).map(move |x| (x, y)))
            .filter(|&(x, y)| buffer[(x, y)].bg == Color::Yellow)
            .map(|(x, y)| buffer[(x, y)].symbol().to_string())
            .collect();
        // The headline's and the body's, on the same screen row.
        assert_eq!(marked, "needleneedle");
    }

    #[test]
    fn the_body_is_coloured_by_the_language_of_the_node() {
        // The language comes from the @file extension: leolib's four-pass rule.
        let mut app = highlighted("def f():\n    # note\n");
        let first = row_colours(&mut app, 60, 8, 2);
        assert!(
            first.iter().any(|(t, c)| t == "def" && *c == Color::Yellow),
            "`def` was not coloured as a keyword: {first:?}"
        );
        let second = row_colours(&mut app, 60, 8, 3);
        assert!(
            second
                .iter()
                .any(|(t, c)| t == "# note" && *c == Color::DarkGray),
            "the comment was not coloured: {second:?}"
        );
    }

    #[test]
    fn a_parse_tree_reaches_the_screen() {
        // The colour a table lookup cannot produce: `f` is a function because
        // of where it sits, not because it is in a keyword list.
        let mut app = highlighted("def f(self):\n    return self.n\n");
        let first = row_colours(&mut app, 60, 8, 2);
        assert!(
            first
                .iter()
                .any(|(t, c)| t == "f" && *c == Color::LightBlue),
            "`f` was not coloured as a function: {first:?}"
        );
        let second = row_colours(&mut app, 60, 8, 3);
        assert!(
            second.iter().any(|(t, c)| t == "n" && *c == Color::Gray),
            "`n` was not coloured as a field: {second:?}"
        );
    }

    #[test]
    fn an_at_language_line_changes_the_colouring_below_it() {
        // The point of the feature: `#` is a comment above the second
        // directive and `//` is one below it, in the same node.
        //
        // Two directives, because Leo's `scanLanguageDirectives` -- and
        // leolib's `get_language` -- resolve a node to the *first* @language
        // in its body. That first one is where the colouring starts; later
        // ones move it.
        let mut app = highlighted("@language python\n# python\n@language c\n// c\n");
        let python = row_colours(&mut app, 60, 9, 3);
        assert!(
            python
                .iter()
                .any(|(t, c)| t == "# python" && *c == Color::DarkGray),
            "{python:?}"
        );
        let directive = row_colours(&mut app, 60, 9, 4);
        assert!(
            directive
                .iter()
                .any(|(t, c)| t == "@language c" && *c == Color::Magenta),
            "{directive:?}"
        );
        let c = row_colours(&mut app, 60, 9, 5);
        assert!(
            c.iter()
                .any(|(t, col)| t == "// c" && *col == Color::DarkGray),
            "the language did not switch: {c:?}"
        );
    }

    #[test]
    fn a_node_nothing_declares_a_language_for_is_left_plain() {
        // The scope rule: `@language` reaches the node that holds it and its
        // descendants, and nothing else. Without it a prose node was coloured
        // as Python, so `class` was a keyword and `It's` opened a string.
        let mut o = Outline::new_empty();
        let prose = o.root_position().unwrap();
        o.set_headline(&prose, "Notes");
        o.set_body(&prose, "Ideas for the class, not code.\n");
        let code = o.insert_after(&prose);
        o.set_headline(&code, "Code");
        o.set_body(&code, "@language python\ndef f(): pass\n");
        let child = o.insert_as_last_child(&code);
        o.set_headline(&child, "a child");
        o.set_body(&child, "class C: pass\n");
        o.expand(&code);
        let mut app = App::new(Document::new(o));

        // The whole line is one uncoloured run: no keyword, no string.
        let plain = row_colours(&mut app, 70, 8, 2);
        assert!(
            plain
                .iter()
                .any(|(t, c)| t == "Ideas for the class, not code." && *c == Color::Reset),
            "prose was coloured: {plain:?}"
        );

        // Its sibling declares one, so it and its child are coloured.
        press(&mut app, &["j"]);
        let declared = row_colours(&mut app, 70, 8, 3);
        assert!(
            declared
                .iter()
                .any(|(t, c)| t == "def" && *c == Color::Yellow),
            "the declaring node was not coloured: {declared:?}"
        );
        press(&mut app, &["j"]);
        let inherited = row_colours(&mut app, 70, 8, 2);
        assert!(
            inherited
                .iter()
                .any(|(t, c)| t == "class" && *c == Color::Yellow),
            "the child did not inherit: {inherited:?}"
        );
    }

    #[test]
    fn set_nosyntax_leaves_the_body_plain() {
        let mut app = highlighted("def f():\n");
        let coloured = row_colours(&mut app, 60, 8, 2);
        assert!(coloured
            .iter()
            .any(|(t, c)| t == "def" && *c == Color::Yellow));
        app.run_command_line("set nosyntax");
        let plain = row_colours(&mut app, 60, 8, 2);
        assert!(
            plain
                .iter()
                .any(|(t, c)| t == "def f():" && *c == Color::Reset),
            "the body is still coloured: {plain:?}"
        );
    }

    #[test]
    fn a_narrow_terminal_truncates_rather_than_wrapping() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, &"x".repeat(200));
        let mut app = App::new(Document::new(o));
        let lines = render(&mut app, 40, 6);
        assert!(lines.iter().all(|l| l.chars().count() <= 40));
    }

    /// An app over one node whose body is `text`.
    fn body_of(text: &str) -> App {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        o.set_body(&root, text);
        App::new(Document::new(o))
    }

    #[test]
    fn set_wrap_folds_a_long_line_onto_the_next_rows() {
        let body = format!("{}END\n", "a".repeat(60));
        let mut app = body_of(&body);
        assert!(!render(&mut app, 40, 8).join("\n").contains("END"));
        app.options.wrap = true;
        let text = render(&mut app, 40, 8).join("\n");
        assert!(text.contains("END"), "{text}");
    }

    #[test]
    fn the_body_slides_sideways_to_keep_the_cursor_on_screen() {
        let body = format!("{}END\n", "a".repeat(60));
        let mut app = body_of(&body);
        press(&mut app, &["Tab", "$"]);
        let text = render(&mut app, 40, 8).join("\n");
        assert!(text.contains("END"), "{text}");
    }

    /// The terminal cursor's column after pressing `keys` in the body.
    fn cursor_x(body: &str, keys: &[&str]) -> u16 {
        let mut app = body_of(body);
        press(&mut app, &["Tab"]);
        press(&mut app, keys);
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        terminal.get_cursor_position().unwrap().x
    }

    #[test]
    fn the_body_cursor_counts_screen_columns() {
        // A tab runs to the next stop of @tabwidth; a CJK character is two wide.
        let start = cursor_x("\tx\n", &[]);
        assert_eq!(cursor_x("\tx\n", &["l"]) - start, 4);
        assert_eq!(cursor_x("ab\tx\n", &["l", "l", "l"]) - start, 4);
        assert_eq!(cursor_x("\u{4e2d}x\n", &["l"]) - start, 2);
    }

    #[test]
    fn no_color_draws_no_colour_and_reverses_the_selected_row() {
        let mut app = body_of("text\n");
        app.depth = leoapp::theme::Depth::None;
        let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
        terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        assert!(buffer
            .content
            .iter()
            .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
        let reversed = |y: u16| {
            (0..40).any(|x| {
                buffer[(x, y)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED)
            })
        };
        // The selected row and the status line.
        assert!(reversed(2) && reversed(5));
        assert!(!reversed(3));
    }
}
