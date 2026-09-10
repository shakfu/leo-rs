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

mod app;
mod bindings;
mod commands;
mod config;
mod editor;
mod highlight;
mod keys;
mod keywords;
mod minibuffer;
mod search;
mod theme;
mod treesit;
mod ui;

use std::io;

use crossterm::event::{
    self, Event, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;

use app::App;
use leolib::Document;

struct Args {
    path: Option<String>,
    dump: bool,
    list_keys: bool,
    list_key_specs: bool,
    press: Vec<String>,
    kitty_keys: bool,
    width: u16,
    height: u16,
    read_external: bool,
    theme: Option<String>,
}

/// The theme loaded when neither `--theme` nor the config file names one.
///
/// A Helix theme name: leotui reads their files and vendors none, so this is
/// only a default and holds whatever the user has on disk under that name.
const DEFAULT_THEME: &str = "sonokai";

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        path: None,
        dump: false,
        list_keys: false,
        list_key_specs: false,
        press: Vec::new(),
        kitty_keys: true,
        width: 100,
        height: 30,
        read_external: true,
        theme: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dump" => args.dump = true,
            "--keys" => args.list_keys = true,
            "--key-specs" => args.list_key_specs = true,
            "--press" => {
                let spec = it.next().ok_or("--press needs a key sequence")?;
                args.press = spec.split(',').map(|s| s.trim().to_string()).collect();
            }
            "--no-external" => args.read_external = false,
            "--theme" => args.theme = Some(it.next().ok_or("--theme needs a name")?),
            "--no-kitty-keys" => args.kitty_keys = false,
            "--width" => {
                args.width = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or("--width needs a number")?
            }
            "--height" => {
                args.height = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or("--height needs a number")?
            }
            "-h" | "--help" => return Err(usage()),
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}\n{}", usage())),
            _ => args.path = Some(arg),
        }
    }
    Ok(args)
}

fn usage() -> String {
    "usage: leotui [FILE.leo] [--dump] [--keys] [--key-specs] [--press KEYS] \
[--no-external] [--no-kitty-keys] [--width N] [--height N] [--theme NAME]"
        .to_string()
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
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
    let doc = match &args.path {
        Some(path) => match Document::open(path, args.read_external) {
            Ok(doc) => doc,
            Err(e) => {
                eprintln!("leotui: {e}");
                std::process::exit(1);
            }
        },
        None => Document::new_empty(""),
    };
    let mut app = App::new(doc);
    app.config_path = config::path();
    // A theme asked for by name, on the command line or in the config file,
    // reports when it is missing. The default does not: the built-in sixteen
    // colours stand, and saying so on every start would be noise.
    let settings = config::load();
    let (theme, asked) = config::chosen_theme(args.theme.as_deref(), &settings, DEFAULT_THEME);
    if !app.set_theme(theme) && !asked {
        app.message.clear();
    }
    if app.message.is_empty() {
        if let Some(warning) = settings.warnings.first() {
            app.message = warning.clone();
        }
    }
    // A file that could not be read outranks a theme or a settings message:
    // its node is empty, and would otherwise pass for the file's contents.
    if let Some(report) = app::read_report_message(&app.doc.read_report) {
        app.message = report;
    }

    // Unfold the top level, so an outline opens showing something.
    if let Some(root) = app.outline().root_position() {
        for p in root.self_and_siblings(app.outline()) {
            app.doc.outline.expand(&p);
        }
    }

    // Pressing keys before drawing makes any state reachable headlessly,
    // which is how the help overlay and the modes are checked.
    for spec in &args.press {
        for key in keys::parse(spec) {
            app.handle_key(crossterm::event::KeyEvent::new(key.code, key.mods));
        }
    }

    let code = if args.dump {
        dump(&mut app, args.width, args.height)
    } else {
        run(&mut app, args.kitty_keys)
    };
    std::process::exit(code);
}

/// Print the binding table, grouped by pane. The same data the dispatcher and
/// the help overlay read, so a missing binding shows up here first.
fn print_keys() {
    for (name, focus) in [("outline", app::Focus::Tree), ("body", app::Focus::Body)] {
        println!("# {name} pane");
        for line in ui::help_lines(focus) {
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

    let guard = match TerminalGuard::new(kitty_keys) {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("leotui: {e}");
            return 1;
        }
    };
    let result = event_loop(app);
    drop(guard);
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("leotui: {e}");
            1
        }
    }
}

fn event_loop(app: &mut App) -> io::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    while !app.quit {
        terminal.draw(|f| ui::draw(f, app))?;
        // Key *press* only: on Windows crossterm also reports releases, which
        // would run every command twice.
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                app.handle_key(key);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use leolib::Outline;
    use ratatui::style::Color;

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
        let tree = ui::help_lines(app::Focus::Tree).join("\n");
        let body = ui::help_lines(app::Focus::Body).join("\n");
        assert!(tree.contains("move-outline-down"), "{tree}");
        assert!(!tree.contains("body-operators"), "{tree}");
        assert!(body.contains("body-operators"), "{body}");
        assert!(!body.contains("move-outline-down"), "{body}");
        // Bindings that apply to both panes appear in both listings.
        assert!(tree.contains("undo") && body.contains("undo"));
    }

    /// The background of the last row, which is the status line.
    fn status_backgrounds(app: &mut App, width: u16, height: u16) -> Vec<Color> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| ui::draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..width).map(|x| buffer[(x, height - 1)].bg).collect()
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
        app.handle_key(KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ));
        for spec in ["/", "e"] {
            press(&mut app, &[spec]);
            let line = status_backgrounds(&mut app, 60, 8);
            assert!(
                line.iter().all(|c| *c == Color::Reset),
                "the {spec} line is painted: {line:?}"
            );
            app.handle_key(KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE,
            ));
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
}
