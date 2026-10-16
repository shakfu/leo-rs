//! The colours the front ends draw their panes with, read from the theme.
//!
//! Each part reads the Helix scope Helix draws the same part with: the mode
//! badge `ui.statusline.normal`, a tab `ui.bufferline`, the cursor
//! `ui.cursor.primary.insert`, and so on. `Theme::face` falls back through a
//! scope's prefixes, so a theme that defines only `ui.cursor` still colours
//! every cursor. What a theme leaves out is derived from its background and
//! text.
//!
//! leogui and leotui both build this, then convert each colour to their
//! own type with `Palette::map`, so one theme looks the same in both.

use crate::app::Mode;
use crate::theme::{Colour, Depth, Face, Theme, UnderlineStyle};
use crate::view::Severity;

/// How the body cursor is drawn: its fill, and the text over it when the
/// theme says (`fg`, or a `reversed` cursor's background).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor<C> {
    pub fill: C,
    pub text: Option<C>,
    /// The theme names no fill: the accent, drawn at 75% so the text shows.
    pub soft: bool,
}

/// A diagnostic's underline and its gutter mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark<C> {
    pub underline: C,
    pub style: UnderlineStyle,
    pub gutter: C,
}

/// The colours one frame draws with, read from the theme once.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette<C = Colour> {
    pub dark: bool,
    pub bg: C,
    /// Side panels and bars, a step off the editor's background.
    pub panel: C,
    pub statusbar: C,
    pub statusbar_text: C,
    pub popup: C,
    pub help: C,
    pub menu: C,
    pub menu_selected: C,
    /// The text on `menu_selected`: the theme's, or black or white.
    pub menu_selected_text: C,
    pub fg: C,
    pub dim: C,
    pub accent: C,
    pub selection: C,
    pub line: C,
    pub border: C,
    /// The outline's and the body's indent guides.
    pub guide: C,
    /// Behind a bracket and its match: `ui.cursor.match`.
    pub bracket: C,
    /// The `@pagewidth` ruler: `ui.virtual.ruler`.
    pub ruler: C,
    /// `:set list`'s marks for spaces and tabs: `ui.virtual.whitespace`.
    pub whitespace: C,
    /// An `@<file>` node's icon: a neutral tone a step brighter than a
    /// plain node's, its shape saying it is a file. A theme's accent or
    /// directory colour is a strong red or green in some themes.
    pub file: C,
    pub warning: C,
    pub gutter: C,
    pub linenr: C,
    pub linenr_current: C,
    /// Tabs: the strip, a tab, the open tab, as (background, text).
    pub tab_strip: C,
    pub tab: (C, C),
    pub tab_active: (C, C),
    /// Badge (background, text) per mode, in `badge_index` order.
    badges: [(C, C); 7],
    cursors: [Cursor<C>; 3],
    marks: [Mark<C>; 4],
}

/// A theme colour as red, green and blue. A terminal colour is `ansi`'s.
pub fn rgb(c: Colour) -> (u8, u8, u8) {
    match c {
        Colour::Rgb(r, g, b) => (r, g, b),
        Colour::Ansi(n) => ansi(n),
    }
}

