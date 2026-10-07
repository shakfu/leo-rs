//! The theme picker: every Helix theme on disk, light or dark by its
//! background, previewed as the pointer passes over it.

use eframe::egui::{self, Align2, Key, RichText};
use leoapp::app::App;
use leoapp::theme::Theme;

use crate::style::{self, Palette};

/// Which themes the list shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Show {
    Light,
    Dark,
    All,
}

#[derive(Default)]
pub struct ThemePicker {
    pub open: bool,
    query: String,
    show: Option<Show>,
    /// Every theme name, and whether it is light: None for a theme with no
    /// background. Read once, the first time the picker opens.
    themes: Option<Vec<(String, Option<bool>)>>,
    /// The theme in force when the picker opened, put back on Escape.
    origin: Option<String>,
    previewing: Option<String>,
    fresh: bool,
}

impl ThemePicker {
    /// Open it on the themes of the current appearance.
    pub fn show_picker(&mut self, app: &App, light: bool) {
        self.open = true;
        self.fresh = true;
        self.query.clear();
        self.show = Some(if light { Show::Light } else { Show::Dark });
        self.origin = Some(app.theme.name().to_string());
        self.previewing = None;
        if self.themes.is_none() {
            let themes = leoapp::theme::names()
                .into_iter()
                .map(|name| {
                    let light = Theme::load(&name).and_then(|t| style::is_light(app, &t));
                    (name, light)
                })
                .collect();
            self.themes = Some(themes);
        }
    }

    fn close(&mut self, app: &mut App, keep: bool) {
        self.open = false;
        if !keep {
            if let Some(name) = self.origin.take() {
                app.set_theme(&name);
            }
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context, app: &mut App, colours: &Palette) {
        if !self.open {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.close(app, false);
            return;
        }
        let current = app.theme.name().to_string();
        let mut open = true;
        let mut chosen: Option<String> = None;
        let mut hovered: Option<String> = None;
        egui::Window::new("Themes")
            .open(&mut open)
            .collapsible(false)
            .default_size([360.0, 480.0])
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let edit = egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search themes")
                    .desired_width(f32::INFINITY);
                let r = ui.add(edit);
                if self.fresh {
                    r.request_focus();
                    self.fresh = false;
                }
                let show = self.show.get_or_insert(Show::All);
                ui.horizontal(|ui| {
                    ui.selectable_value(show, Show::Dark, "Dark");
                    ui.selectable_value(show, Show::Light, "Light");
                    ui.selectable_value(show, Show::All, "All");
                });
                ui.separator();
                let show = *show;
                let query = self.query.to_lowercase();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (name, light) in self.themes.iter().flatten() {
                            let fits = match (show, light) {
                                (Show::All, _) => true,
                                (Show::Light, Some(l)) | (Show::Dark, Some(l)) => {
                                    *l == (show == Show::Light)
                                }
                                (_, None) => false,
                            };
                            if !fits || !name.to_lowercase().contains(&query) {
                                continue;
                            }
                            let r = ui.selectable_label(*name == current, name.as_str());
                            if r.hovered() {
                                hovered = Some(name.clone());
                            }
                            if r.clicked() {
                                chosen = Some(name.clone());
                            }
                        }
                    });
                ui.label(
                    RichText::new("Hover to preview, click to keep, Escape to go back")
                        .size(11.5)
                        .color(colours.dim),
                );
            });
        if let Some(name) = chosen {
            app.set_theme(&name);
            if let Some(path) = &app.config_path {
                app.message = match leoapp::config::save_theme_as(path, app.theme_setting, &name) {
                    Ok(()) => format!("theme: {name} (saved)"),
                    Err(e) => format!("theme: {name} (not saved: {e})"),
                };
            }
            self.close(app, true);
            return;
        }
        if !open {
            self.close(app, false);
            return;
        }
        // Preview what the pointer is on; leaving the list keeps the last one
        // previewed until a click or Escape decides.
        if let Some(name) = hovered {
            if self.previewing.as_ref() != Some(&name) {
                let message = app.message.clone();
                app.set_theme(&name);
                app.message = message;
                self.previewing = Some(name);
            }
        }
    }
}
