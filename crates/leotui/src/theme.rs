//! Themes, read from Helix's own theme files.
//!
//! Helix keys a theme by tree-sitter capture name -- `keyword`, `type.builtin`,
//! `variable.other.member` -- which is the vocabulary `treesit` already speaks,
//! so a Helix theme drops straight onto the classes `highlight` produces. That
//! is why this reads their format rather than defining one: a hundred themes
//! already exist and none of them had to be written here.
//!
//! Nothing is vendored. Themes are read from the user's own directories at
//! startup, so leotui carries no other project's files or licence.

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::PathBuf;

/// A colour, before the terminal's limits are applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Colour {
    /// One of the terminal's own, which a theme may name directly. Under 16
    /// this is a named ANSI colour, and the terminal's palette decides it.
    Ansi(u8),
    Rgb(u8, u8, u8),
}

/// How one scope is drawn.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Face {
    pub fg: Option<Colour>,
    pub bold: bool,
    pub italic: bool,
}

/// How many colours the terminal can show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    Ansi16,
    Indexed,
    True,
}

impl Depth {
    /// What the environment claims, which is all a terminal offers.
    ///
    /// There is no query for this: `COLORTERM` is the convention truecolor
    /// terminals set, and `TERM` carries the rest.
    pub fn detect() -> Depth {
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            return Depth::True;
        }
        let term = std::env::var("TERM").unwrap_or_default();
        match term.contains("direct") {
            true => Depth::True,
            false => match term.contains("256") {
                true => Depth::Indexed,
                false => Depth::Ansi16,
            },
        }
    }

    /// `name` as `:set colors` spells it.
    pub fn parse(name: &str) -> Option<Depth> {
        match name {
            "true" | "truecolor" | "24bit" => Some(Depth::True),
            "256" | "indexed" => Some(Depth::Indexed),
            "16" | "ansi" => Some(Depth::Ansi16),
            _ => None,
        }
    }
}

impl Colour {
    /// The nearest colour this depth can actually show.
    ///
    /// A named ANSI colour passes through: the terminal's own palette is
    /// already the best answer, whatever the depth.
    pub fn reduce(self, depth: Depth) -> Colour {
        let Colour::Rgb(r, g, b) = self else {
            return self;
        };
        match depth {
            Depth::True => self,
            Depth::Indexed => Colour::Ansi(nearest(r, g, b, &LAB_INDEXED, 16)),
            Depth::Ansi16 => Colour::Ansi(nearest(r, g, b, &LAB_16, 0)),
        }
    }
}

/// xterm's defaults for the sixteen, for measuring distance against.
const XTERM_16: &[(u8, u8, u8)] = &[
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// The 216-colour cube and the 24 greys, in their palette order.
///
/// The first sixteen are left out: they are whatever the terminal's palette
/// says, so their distance from a theme's colour is not knowable here.
fn indexed_palette() -> Vec<(u8, u8, u8)> {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let mut out = Vec::with_capacity(240);
    for r in LEVELS {
        for g in LEVELS {
            for b in LEVELS {
                out.push((r, g, b));
            }
        }
    }
    for i in 0..24u8 {
        let v = 8 + i * 10;
        out.push((v, v, v));
    }
    out
}

static LAB_16: Lazy<Vec<(f32, f32, f32)>> =
    Lazy::new(|| XTERM_16.iter().map(|c| lab(c.0, c.1, c.2)).collect());
static LAB_INDEXED: Lazy<Vec<(f32, f32, f32)>> = Lazy::new(|| {
    indexed_palette()
        .iter()
        .map(|c| lab(c.0, c.1, c.2))
        .collect()
});

/// sRGB to CIELAB, so that "nearest" means nearest to the eye.
///
/// Distance in RGB puts grey closest to anything unsaturated, because
/// (127,127,127) sits in the middle of the cube. Sonokai's pink keyword,
/// `#fc5d7c`, came out DarkGray at sixteen colours: 16,790 from grey against
/// 24,034 from red. In CIELAB it is red, which is what it looks like.
fn lab(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let linear = |v: u8| {
        let v = v as f32 / 255.0;
        match v <= 0.04045 {
            true => v / 12.92,
            false => ((v + 0.055) / 1.055).powf(2.4),
        }
    };
    let (r, g, b) = (linear(r), linear(g), linear(b));
    // D65, then the CIE's cube root with its linear tail near zero.
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| match t > 0.008856 {
        true => t.cbrt(),
        false => 7.787 * t + 16.0 / 116.0,
    };
    let (x, y, z) = (f(x), f(y), f(z));
    (116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z))
}

