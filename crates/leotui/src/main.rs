//! leotui: a terminal front end for leolib.
//!
//!     leotui FILE.leo            edit an outline
//!     leotui FILE.leo --dump     print one composed frame and exit
//!
//! `--dump` renders through the same code the terminal uses, so the view can
//! be exercised in a test or a pipe with no terminal at all.

mod app;
mod keys;
mod ui;

use std::io;

use crossterm::event::{self, Event, KeyEventKind};
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
    width: u16,
    height: u16,
    read_external: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        path: None,
        dump: false,
        width: 100,
        height: 30,
        read_external: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dump" => args.dump = true,
            "--no-external" => args.read_external = false,
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
    "usage: leotui [FILE.leo] [--dump] [--no-external] [--width N] [--height N]".to_string()
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
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
    // Unfold the top level, so an outline opens showing something.
    if let Some(root) = app.outline().root_position() {
        for p in root.self_and_siblings(app.outline()) {
            app.doc.outline.expand(&p);
        }
    }

    let code = if args.dump {
        dump(&mut app, args.width, args.height)
    } else {
        run(&mut app)
    };
    std::process::exit(code);
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

fn run(app: &mut App) -> i32 {
    if let Err(e) = enable_raw_mode() {
        eprintln!("leotui: {e}");
        return 1;
    }
    let mut stdout = io::stdout();
    let _ = execute!(stdout, EnterAlternateScreen);
    let result = event_loop(app, &mut stdout);
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("leotui: {e}");
            1
        }
    }
}

fn event_loop(app: &mut App, stdout: &mut io::Stdout) -> io::Result<()> {
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    terminal.clear()?;
    while !app.quit {
        terminal.draw(|f| ui::draw(f, app))?;
        // Key *press* only: on Windows crossterm also reports releases, which
        // would run every command twice.
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                keys::handle(app, key);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Outline;

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
        // The cursor column marks the selected row.
        assert!(lines[1].starts_with("│>"), "{:?}", lines[1]);
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