/// `a` moved `t` of the way toward `b`.
pub fn mix(a: Colour, b: Colour, t: f32) -> Colour {
    let (a, b) = (rgb(a), rgb(b));
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Colour::Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

pub fn luminance(c: Colour) -> f32 {
    let (r, g, b) = rgb(c);
    0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
}

/// Black or white, whichever reads on `bg`.
pub fn text_on(bg: Colour) -> Colour {
    match luminance(bg) > 140.0 {
        true => Colour::Rgb(0, 0, 0),
        false => Colour::Rgb(255, 255, 255),
    }
}

fn grey(v: u8) -> Colour {
    Colour::Rgb(v, v, v)
}

/// A terminal palette entry: the sixteen named colours, then xterm's 6x6x6
/// cube and grey ramp. The sixteen are One Dark's: a terminal's own, not
/// xterm's saturated defaults, whose pure green and blue glare in a window.
pub fn ansi(n: u8) -> (u8, u8, u8) {
    const SIXTEEN: [(u8, u8, u8); 16] = [
        (40, 44, 52),
        (224, 108, 117),
        (152, 195, 121),
        (229, 192, 123),
        (97, 175, 239),
        (198, 120, 221),
        (86, 182, 194),
        (171, 178, 191),
        (92, 99, 112),
        (240, 130, 138),
        (170, 210, 140),
        (240, 205, 140),
        (120, 190, 245),
        (210, 145, 230),
        (110, 195, 205),
        (220, 223, 228),
    ];
    match n {
        0..=15 => SIXTEEN[n as usize],
        16..=231 => {
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            let i = n - 16;
            (level(i / 36), level(i / 6 % 6), level(i % 6))
        }
        _ => {
            let v = 8 + 10 * (n - 232);
            (v, v, v)
        }
    }
}

/// The three modes Helix colours by name, as indexes.
fn mode_index(mode: Mode) -> Option<usize> {
    match mode {
        Mode::Normal => Some(0),
        Mode::Insert => Some(1),
        Mode::Visual => Some(2),
        _ => None,
    }
}

const MODE_NAMES: [&str; 3] = ["normal", "insert", "select"];

/// Every mode's badge slot, the three Helix names first.
fn badge_index(mode: Mode) -> usize {
    mode_index(mode).unwrap_or(match mode {
        Mode::Headline => 3,
        Mode::Help => 4,
        Mode::Confirm => 5,
        _ => 6,
    })
}

fn severity_index(severity: Severity) -> usize {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Information => 2,
        Severity::Hint => 3,
    }
}

