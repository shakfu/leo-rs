//! egui's events as leoapp's keys.
//!
//! egui-winit reports a printable key twice, as `Event::Key` and then as
//! `Event::Text`, except while Ctrl or Cmd is held, when it sends no text.
//! It also turns Cmd-C, Cmd-X and Cmd-V (Ctrl elsewhere) into `Copy`, `Cut`
//! and `Paste`, with no key event, Shift or not. So: a bare printable key is
//! taken from its text, a chord from its key, and the three clipboard events
//! become the chords they replaced: with Shift, Leo's copy-node, cut-node
//! and paste-node.
//!
//! On macOS, Cmd is sent as SUPER, which leoapp reads as Leo's Ctrl: on a
//! Mac, Leo's Ctrl is the Cmd key. Control stays leoapp's Ctrl, vim's.

use eframe::egui::{self, Event, ImeEvent, Key};
use leoapp::keys::{KeyCode, KeyEvent, KeyModifiers};

/// What one egui event asks of the app.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Key(KeyEvent),
    Text(String),
    Paste(String),
    /// Text an IME is composing, drawn at the cursor and not yet typed.
    /// Empty when composition ends.
    Preedit(String),
    Focused,
}

pub struct Translator {
    /// macOS: Cmd is Leo's Ctrl. False under
    /// `qt-mac-dont-swap-ctrl-and-meta`, when Cmd is Meta and binds nothing.
    pub cmd_is_ctrl: bool,
    /// An Alt chord was sent as a key; the text egui sends after it (Option-g
    /// types a symbol on macOS) is dropped.
    suppress_text: bool,
}

impl Translator {
    pub fn new(cmd_is_ctrl: bool) -> Self {
        Self {
            cmd_is_ctrl,
            suppress_text: false,
        }
    }

    /// `held` is the modifiers down when the event came, which a clipboard
    /// event does not carry.
    pub fn translate(&mut self, event: &Event, held: egui::Modifiers) -> Vec<Action> {
        let suppress = std::mem::take(&mut self.suppress_text);
        match event {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let mods = self.modifiers(*modifiers);
                if let Some(code) = named(*key) {
                    return vec![Action::Key(KeyEvent::new(code, mods))];
                }
                let Some(ch) = printable(*key, modifiers.shift) else {
                    return vec![];
                };
                // Without a chord the text event carries the character, in
                // the user's layout and case.
                let chord = [
                    KeyModifiers::CONTROL,
                    KeyModifiers::ALT,
                    KeyModifiers::META,
                    KeyModifiers::SUPER,
                ];
                if !chord.into_iter().any(|m| mods.contains(m)) {
                    return vec![];
                }
                self.suppress_text = modifiers.alt && !modifiers.ctrl && !modifiers.command;
                vec![Action::Key(KeyEvent::new(KeyCode::Char(ch), mods))]
            }
            Event::Text(text) if !suppress => vec![Action::Text(text.clone())],
            Event::Copy => vec![self.clipboard_chord('c', held)],
            Event::Cut => vec![self.clipboard_chord('x', held)],
            Event::Paste(_) if held.shift => vec![self.clipboard_chord('v', held)],
            Event::Paste(text) => vec![Action::Paste(text.clone())],
            Event::Ime(ImeEvent::Preedit { text, .. }) => vec![Action::Preedit(text.clone())],
            Event::Ime(ImeEvent::Commit(text)) => {
                vec![Action::Preedit(String::new()), Action::Text(text.clone())]
            }
            Event::WindowFocused(true) => vec![Action::Focused],
            _ => vec![],
        }
    }

    /// The chord a clipboard event replaced. Shifted, it is Leo's node
    /// command (Ctrl-Shift-c copies the node); plain Copy is Ctrl-c, leoapp's
    /// interrupt, whichever key made it.
    fn clipboard_chord(&self, ch: char, held: egui::Modifiers) -> Action {
        let mods = match held.shift {
            true => self.modifiers(held) | KeyModifiers::SHIFT,
            false if cfg!(target_os = "macos") && !self.cmd_is_ctrl => KeyModifiers::META,
            false => KeyModifiers::CONTROL,
        };
        Action::Key(KeyEvent::new(KeyCode::Char(ch), mods))
    }

    fn modifiers(&self, m: egui::Modifiers) -> KeyModifiers {
        let mut mods = KeyModifiers::NONE;
        if m.shift {
            mods |= KeyModifiers::SHIFT;
        }
        if m.ctrl {
            mods |= KeyModifiers::CONTROL;
        }
        if m.mac_cmd && self.cmd_is_ctrl {
            mods |= KeyModifiers::SUPER;
        }
        if m.mac_cmd && !self.cmd_is_ctrl {
            mods |= KeyModifiers::META;
        }
        if m.alt {
            mods |= KeyModifiers::ALT;
        }
        mods
    }
}

