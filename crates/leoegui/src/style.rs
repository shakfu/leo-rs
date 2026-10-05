//! Colours: the theme's faces as egui colours, and egui's own widgets
//! dressed in the theme, so menus and panels match the body.
//!
//! Each part of the window reads the Helix scope Helix draws the same part
//! with: the mode badge `ui.statusline.normal`, a tab `ui.bufferline`, the
//! cursor `ui.cursor.primary.insert`, and so on. `Theme::face` falls back
//! through a scope's prefixes, so a theme that defines only `ui.cursor` still
//! colours every cursor. What a theme leaves out is derived from its
//! background and text.

use eframe::egui::{self, Color32, Stroke};
use leoapp::app::{App, Mode};
use leoapp::theme::{Colour, Depth, Face, UnderlineStyle};
use leoapp::view::Severity;

/// How the body cursor is drawn: its fill, and the text over it when the
/// theme says (`fg`, or a `reversed` cursor's background).
#[derive(Clone, Copy)]
pub struct Cursor {
    pub fill: Color32,
    pub text: Option<Color32>,
}

/// A diagnostic's underline and its gutter mark.
#[derive(Clone, Copy)]
pub struct Mark {
    pub underline: Color32,
    pub style: UnderlineStyle,
    pub gutter: Color32,
}

/// The colours one frame draws with, read from the theme once.
#[derive(Clone)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    /// Side panels and bars, a step off the editor's background.
    pub panel: Color32,
    pub statusbar: Color32,
    pub statusbar_text: Color32,
    pub popup: Color32,
    pub help: Color32,
    pub menu: Color32,
    pub menu_selected: Color32,
    pub fg: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub line: Color32,
    pub border: Color32,
    /// The outline's indent guides.
    pub guide: Color32,
    /// An `@<file>` node's icon: a neutral tone a step brighter than a
    /// plain node's, its shape saying it is a file. A theme's accent or
    /// directory colour is a strong red or green in some themes.
    pub file: Color32,
    pub warning: Color32,
    pub gutter: Color32,
    pub linenr: Color32,
    pub linenr_current: Color32,
    /// Tabs: the strip, a tab, the open tab, as (background, text).
    pub tab_strip: Color32,
    pub tab: (Color32, Color32),
    pub tab_active: (Color32, Color32),
    /// Badge (background, text) for NORMAL, INSERT and VISUAL.
    modes: [Option<(Color32, Color32)>; 3],
    cursors: [Cursor; 3],
    marks: [Mark; 4],
}