/// The palette index closest to `(r, g, b)`, by CIE76 distance.
fn nearest(r: u8, g: u8, b: u8, palette: &[(f32, f32, f32)], offset: u8) -> u8 {
    let (l0, a0, b0) = lab(r, g, b);
    let best = palette
        .iter()
        .enumerate()
        .min_by(|(_, x), (_, y)| {
            let d =
                |c: &(f32, f32, f32)| (c.0 - l0).powi(2) + (c.1 - a0).powi(2) + (c.2 - b0).powi(2);
            d(x).total_cmp(&d(y))
        })
        .map(|(i, _)| i)
        .unwrap_or(0);
    offset + best as u8
}

/// A theme: what to draw each scope as.
pub struct Theme {
    name: String,
    scopes: HashMap<String, Face>,
}

impl Theme {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The face for `scope`, or for its longest defined prefix.
    ///
    /// Helix resolves `type.builtin` to `type` when a theme names only the
    /// second, which is the rule tree-sitter uses for captures. A theme that
    /// names neither leaves the text plain.
    pub fn face(&self, scope: &str) -> Face {
        let mut at = scope;
        loop {
            if let Some(face) = self.scopes.get(at) {
                return *face;
            }
            match at.rfind('.') {
                Some(i) => at = &at[..i],
                None => return Face::default(),
            }
        }
    }

    /// The colours leotui draws when no theme has been loaded.
    ///
    /// The terminal's own sixteen, so this looks the same on any palette and
    /// needs nothing on disk.
    pub fn builtin() -> Theme {
        let fg = |n: u8| Face {
            fg: Some(Colour::Ansi(n)),
            ..Face::default()
        };
        let scopes = [
            ("keyword", fg(3)),
            ("string", fg(2)),
            ("comment", fg(8)),
            ("constant", fg(6)),
            ("type", fg(14)),
            ("type.builtin", fg(4)),
            ("function", fg(12)),
            ("variable.other.member", fg(7)),
            ("attribute", fg(13)),
            ("markup.link.text", fg(5)),
            (
                "keyword.directive",
                Face {
                    fg: Some(Colour::Ansi(5)),
                    bold: true,
                    italic: false,
                },
            ),
        ];
        Theme {
            name: "builtin".to_string(),
            scopes: scopes
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    /// Read `name` from the theme directories, following `inherits`.
    ///
    /// None when no directory holds it. A file that holds nothing this
    /// understands loads as an empty theme rather than failing: a theme is
    /// decoration, and refusing to start over one helps nobody.
    pub fn load(name: &str) -> Option<Theme> {
        let mut scopes = HashMap::new();
        let mut seen: Vec<String> = Vec::new();
        let mut next = Some(name.to_string());
        // Parents first, so a child's entries overlay them. Depth is capped
        // because `inherits` is a user's file and may point at itself.
        let mut chain: Vec<HashMap<String, Face>> = Vec::new();
        while let Some(current) = next.take() {
            if seen.contains(&current) || seen.len() > 8 {
                break;
            }
            let text = read_theme(&current)?;
            seen.push(current);
            let (entries, inherits) = parse(&text);
            chain.push(entries);
            next = inherits;
        }
        for entries in chain.into_iter().rev() {
            scopes.extend(entries);
        }
        Some(Theme {
            name: name.to_string(),
            scopes,
        })
    }
}

/// Every theme name the directories hold, sorted, without duplicates.
///
/// A name nearer the front of the search path wins, so a theme kept under
/// `leotui` shadows Helix's of the same name, as `load` resolves it.
pub fn names() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for dir in theme_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                out.push(stem.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Where a theme file may live, nearest first.
///
/// leotui's own directory, then Helix's, so a theme can be kept here without
/// Helix installed and Helix's hundred can be used when it is.
fn theme_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(config) = crate::config::config_home() {
        out.push(config.join("leotui/themes"));
        out.push(config.join("helix/themes"));
    }
    if let Some(runtime) = std::env::var_os("HELIX_RUNTIME") {
        out.push(PathBuf::from(runtime).join("themes"));
    }
    out
}

fn read_theme(name: &str) -> Option<String> {
    // A name is a file stem, never a path: a theme must not reach outside the
    // theme directories.
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return None;
    }
    theme_dirs()
        .into_iter()
        .find_map(|dir| std::fs::read_to_string(dir.join(format!("{name}.toml"))).ok())
}

/// The scopes a theme file defines, and the theme it inherits from.
///
/// A hand parser rather than a TOML crate: the format uses three shapes --
/// `key = "value"`, `key = { fg = "value", modifiers = [..] }` and a
/// `[palette]` of names to hex -- and anything it does not recognise is
/// skipped rather than refused.
fn parse(text: &str) -> (HashMap<String, Face>, Option<String>) {
    let mut palette: HashMap<String, Colour> = HashMap::new();
    let mut raw: Vec<(String, String)> = Vec::new();
    let mut inherits = None;
    let mut in_palette = false;

    for line in text.lines() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(section) = line.strip_prefix('[') {
            in_palette = section.trim_end_matches(']').trim() == "palette";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = unquote(key.trim());
        let value = value.trim();
        if key == "inherits" && !in_palette {
            inherits = Some(unquote(value).to_string());
            continue;
        }
        match in_palette {
            true => {
                if let Some(colour) = hex(unquote(value)) {
                    palette.insert(key.to_string(), colour);
                }
            }
            false => raw.push((key.to_string(), value.to_string())),
        }
    }

    let scopes = raw
        .into_iter()
        .filter_map(|(key, value)| face(&value, &palette).map(|f| (key, f)))
        .collect();
    (scopes, inherits)
}

/// Everything before an unquoted `#`.
pub(crate) fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = None::<u8>;
    for (i, &c) in bytes.iter().enumerate() {
        match quoted {
            Some(q) if c == q => quoted = None,
            Some(_) => {}
            // A `#` opens a colour as often as a comment, so only a bare one
            // outside quotes ends the line.
            None if c == b'#' => return &line[..i],
            None if c == b'"' || c == b'\'' => quoted = Some(c),
            None => {}
        }
    }
    line
}

pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// One scope's value: a bare colour, or an inline table naming one.
fn face(value: &str, palette: &HashMap<String, Colour>) -> Option<Face> {
    let value = value.trim();
    let Some(inner) = value
        .strip_prefix('{')
        .and_then(|r| r.trim_end().strip_suffix('}'))
    else {
        return Some(Face {
            fg: Some(colour(unquote(value), palette)?),
            ..Face::default()
        });
    };
    let mut out = Face::default();
    for field in split_fields(inner) {
        let Some((key, val)) = field.split_once('=') else {
            continue;
        };
        match unquote(key.trim()) {
            "fg" => out.fg = colour(unquote(val.trim()), palette),
            "modifiers" => {
                out.bold = val.contains("bold");
                out.italic = val.contains("italic");
            }
            // `bg` and `underline` are the pane's business, not a run of text's.
            _ => {}
        }
    }
    Some(out)
}

/// An inline table's fields, splitting only at the top level.
fn split_fields(inner: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0usize);
    for (i, c) in inner.char_indices() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&inner[start..]);
    out
}

/// A hex literal, a palette name, or one of the sixteen Helix names.
///
/// The palette is tried first: a theme that defines `red` means its own red,
/// not the terminal's.
fn colour(name: &str, palette: &HashMap<String, Colour>) -> Option<Colour> {
    if let Some(c) = hex(name) {
        return Some(c);
    }
    if let Some(c) = palette.get(name) {
        return Some(*c);
    }
    let ansi = |n: u8| Some(Colour::Ansi(n));
    match name {
        "black" => ansi(0),
        "red" => ansi(1),
        "green" => ansi(2),
        "yellow" => ansi(3),
        "blue" => ansi(4),
        "magenta" => ansi(5),
        "cyan" => ansi(6),
        "gray" | "grey" => ansi(7),
        "light-gray" | "light-grey" => ansi(8),
        "light-red" => ansi(9),
        "light-green" => ansi(10),
        "light-yellow" => ansi(11),
        "light-blue" => ansi(12),
        "light-magenta" => ansi(13),
        "light-cyan" => ansi(14),
        "white" => ansi(15),
        _ => None,
    }
}