impl Palette {
    /// `theme`'s colours at `depth`. `light` picks the defaults for what
    /// the theme leaves out, as the built-in theme leaves out the background.
    pub fn of(theme: &Theme, depth: Depth, light: bool) -> Palette {
        let face = |scope: &str| theme.face(scope);
        let colour = |c: Option<Colour>| match depth {
            Depth::None => None,
            _ => c.map(|c| c.reduce(depth)),
        };
        let fg_of = |f: Face| colour(f.fg);
        let bg_of = |f: Face| colour(f.bg);
        let blank = if light { grey(250) } else { grey(30) };
        let bg = bg_of(face("ui.background")).unwrap_or(blank);
        let dark = luminance(bg) < 128.0;
        let fg = fg_of(face("ui.text")).unwrap_or(match dark {
            true => grey(212),
            false => grey(30),
        });
        let toward = if dark { grey(255) } else { grey(0) };
        let accent = fg_of(face("ui.text.focus"))
            .or_else(|| fg_of(face("function")))
            .unwrap_or(Colour::Rgb(86, 156, 214));
        let panel = mix(bg, toward, 0.07);
        let popup = bg_of(face("ui.popup")).unwrap_or(mix(bg, toward, 0.10));
        let selection = bg_of(face("ui.selection.primary")).unwrap_or(mix(bg, accent, 0.30));
        let border = fg_of(face("ui.background.separator"))
            .or_else(|| fg_of(face("ui.window")).map(|c| mix(c, bg, 0.5)))
            .unwrap_or(mix(bg, toward, 0.14));
        let statusline = face("ui.statusline");
        let statusbar = bg_of(statusline).unwrap_or(mix(bg, toward, 0.12));

        // Only a mode the theme names: the prefix, `ui.statusline`, is the
        // bar's own colour, and a badge in it would vanish into the bar.
        // The other modes take a fixed terminal colour.
        let badges = [
            Mode::Normal,
            Mode::Insert,
            Mode::Visual,
            Mode::Headline,
            Mode::Help,
            Mode::Confirm,
            Mode::Command,
        ]
        .map(|mode| {
            let named = mode_index(mode).and_then(|i| {
                let scope = format!("ui.statusline.{}", MODE_NAMES[i]);
                let f = face(&scope);
                let b = bg_of(f).filter(|_| theme.defines(&scope))?;
                Some((b, fg_of(f).unwrap_or_else(|| text_on(b))))
            });
            named.unwrap_or_else(|| {
                let b = Colour::Ansi(match mode {
                    Mode::Normal => 4,
                    Mode::Insert => 2,
                    Mode::Visual | Mode::Headline => 5,
                    Mode::Help => 6,
                    Mode::Confirm => 1,
                    Mode::Command | Mode::Search => 3,
                });
                (b, text_on(b))
            })
        });
        // The same for a scope whose prefix means something else.
        let own = |scope: &str| theme.defines(scope).then(|| face(scope));
        let cursors = MODE_NAMES.map(|m| {
            let f = face(&format!("ui.cursor.primary.{m}"));
            match (f.reversed, bg_of(f)) {
                // A reversed cursor is the text's colour, over the text.
                (true, _) | (false, None) => Cursor {
                    fill: if f.reversed { fg } else { accent },
                    text: f.reversed.then_some(bg),
                    soft: !f.reversed,
                },
                (false, Some(b)) => Cursor {
                    fill: b,
                    text: fg_of(f),
                    soft: false,
                },
            }
        });
        let fallback = [
            Colour::Rgb(240, 80, 80),
            Colour::Rgb(230, 180, 60),
            Colour::Rgb(90, 160, 240),
            grey(140),
        ];
        let marks = std::array::from_fn(|i| {
            let s = ["error", "warning", "info", "hint"][i];
            let gutter = fg_of(face(s)).unwrap_or(fallback[i]);
            let d = face(&format!("diagnostic.{s}"));
            Mark {
                underline: colour(d.underline_colour)
                    .or_else(|| fg_of(d))
                    .unwrap_or(gutter),
                // A theme that only underlines gets the wavy line editors
                // draw, unless it asks for another.
                style: match d.underlined && d.underline_colour.is_some() {
                    true => d.underline_style,
                    false => UnderlineStyle::Curl,
                },
                gutter,
            }
        });
        let bufferline = face("ui.bufferline");
        let active = face("ui.bufferline.active");
        let menu_selected = own("ui.menu.selected").and_then(bg_of).unwrap_or(selection);
        Palette {
            dark,
            bg,
            panel,
            statusbar,
            statusbar_text: fg_of(statusline).unwrap_or(fg),
            popup,
            help: bg_of(face("ui.help")).unwrap_or(popup),
            menu: bg_of(face("ui.menu")).unwrap_or(popup),
            menu_selected,
            menu_selected_text: own("ui.menu.selected")
                .and_then(fg_of)
                .unwrap_or_else(|| text_on(menu_selected)),
            fg,
            dim: own("ui.text.inactive")
                .and_then(fg_of)
                .unwrap_or(mix(fg, bg, 0.45)),
            accent,
            selection,
            line: bg_of(face("ui.cursorline.primary")).unwrap_or(mix(bg, toward, 0.05)),
            border,
            guide: fg_of(face("ui.virtual.indent-guide")).unwrap_or(border),
            bracket: bg_of(face("ui.cursor.match")).unwrap_or(mix(bg, toward, 0.25)),
            ruler: bg_of(face("ui.virtual.ruler")).unwrap_or(mix(bg, toward, 0.08)),
            whitespace: fg_of(face("ui.virtual.whitespace")).unwrap_or(mix(fg, bg, 0.7)),
            file: mix(fg, bg, 0.3),
            warning: fg_of(face("warning")).unwrap_or(Colour::Ansi(11)),
            gutter: bg_of(face("ui.gutter")).unwrap_or(bg),
            linenr: fg_of(face("ui.linenr")).unwrap_or(mix(fg, bg, 0.6)),
            linenr_current: fg_of(face("ui.linenr.selected")).unwrap_or(fg),
            tab_strip: bg_of(face("ui.bufferline.background")).unwrap_or(panel),
            tab: (
                bg_of(bufferline).unwrap_or(panel),
                fg_of(bufferline).unwrap_or(mix(fg, bg, 0.45)),
            ),
            tab_active: (bg_of(active).unwrap_or(bg), fg_of(active).unwrap_or(fg)),
            badges,
            cursors,
            marks,
        }
    }
}

