//! Key specifications, and the pending-key buffer that matches them.
//!
//! A binding is written the way `docs/dev/tui-design.md` writes it -- `"gg"`,
//! `"Shift-Down"`, `"Ctrl-r"`, `"Alt--"` -- and parsed here. That keeps the
//! table readable, which matters because the same table is the help screen.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One keypress, normalized so a table entry and a terminal event compare equal.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Key {
    pub fn new(code: KeyCode, mods: KeyModifiers) -> Self {
        let (code, mods) = normalize(code, mods);
        Self { code, mods }
    }

    pub fn from_event(event: KeyEvent) -> Self {
        Self::new(event.code, event.modifiers)
    }
}

/// One identity per physical chord, whatever the terminal reports.
///
/// Two disagreements have to be settled, and the second only shows up once
/// the keyboard enhancement protocol is on:
///
/// - An uppercase character already encodes shift, and terminals disagree
///   about whether to also set the bit. `J` is `J`, with no SHIFT.
/// - A control chord is named by its *lowercase* letter plus its modifiers:
///   Ctrl-Shift-Z arrives as `Char('z')+SHIFT|CONTROL` from some terminals and
///   `Char('Z')+SHIFT|CONTROL` from others. Both mean the same chord.
///
/// Shift-Tab gets the same treatment for the same reason: a terminal reports
/// it as `BackTab`+SHIFT, and the table spells it `Shift-Tab`, which parses to
/// `Tab`+SHIFT. Both become a bare `BackTab`.
fn normalize(code: KeyCode, mods: KeyModifiers) -> (KeyCode, KeyModifiers) {
    let mut mods = mods;
    let mut code = code;
    if code == KeyCode::Tab && mods.contains(KeyModifiers::SHIFT) {
        code = KeyCode::BackTab;
    }
    if code == KeyCode::BackTab {
        mods.remove(KeyModifiers::SHIFT);
    }
    if let KeyCode::Char(c) = code {
        if mods.contains(KeyModifiers::CONTROL) {
            code = KeyCode::Char(c.to_ascii_lowercase());
        } else if c.is_uppercase() || !c.is_alphabetic() {
            mods.remove(KeyModifiers::SHIFT);
        }
    }
    (code, mods)
}

/// Parse a binding string into the sequence of keys it names.
///
/// A token that names a key (`Enter`, `F1`, `Ctrl-r`) is one key; anything
/// else is one key per character, so `"gg"` and `"[m"` are two. A space
/// separates keys, so a sequence can start with a named one: `"Ctrl-w <"`.
pub fn parse(spec: &str) -> Vec<Key> {
    if spec.contains(' ') {
        return spec.split_whitespace().flat_map(parse).collect();
    }
    if let Some(key) = parse_one(spec) {
        return vec![key];
    }
    spec.chars()
        .map(|c| Key::new(KeyCode::Char(c), KeyModifiers::NONE))
        .collect()
}

/// One key, or None if `spec` is not a single key's name.
fn parse_one(spec: &str) -> Option<Key> {
    let mut mods = KeyModifiers::NONE;
    let mut rest = spec;
    loop {
        let lower = rest.to_ascii_lowercase();
        if let Some(tail) = lower.strip_prefix("ctrl-") {
            mods |= KeyModifiers::CONTROL;
            rest = &rest[rest.len() - tail.len()..];
        } else if let Some(tail) = lower.strip_prefix("alt-") {
            mods |= KeyModifiers::ALT;
            rest = &rest[rest.len() - tail.len()..];
        } else if let Some(tail) = lower.strip_prefix("shift-") {
            mods |= KeyModifiers::SHIFT;
            rest = &rest[rest.len() - tail.len()..];
        } else {
            break;
        }
    }
    let code = named(rest).or_else(|| {
        let mut chars = rest.chars();
        match (chars.next(), chars.next()) {
            // Control folds case -- a terminal reports Ctrl-H as Ctrl-h -- but
            // a plain letter does not: `G` and `g` are different bindings.
            (Some(c), None) if mods.contains(KeyModifiers::CONTROL) => {
                Some(KeyCode::Char(c.to_ascii_lowercase()))
            }
            (Some(c), None) => Some(KeyCode::Char(c)),
            _ => None,
        }
    })?;
    Some(Key::new(code, mods))
}

fn named(name: &str) -> Option<KeyCode> {
    let code = match name.to_ascii_lowercase().as_str() {
        "space" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "prior" => KeyCode::PageUp,
        "pagedown" | "next" => KeyCode::PageDown,
        "up" | "uparrow" => KeyCode::Up,
        "down" | "dnarrow" | "downarrow" => KeyCode::Down,
        "left" | "ltarrow" | "leftarrow" => KeyCode::Left,
        "right" | "rtarrow" | "rightarrow" => KeyCode::Right,
        other => {
            let n: u8 = other.strip_prefix('f')?.parse().ok()?;
            if (1..=12).contains(&n) {
                return Some(KeyCode::F(n));
            }
            return None;
        }
    };
    Some(code)
}

/// The keys typed so far towards a binding, and any count before them.
#[derive(Default)]
pub struct Pending {
    pub keys: Vec<Key>,
    pub count: Option<usize>,
}

