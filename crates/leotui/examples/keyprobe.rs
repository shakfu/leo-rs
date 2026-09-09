//! Print the key events crossterm decodes, until 'q' or 3 seconds of silence.
//!
//! Which of Leo's bindings a terminal can deliver is a measurement, not a
//! guess: Ctrl-I and Tab are the same byte, and modified arrows are not. The
//! table in `docs/dev/tui-design.md` section 2 comes from running this under a
//! pty and feeding it escape sequences.

use std::time::Duration;

use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

fn main() {
    enable_raw_mode().unwrap();
    loop {
        match event::poll(Duration::from_millis(3000)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(k)) => {
                    println!("{:?} {:?} {:?}\r", k.code, k.modifiers, k.kind);
                    if k.code == event::KeyCode::Char('q') {
                        break;
                    }
                }
                Ok(other) => println!("{other:?}\r"),
                Err(e) => {
                    println!("error {e}\r");
                    break;
                }
            },
            Ok(false) => break,
            Err(e) => {
                println!("error {e}\r");
                break;
            }
        }
    }
    disable_raw_mode().unwrap();
}