impl<C: Copy> Palette<C> {
    /// The same palette with each colour converted by `f`.
    pub fn map<D: Copy>(&self, f: impl Fn(C) -> D) -> Palette<D> {
        let pair = |(a, b): (C, C)| (f(a), f(b));
        Palette {
            dark: self.dark,
            bg: f(self.bg),
            panel: f(self.panel),
            statusbar: f(self.statusbar),
            statusbar_text: f(self.statusbar_text),
            popup: f(self.popup),
            help: f(self.help),
            menu: f(self.menu),
            menu_selected: f(self.menu_selected),
            menu_selected_text: f(self.menu_selected_text),
            fg: f(self.fg),
            dim: f(self.dim),
            accent: f(self.accent),
            selection: f(self.selection),
            line: f(self.line),
            border: f(self.border),
            guide: f(self.guide),
            bracket: f(self.bracket),
            ruler: f(self.ruler),
            whitespace: f(self.whitespace),
            file: f(self.file),
            warning: f(self.warning),
            gutter: f(self.gutter),
            linenr: f(self.linenr),
            linenr_current: f(self.linenr_current),
            tab_strip: f(self.tab_strip),
            tab: pair(self.tab),
            tab_active: pair(self.tab_active),
            badges: self.badges.map(pair),
            cursors: self.cursors.map(|c| Cursor {
                fill: f(c.fill),
                text: c.text.map(&f),
                soft: c.soft,
            }),
            marks: self.marks.map(|m| Mark {
                underline: f(m.underline),
                style: m.style,
                gutter: f(m.gutter),
            }),
        }
    }

    /// The mode badge's (background, text): the theme's for NORMAL, INSERT
    /// and VISUAL, a fixed colour for the modes Helix has no name for.
    pub fn mode(&self, mode: Mode) -> (C, C) {
        self.badges[badge_index(mode)]
    }

    /// The body cursor in `mode`.
    pub fn cursor(&self, mode: Mode) -> Cursor<C> {
        self.cursors[mode_index(mode).unwrap_or(0)]
    }

    /// How a diagnostic of `severity` is marked.
    pub fn mark(&self, severity: Severity) -> Mark<C> {
        self.marks[severity_index(severity)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_is_one_darks_sixteen_then_xterms() {
        assert_eq!(ansi(3), (229, 192, 123));
        assert_eq!(ansi(16), (0, 0, 0));
        assert_eq!(ansi(21), (0, 0, 255));
        assert_eq!(ansi(196), (255, 0, 0));
        assert_eq!(ansi(232), (8, 8, 8));
        assert_eq!(ansi(255), (238, 238, 238));
    }

    #[test]
    fn a_colour_mixes_toward_another() {
        assert_eq!(mix(grey(0), grey(255), 0.5), grey(128));
        // A terminal colour mixes as its palette entry.
        assert_eq!(mix(Colour::Ansi(3), Colour::Ansi(3), 0.5), {
            let (r, g, b) = ansi(3);
            Colour::Rgb(r, g, b)
        });
    }

    #[test]
    fn the_builtin_theme_still_gives_every_part_a_colour() {
        for light in [false, true] {
            let p = Palette::of(&Theme::builtin(), Depth::True, light);
            assert_eq!(p.dark, !light);
            // The modes Helix names fall back to the badge colours.
            assert_ne!(p.mode(Mode::Normal).0, p.mode(Mode::Insert).0);
            assert_eq!(p.mode(Mode::Command), p.mode(Mode::Search));
            assert_ne!(
                p.mark(Severity::Error).gutter,
                p.mark(Severity::Hint).gutter
            );
        }
    }

    #[test]
    fn without_colour_every_part_is_derived() {
        let p = Palette::of(&Theme::builtin(), Depth::None, false);
        assert_eq!(p.bg, grey(30));
        assert_eq!(p.selection, mix(grey(30), Colour::Rgb(86, 156, 214), 0.3));
    }

    #[test]
    fn a_mapped_palette_keeps_its_parts() {
        let p = Palette::of(&Theme::builtin(), Depth::True, false);
        let m = p.map(rgb);
        assert_eq!(m.bg, rgb(p.bg));
        assert_eq!(m.mode(Mode::Insert).0, rgb(p.mode(Mode::Insert).0));
        assert_eq!(
            m.mark(Severity::Warning).style,
            p.mark(Severity::Warning).style
        );
    }
}