/// A theme colour as egui's. A terminal colour is `ansi`'s.
pub fn colour(app: &App, c: Option<Colour>) -> Option<Color32> {
    if app.depth == Depth::None {
        return None;
    }
    c.map(|c| match c.reduce(app.depth) {
        Colour::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        Colour::Ansi(n) => ansi(n),
    })
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn luminance(c: Color32) -> f32 {
    0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32
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

fn severity_index(severity: Severity) -> usize {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Information => 2,
        Severity::Hint => 3,
    }
}

impl Palette {
    /// The theme's colours. `light` picks the defaults for what the theme
    /// leaves out, as the built-in theme leaves out the background.
    pub fn of(app: &App, light: bool) -> Palette {
        let face = |scope: &str| app.theme.face(scope);
        let fg_of = |f: Face| colour(app, f.fg);
        let bg_of = |f: Face| colour(app, f.bg);
        let blank = if light {
            Color32::from_gray(250)
        } else {
            Color32::from_gray(30)
        };
        let bg = bg_of(face("ui.background")).unwrap_or(blank);
        let dark = luminance(bg) < 128.0;
        let fg = fg_of(face("ui.text")).unwrap_or(match dark {
            true => Color32::from_gray(212),
            false => Color32::from_gray(30),
        });
        let toward = if dark { Color32::WHITE } else { Color32::BLACK };
        let accent = fg_of(face("ui.text.focus"))
            .or_else(|| fg_of(face("function")))
            .unwrap_or(Color32::from_rgb(86, 156, 214));
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
        let modes = MODE_NAMES.map(|m| {
            let scope = format!("ui.statusline.{m}");
            let f = face(&scope);
            let b = bg_of(f).filter(|_| app.theme.defines(&scope))?;
            Some((b, fg_of(f).unwrap_or_else(|| text_on(b))))
        });
        // The same for a scope whose prefix means something else.
        let own = |scope: &str| app.theme.defines(scope).then(|| face(scope));
        let cursors = MODE_NAMES.map(|m| {
            let f = face(&format!("ui.cursor.primary.{m}"));
            match (f.reversed, bg_of(f)) {
                // A reversed cursor is the text's colour, over the text.
                (true, _) | (false, None) => Cursor {
                    fill: if f.reversed {
                        fg
                    } else {
                        accent.gamma_multiply(0.75)
                    },
                    text: f.reversed.then_some(bg),
                },
                (false, Some(b)) => Cursor {
                    fill: b,
                    text: fg_of(f),
                },
            }
        });
        let fallback = [
            Color32::from_rgb(240, 80, 80),
            Color32::from_rgb(230, 180, 60),
            Color32::from_rgb(90, 160, 240),
            Color32::from_gray(140),
        ];
        let marks = std::array::from_fn(|i| {
            let s = ["error", "warning", "info", "hint"][i];
            let gutter = fg_of(face(s)).unwrap_or(fallback[i]);
            let d = face(&format!("diagnostic.{s}"));
            Mark {
                underline: colour(app, d.underline_colour)
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
        Palette {
            dark,
            bg,
            panel,
            statusbar,
            statusbar_text: fg_of(statusline).unwrap_or(fg),
            popup,
            help: bg_of(face("ui.help")).unwrap_or(popup),
            menu: bg_of(face("ui.menu")).unwrap_or(popup),
            menu_selected: own("ui.menu.selected").and_then(bg_of).unwrap_or(selection),
            fg,
            dim: own("ui.text.inactive")
                .and_then(fg_of)
                .unwrap_or(mix(fg, bg, 0.45)),
            accent,
            selection,
            line: bg_of(face("ui.cursorline.primary")).unwrap_or(mix(bg, toward, 0.05)),
            border,
            guide: fg_of(face("ui.virtual.indent-guide")).unwrap_or(border),
            file: mix(fg, bg, 0.3),
            warning: fg_of(face("warning")).unwrap_or(ansi(11)),
            gutter: bg_of(face("ui.gutter")).unwrap_or(bg),
            linenr: fg_of(face("ui.linenr")).unwrap_or(mix(fg, bg, 0.6)),
            linenr_current: fg_of(face("ui.linenr.selected")).unwrap_or(fg),
            tab_strip: bg_of(face("ui.bufferline.background")).unwrap_or(panel),
            tab: (
                bg_of(bufferline).unwrap_or(panel),
                fg_of(bufferline).unwrap_or(mix(fg, bg, 0.45)),
            ),
            tab_active: (bg_of(active).unwrap_or(bg), fg_of(active).unwrap_or(fg)),
            modes,
            cursors,
            marks,
        }
    }

    /// The mode badge's (background, text): the theme's for NORMAL, INSERT
    /// and VISUAL, a fixed colour for the modes Helix has no name for.
    pub fn mode(&self, mode: Mode) -> (Color32, Color32) {
        if let Some(c) = mode_index(mode).and_then(|i| self.modes[i]) {
            return c;
        }
        let bg = ansi(match mode {
            Mode::Normal => 4,
            Mode::Insert => 2,
            Mode::Visual | Mode::Headline => 5,
            Mode::Help => 6,
            Mode::Confirm => 1,
            Mode::Command | Mode::Search => 3,
        });
        (bg, text_on(bg))
    }

    /// The body cursor in `mode`.
    pub fn cursor(&self, mode: Mode) -> Cursor {
        self.cursors[mode_index(mode).unwrap_or(0)]
    }

    /// How a diagnostic of `severity` is marked.
    pub fn mark(&self, severity: Severity) -> Mark {
        self.marks[severity_index(severity)]
    }

    /// egui's widgets in the theme's colours.
    pub fn visuals(&self) -> egui::Visuals {
        let mut v = match self.dark {
            true => egui::Visuals::dark(),
            false => egui::Visuals::light(),
        };
        v.panel_fill = self.panel;
        v.window_fill = self.menu;
        v.extreme_bg_color = self.bg;
        v.faint_bg_color = self.line;
        v.override_text_color = Some(self.fg);
        v.selection.bg_fill = self.menu_selected;
        v.selection.stroke = Stroke::new(1.0, self.fg);
        v.window_stroke = Stroke::new(1.0, self.border);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.noninteractive.bg_fill = self.panel;
        // A control's box -- a checkbox, a radio button, a slider's rail, a
        // text field's frame -- must stand off the dialog it sits on, which
        // is the menu colour. So each state is the menu colour lifted toward
        // the text, with an edge, rather than the theme's own fills, which
        // can match the dialog and vanish.
        let lift = |t: f32| mix(self.menu, self.fg, t);
        let edge = Stroke::new(1.0, lift(0.35));
        v.widgets.inactive.bg_fill = lift(0.14);
        v.widgets.inactive.weak_bg_fill = lift(0.08);
        v.widgets.inactive.bg_stroke = edge;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.fg);
        v.widgets.hovered.bg_fill = lift(0.24);
        v.widgets.hovered.weak_bg_fill = lift(0.16);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, lift(0.55));
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, self.fg);
        v.widgets.active.bg_fill = lift(0.32);
        v.widgets.active.weak_bg_fill = self.menu_selected;
        v.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        v.widgets.active.fg_stroke = Stroke::new(1.5, self.fg);
        // A window's title bar and an open combo box take this: a quiet
        // step, not the menu's selection colour.
        v.widgets.open.weak_bg_fill = lift(0.10);
        v.widgets.open.bg_stroke = edge;
        // A text field's inside: darker than the dialog in a dark theme,
        // lighter in a light one, as the editor's own background is.
        v.text_edit_bg_color = Some(mix(self.menu, self.bg, 0.6));
        v.hyperlink_color = self.accent;
        v
    }
}

/// Black or white, whichever reads on `bg`.
pub fn text_on(bg: Color32) -> Color32 {
    match luminance(bg) > 140.0 {
        true => Color32::BLACK,
        false => Color32::WHITE,
    }
}

/// Whether a theme is light, by its background; None if it names none.
pub fn is_light(app: &App, theme: &leoapp::theme::Theme) -> Option<bool> {
    let bg = colour(app, theme.face("ui.background").bg)?;
    Some(luminance(bg) >= 128.0)
}

/// A terminal palette entry: the sixteen named colours, then xterm's 6x6x6
/// cube and grey ramp. The sixteen are One Dark's: a terminal's own, not
/// xterm's saturated defaults, whose pure green and blue glare in a window.
pub fn ansi(n: u8) -> Color32 {
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
    let (r, g, b) = match n {
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
    };
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Document;

    #[test]
    fn the_palette_is_one_darks_sixteen_then_xterms() {
        assert_eq!(ansi(3), Color32::from_rgb(229, 192, 123));
        assert_eq!(ansi(16), Color32::from_rgb(0, 0, 0));
        assert_eq!(ansi(21), Color32::from_rgb(0, 0, 255));
        assert_eq!(ansi(196), Color32::from_rgb(255, 0, 0));
        assert_eq!(ansi(232), Color32::from_rgb(8, 8, 8));
        assert_eq!(ansi(255), Color32::from_rgb(238, 238, 238));
    }

    #[test]
    fn a_colour_mixes_toward_another() {
        let grey = mix(Color32::BLACK, Color32::WHITE, 0.5);
        assert_eq!(grey, Color32::from_gray(128));
    }

    #[test]
    fn the_builtin_theme_still_gives_every_part_a_colour() {
        let mut app = App::new(Document::new_empty(""));
        app.depth = Depth::True;
        for light in [false, true] {
            let p = Palette::of(&app, light);
            assert_eq!(p.dark, !light);
            // The modes Helix names fall back to the badge colours.
            assert_ne!(p.mode(Mode::Normal).0, p.mode(Mode::Insert).0);
            assert_ne!(
                p.mark(Severity::Error).gutter,
                p.mark(Severity::Hint).gutter
            );
        }
    }
}