impl Pending {
    pub fn clear(&mut self) {
        self.keys.clear();
        self.count = None;
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.count.is_none()
    }

    /// The count to apply, defaulting to 1.
    pub fn count(&self) -> usize {
        self.count.unwrap_or(1).max(1)
    }

    /// Add a digit to the count. `0` only continues a count already started.
    pub fn push_digit(&mut self, d: usize) -> bool {
        if d == 0 && self.count.is_none() {
            return false;
        }
        self.count = Some(self.count.unwrap_or(0) * 10 + d);
        true
    }

    /// How the pending keys relate to a binding's keys.
    pub fn describe(&self) -> String {
        let count = self.count.map(|n| n.to_string()).unwrap_or_default();
        let keys: String = self.keys.iter().map(display).collect();
        format!("{count}{keys}")
    }
}

/// A key as the help screen and the status line spell it.
pub fn display(key: &Key) -> String {
    let mut out = String::new();
    if key.mods.contains(KeyModifiers::CONTROL) {
        out.push_str("Ctrl-");
    }
    if key.mods.contains(KeyModifiers::ALT) {
        out.push_str("Alt-");
    }
    if key.mods.contains(KeyModifiers::SHIFT) {
        out.push_str("Shift-");
    }
    let name = match key.code {
        // The table spells it the way a keyboard does.
        KeyCode::BackTab => return format!("{out}Shift-Tab"),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        other => format!("{other:?}"),
    };
    out.push_str(&name);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(spec: &str) -> Vec<(KeyCode, KeyModifiers)> {
        parse(spec).into_iter().map(|k| (k.code, k.mods)).collect()
    }

    #[test]
    fn a_bare_word_is_one_key_per_character() {
        assert_eq!(
            keys("gg"),
            vec![
                (KeyCode::Char('g'), KeyModifiers::NONE),
                (KeyCode::Char('g'), KeyModifiers::NONE)
            ]
        );
        assert_eq!(keys("[m").len(), 2);
        assert_eq!(keys("<<").len(), 2);
        assert_eq!(keys("z1").len(), 2);
    }

    #[test]
    fn a_named_key_is_one_key() {
        assert_eq!(
            keys("Space"),
            vec![(KeyCode::Char(' '), KeyModifiers::NONE)]
        );
        assert_eq!(keys("Enter"), vec![(KeyCode::Enter, KeyModifiers::NONE)]);
        assert_eq!(keys("F1"), vec![(KeyCode::F(1), KeyModifiers::NONE)]);
    }

    #[test]
    fn modifiers_bind_to_the_rest_of_the_token() {
        assert_eq!(
            keys("Ctrl-r"),
            vec![(KeyCode::Char('r'), KeyModifiers::CONTROL)]
        );
        assert_eq!(
            keys("Shift-Down"),
            vec![(KeyCode::Down, KeyModifiers::SHIFT)]
        );
        assert_eq!(keys("Alt-Home"), vec![(KeyCode::Home, KeyModifiers::ALT)]);
    }

    #[test]
    fn a_modifier_can_be_followed_by_a_hyphen() {
        // Leo binds contract-all to Alt-minus.
        assert_eq!(keys("Alt--"), vec![(KeyCode::Char('-'), KeyModifiers::ALT)]);
    }

    #[test]
    fn an_uppercase_character_does_not_need_the_shift_bit() {
        // Terminals disagree about whether to set it; the table must not care.
        let from_table = parse("J")[0];
        let from_terminal = Key::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(from_table, from_terminal);
    }

    #[test]
    fn a_count_needs_a_nonzero_digit_first() {
        let mut p = Pending::default();
        assert!(!p.push_digit(0));
        assert!(p.push_digit(3));
        assert!(p.push_digit(0));
        assert_eq!(p.count(), 30);
    }

    #[test]
    fn a_control_chord_has_one_identity_whatever_the_terminal_sends() {
        // Under the keyboard enhancement protocol some terminals report the
        // shifted codepoint and some the unshifted one.
        let lower = Key::new(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let upper = Key::new(
            KeyCode::Char('Z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(lower, upper);
        assert_eq!(parse("Ctrl-Shift-Z")[0], lower);
        assert_eq!(parse("Ctrl-Shift-z")[0], lower);
    }

    #[test]
    fn shift_tab_is_one_key_however_it_is_spelled() {
        // A terminal sends BackTab+SHIFT; the table says Shift-Tab.
        let from_terminal = Key::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(parse("Shift-Tab")[0], from_terminal);
        assert_eq!(parse("BackTab")[0], from_terminal);
        assert_ne!(parse("Tab")[0], from_terminal);
        assert_eq!(display(&from_terminal), "Shift-Tab");
    }

    #[test]
    fn control_folds_case_but_a_plain_letter_does_not() {
        assert_eq!(parse("Ctrl-H")[0], parse("Ctrl-h")[0]);
        assert_ne!(parse("G")[0], parse("g")[0]);
    }

    #[test]
    fn display_round_trips_a_spec() {
        for spec in ["Ctrl-r", "Shift-Down", "Alt-Home", "F1", "Space"] {
            assert_eq!(display(&parse(spec)[0]), spec);
        }
    }
}
