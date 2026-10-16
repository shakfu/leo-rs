//! Colours: the theme's palette (`leoapp::palette`) as egui colours, and
//! egui's own widgets dressed in the theme, so menus and panels match the
//! body.

use eframe::egui::{self, Color32, Stroke};
use leoapp::app::App;
use leoapp::theme::{Colour, Depth};

/// The colours one frame draws with, read from the theme once.
pub type Palette = leoapp::palette::Palette<Color32>;

/// The theme's palette. `light` picks the defaults for what the theme
/// leaves out, as the built-in theme leaves out the background.
pub fn palette(app: &App, light: bool) -> Palette {
    leoapp::palette::Palette::of(&app.theme, app.depth, light).map(egui_colour)
}

/// A theme colour as egui's. A terminal colour is `ansi`'s.
pub fn colour(app: &App, c: Option<Colour>) -> Option<Color32> {
    if app.depth == Depth::None {
        return None;
    }
    c.map(|c| egui_colour(c.reduce(app.depth)))
}

fn egui_colour(c: Colour) -> Color32 {
    let (r, g, b) = leoapp::palette::rgb(c);
    Color32::from_rgb(r, g, b)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

fn luminance(c: Color32) -> f32 {
    0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32
}

/// egui's widgets in the theme's colours.
pub fn visuals(p: &Palette) -> egui::Visuals {
    let mut v = match p.dark {
        true => egui::Visuals::dark(),
        false => egui::Visuals::light(),
    };
    v.panel_fill = p.panel;
    v.window_fill = p.menu;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.line;
    // Text takes each state's colour rather than one override, which
    // egui puts before the selection's: a selected item's text is then
    // the theme's, as sonokai's black on green.
    v.override_text_color = None;
    v.selection.bg_fill = p.menu_selected;
    v.selection.stroke = Stroke::new(1.0, p.menu_selected_text);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.bg_fill = p.panel;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.fg);
    // A control's box -- a checkbox, a radio button, a slider's rail, a
    // text field's frame -- must stand off the dialog it sits on, which
    // is the menu colour. So each state is the menu colour lifted toward
    // the text, with an edge, rather than the theme's own fills, which
    // can match the dialog and vanish.
    let lift = |t: f32| mix(p.menu, p.fg, t);
    let edge = Stroke::new(1.0, lift(0.35));
    v.widgets.inactive.bg_fill = lift(0.14);
    v.widgets.inactive.weak_bg_fill = lift(0.08);
    v.widgets.inactive.bg_stroke = edge;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.fg);
    v.widgets.hovered.bg_fill = lift(0.24);
    v.widgets.hovered.weak_bg_fill = lift(0.16);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, lift(0.55));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, p.fg);
    v.widgets.active.bg_fill = lift(0.32);
    v.widgets.active.weak_bg_fill = p.menu_selected;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.5, p.fg);
    // A window's title bar and an open combo box take this: a quiet
    // step, not the menu's selection colour.
    v.widgets.open.weak_bg_fill = lift(0.10);
    v.widgets.open.fg_stroke = Stroke::new(1.0, p.fg);
    v.widgets.open.bg_stroke = edge;
    // A text field's inside: darker than the dialog in a dark theme,
    // lighter in a light one, as the editor's own background is.
    v.text_edit_bg_color = Some(mix(p.menu, p.bg, 0.6));
    v.hyperlink_color = p.accent;
    v
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

/// A terminal palette entry, as `leoapp::palette::ansi` gives it.
pub fn ansi(n: u8) -> Color32 {
    let (r, g, b) = leoapp::palette::ansi(n);
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Document;

    #[test]
    fn a_selected_item_takes_the_themes_menu_selected_text() {
        let mut app = App::new(Document::new_empty(""));
        app.depth = Depth::True;
        let p = palette(&app, false);
        // The builtin theme: white on blue.
        assert_eq!(p.menu_selected_text, ansi(15));
        assert_ne!(p.menu_selected_text, p.fg);
        let v = visuals(&p);
        assert_eq!(
            v.override_text_color, None,
            "it would win over the selection's"
        );
        assert_eq!(v.selection.stroke.color, p.menu_selected_text);
        assert_eq!(v.text_color(), p.fg);
    }

    #[test]
    fn a_colour_mixes_toward_another() {
        let grey = mix(Color32::BLACK, Color32::WHITE, 0.5);
        assert_eq!(grey, Color32::from_gray(128));
    }
}