fn hex(s: &str) -> Option<Colour> {
    let digits = s.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some(Colour::Rgb(byte(0)?, byte(2)?, byte(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
# A theme.
inherits = "parent"

"keyword" = "red"
"comment" = { fg = "grey0", modifiers = ["italic"] }
"keyword.directive" = { fg = "#ff8800", modifiers = ["bold", "italic"] }
"string" = "#00ff00"
"ui.background" = { bg = "black" }

[palette]
red = "#fc5d7c"
grey0 = "#7f8490"   # a trailing comment
"##;

    fn theme(text: &str) -> Theme {
        Theme {
            name: "test".to_string(),
            scopes: parse(text).0,
        }
    }

    #[test]
    fn a_scope_takes_a_bare_colour_a_palette_name_or_a_table() {
        let t = theme(SAMPLE);
        assert_eq!(t.face("string").fg, Some(Colour::Rgb(0, 255, 0)));
        // `red` is the theme's own, not the terminal's.
        assert_eq!(t.face("keyword").fg, Some(Colour::Rgb(0xfc, 0x5d, 0x7c)));
        assert_eq!(t.face("comment").fg, Some(Colour::Rgb(0x7f, 0x84, 0x90)));
    }

    #[test]
    fn modifiers_are_read_and_the_rest_of_a_table_ignored() {
        let t = theme(SAMPLE);
        assert!(t.face("comment").italic);
        assert!(!t.face("comment").bold);
        let directive = t.face("keyword.directive");
        assert!(directive.bold && directive.italic);
        assert_eq!(directive.fg, Some(Colour::Rgb(0xff, 0x88, 0x00)));
    }

    #[test]
    fn a_hash_opens_a_colour_as_often_as_a_comment() {
        // `#00ff00` must survive, and the comment after a palette entry must not.
        let t = theme(SAMPLE);
        assert_eq!(t.face("string").fg, Some(Colour::Rgb(0, 255, 0)));
        assert_eq!(t.face("comment").fg, Some(Colour::Rgb(0x7f, 0x84, 0x90)));
    }

    #[test]
    fn a_theme_names_the_one_it_inherits_from() {
        assert_eq!(parse(SAMPLE).1.as_deref(), Some("parent"));
        assert_eq!(parse("\"keyword\" = \"red\"").1, None);
    }

    #[test]
    fn a_scope_falls_back_to_its_longest_defined_prefix() {
        let t = theme(SAMPLE);
        // Defined outright.
        assert_eq!(
            t.face("keyword.directive").fg,
            Some(Colour::Rgb(255, 136, 0))
        );
        // `keyword.control.import` has only `keyword`.
        assert_eq!(t.face("keyword.control.import").fg, t.face("keyword").fg);
        assert_eq!(t.face("nothing.like.this"), Face::default());
    }

    #[test]
    fn a_line_that_makes_no_sense_is_skipped_rather_than_fatal() {
        let t = theme("garbage\n\"keyword\" = \"#010203\"\n= = =\n\"string\" = {\n");
        assert_eq!(t.face("keyword").fg, Some(Colour::Rgb(1, 2, 3)));
        assert_eq!(t.face("string"), Face::default());
    }

    #[test]
    fn reducing_a_colour_keeps_its_hue_rather_than_falling_to_grey() {
        // Sonokai's pink keyword. In plain RGB it is 16,790 from DarkGray and
        // 24,034 from red, so distance in CIELAB is what makes it red.
        let pink = Colour::Rgb(0xfc, 0x5d, 0x7c);
        assert_eq!(pink.reduce(Depth::Ansi16), Colour::Ansi(1));
        assert_eq!(
            Colour::Rgb(0x76, 0xcc, 0xe0).reduce(Depth::Ansi16),
            Colour::Ansi(6)
        );
        assert_eq!(
            Colour::Rgb(0xe7, 0xc6, 0x64).reduce(Depth::Ansi16),
            Colour::Ansi(3)
        );
        // A grey really is grey.
        assert_eq!(
            Colour::Rgb(0x7f, 0x84, 0x90).reduce(Depth::Ansi16),
            Colour::Ansi(8)
        );
    }

    #[test]
    fn reducing_to_the_cube_stays_off_the_terminals_own_sixteen() {
        let Colour::Ansi(n) = Colour::Rgb(0xfc, 0x5d, 0x7c).reduce(Depth::Indexed) else {
            panic!("not indexed");
        };
        assert!((16..=255).contains(&n), "{n} is one of the terminal's own");
    }

    #[test]
    fn truecolor_changes_nothing_and_a_named_colour_survives_every_depth() {
        let rgb = Colour::Rgb(1, 2, 3);
        assert_eq!(rgb.reduce(Depth::True), rgb);
        // The terminal's palette is already the best answer for a named one.
        for depth in [Depth::True, Depth::Indexed, Depth::Ansi16] {
            assert_eq!(Colour::Ansi(3).reduce(depth), Colour::Ansi(3));
        }
    }

    #[test]
    fn the_depth_names_set_accepts_are_the_ones_it_documents() {
        assert_eq!(Depth::parse("true"), Some(Depth::True));
        assert_eq!(Depth::parse("256"), Some(Depth::Indexed));
        assert_eq!(Depth::parse("16"), Some(Depth::Ansi16));
        assert_eq!(Depth::parse("lots"), None);
    }

    #[test]
    fn a_theme_name_cannot_reach_outside_the_theme_directories() {
        for name in ["../../etc/passwd", "/etc/passwd", "a/b", ""] {
            assert!(read_theme(name).is_none(), "{name} was read");
        }
    }

    #[test]
    fn the_builtin_theme_covers_every_scope_the_view_asks_for() {
        let t = Theme::builtin();
        for scope in [
            "keyword.directive",
            "markup.link.text",
            "comment",
            "string",
            "constant.numeric",
            "keyword",
            "type.builtin",
            "function",
            "type",
            "variable.other.member",
            "attribute",
        ] {
            assert!(t.face(scope).fg.is_some(), "{scope} has no colour");
        }
    }
}