/// A key that types nothing.
fn named(key: Key) -> Option<KeyCode> {
    Some(match key {
        Key::ArrowDown => KeyCode::Down,
        Key::ArrowLeft => KeyCode::Left,
        Key::ArrowRight => KeyCode::Right,
        Key::ArrowUp => KeyCode::Up,
        Key::Escape => KeyCode::Esc,
        Key::Tab => KeyCode::Tab,
        Key::Backspace => KeyCode::Backspace,
        Key::Enter => KeyCode::Enter,
        Key::Insert => KeyCode::Insert,
        Key::Delete => KeyCode::Delete,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
        key => {
            let name = key.name();
            let n: u8 = name.strip_prefix('F')?.parse().ok()?;
            KeyCode::F(n)
        }
    })
}

/// The character a printable key names, for a chord. A letter is upper case
/// with Shift, as a terminal reports Alt-G.
fn printable(key: Key, shift: bool) -> Option<char> {
    let ch = match key {
        Key::Space => ' ',
        Key::Colon => ':',
        Key::Comma => ',',
        Key::Backslash => '\\',
        Key::Slash => '/',
        Key::Pipe => '|',
        Key::Questionmark => '?',
        Key::Exclamationmark => '!',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::OpenCurlyBracket => '{',
        Key::CloseCurlyBracket => '}',
        Key::Backtick => '`',
        Key::Minus => '-',
        Key::Period => '.',
        Key::Plus => '+',
        Key::Equals => '=',
        Key::Semicolon => ';',
        Key::Quote => '\'',
        key => {
            let mut chars = key.name().chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => c,
                _ => return None,
            }
        }
    };
    Some(match shift {
        true => ch.to_ascii_uppercase(),
        false => ch.to_ascii_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: egui::Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    fn mods(ctrl: bool, alt: bool, shift: bool, mac_cmd: bool) -> egui::Modifiers {
        egui::Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd,
            command: mac_cmd || (ctrl && !cfg!(target_os = "macos")),
        }
    }

    fn k(code: KeyCode, mods: KeyModifiers) -> Vec<Action> {
        vec![Action::Key(KeyEvent::new(code, mods))]
    }

    #[test]
    fn a_bare_letter_comes_from_its_text() {
        let mut t = Translator::new(true);
        assert_eq!(
            t.translate(&key(Key::G, egui::Modifiers::NONE), egui::Modifiers::NONE),
            vec![]
        );
        assert_eq!(
            t.translate(&Event::Text("g".into()), egui::Modifiers::NONE),
            vec![Action::Text("g".into())]
        );
    }

    #[test]
    fn a_chord_comes_from_its_key() {
        let mut t = Translator::new(true);
        let ctrl_r = key(Key::R, mods(true, false, false, false));
        assert_eq!(
            t.translate(&ctrl_r, egui::Modifiers::NONE),
            k(KeyCode::Char('r'), KeyModifiers::CONTROL)
        );
        let ctrl_shift_z = key(Key::Z, mods(true, false, true, false));
        assert_eq!(
            t.translate(&ctrl_shift_z, egui::Modifiers::NONE),
            k(
                KeyCode::Char('Z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )
        );
        let ctrl_bracket = key(Key::CloseBracket, mods(true, false, false, false));
        assert_eq!(
            t.translate(&ctrl_bracket, egui::Modifiers::NONE),
            k(KeyCode::Char(']'), KeyModifiers::CONTROL)
        );
    }

    #[test]
    fn the_text_after_an_alt_chord_is_dropped() {
        let mut t = Translator::new(true);
        let alt_g = key(Key::G, mods(false, true, false, false));
        assert_eq!(
            t.translate(&alt_g, egui::Modifiers::NONE),
            k(KeyCode::Char('g'), KeyModifiers::ALT)
        );
        assert_eq!(
            t.translate(&Event::Text("\u{a9}".into()), egui::Modifiers::NONE),
            vec![]
        );
        // Only the one event after it.
        assert_eq!(
            t.translate(&Event::Text("x".into()), egui::Modifiers::NONE),
            vec![Action::Text("x".into())]
        );
    }

    #[test]
    fn cmd_is_leos_ctrl_unless_the_setting_says_otherwise() {
        let cmd_s = key(Key::S, mods(false, false, false, true));
        assert_eq!(
            Translator::new(true).translate(&cmd_s, egui::Modifiers::NONE),
            k(KeyCode::Char('s'), KeyModifiers::SUPER)
        );
        assert_eq!(
            Translator::new(false).translate(&cmd_s, egui::Modifiers::NONE),
            k(KeyCode::Char('s'), KeyModifiers::META)
        );
        // Control is Ctrl either way, for the vim chords.
        let ctrl_d = key(Key::D, mods(true, false, false, false));
        assert_eq!(
            Translator::new(false).translate(&ctrl_d, egui::Modifiers::NONE),
            k(KeyCode::Char('d'), KeyModifiers::CONTROL)
        );
    }

    #[test]
    fn named_keys_keep_their_modifiers() {
        let mut t = Translator::new(true);
        assert_eq!(
            t.translate(
                &key(Key::Tab, mods(false, false, true, false)),
                egui::Modifiers::NONE
            ),
            k(KeyCode::Tab, KeyModifiers::SHIFT)
        );
        assert_eq!(
            t.translate(&key(Key::F1, egui::Modifiers::NONE), egui::Modifiers::NONE),
            k(KeyCode::F(1), KeyModifiers::NONE)
        );
        assert_eq!(
            t.translate(
                &key(Key::ArrowDown, mods(false, true, false, false)),
                egui::Modifiers::NONE
            ),
            k(KeyCode::Down, KeyModifiers::ALT)
        );
    }

    #[test]
    fn copy_is_the_interrupt_chord_and_paste_pastes() {
        let mut t = Translator::new(true);
        assert_eq!(
            t.translate(&Event::Copy, egui::Modifiers::NONE),
            k(KeyCode::Char('c'), KeyModifiers::CONTROL)
        );
        assert_eq!(
            t.translate(&Event::Paste("p".into()), egui::Modifiers::NONE),
            vec![Action::Paste("p".into())]
        );
    }

    #[test]
    fn a_shifted_clipboard_event_is_leos_node_command() {
        let mut t = Translator::new(true);
        let shift = egui::Modifiers {
            shift: true,
            ctrl: true,
            command: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        let node = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(
            t.translate(&Event::Copy, shift),
            k(KeyCode::Char('c'), node)
        );
        assert_eq!(t.translate(&Event::Cut, shift), k(KeyCode::Char('x'), node));
        assert_eq!(
            t.translate(&Event::Paste("text".into()), shift),
            k(KeyCode::Char('v'), node)
        );
        // On a Mac, from Cmd.
        let cmd = egui::Modifiers {
            shift: true,
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        assert_eq!(
            t.translate(&Event::Copy, cmd),
            k(
                KeyCode::Char('c'),
                KeyModifiers::SUPER | KeyModifiers::SHIFT
            )
        );
    }

    #[test]
    fn an_ime_commit_ends_the_preedit_and_types() {
        let mut t = Translator::new(true);
        let preedit = Event::Ime(ImeEvent::Preedit {
            text: "ni".into(),
            active_range_chars: None,
        });
        assert_eq!(
            t.translate(&preedit, egui::Modifiers::NONE),
            vec![Action::Preedit("ni".into())]
        );
        assert_eq!(
            t.translate(
                &Event::Ime(ImeEvent::Commit("\u{4f60}".into())),
                egui::Modifiers::NONE
            ),
            vec![
                Action::Preedit(String::new()),
                Action::Text("\u{4f60}".into())
            ]
        );
    }
}
