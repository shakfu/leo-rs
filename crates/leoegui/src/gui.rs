//! The window: menu bar, outline sidebar, tabs and body, bottom panel and
//! status bar, and who gets the keys.
//!
//! The keys go to the app, as in leotui, except while the command palette's
//! text field has them.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Pos2};
use leoapp::app::{App, Mode};
use leoapp::config::{Appearance, Config};

use crate::editor::{self, Editor};
use crate::input::{Action, Translator};
use crate::menus;
use crate::palette::CommandPalette;
use crate::panel::{self, PanelTab};
use crate::prompts::{self, Prompts};
use crate::settings::SettingsDialog;
use crate::status;
use crate::style::Palette;
use crate::tabs::{self, Tabs};
use crate::themes::ThemePicker;
use crate::tree::Tree;

pub struct Gui {
    app: App,
    input: Translator,
    palette: CommandPalette,
    themes: ThemePicker,
    settings: SettingsDialog,
    tree: Tree,
    tabs: Tabs,
    editor: Editor,
    prompts: Prompts,
    panel: Option<PanelTab>,
    about: bool,
    /// The theme name and lightness the widgets were last dressed for.
    theme: String,
    /// The settings' choice of light or dark, and the theme for each.
    appearance: Appearance,
    theme_dark: String,
    theme_light: String,
    /// Whether the window is light, as last applied.
    light: Option<bool>,
    title: String,
    /// The outline pane's share of the width last drawn, so a key that
    /// changes it (`Ctrl-w >`) moves the divider and a drag of the divider
    /// is kept.
    tree_percent: u16,
    hover_at: Option<Pos2>,
    screenshot: Option<PathBuf>,
    started: Instant,
    asked: bool,
}

/// The light theme when the settings name none: a Helix theme, so it is the
/// user's own if they have it.
const DEFAULT_LIGHT_THEME: &str = "onelight";

pub fn title(app: &App) -> String {
    let o = app.outline();
    let name = match o.file_name.is_empty() {
        true => "<unsaved>".to_string(),
        false => leolib::util::short_file_name(&o.file_name),
    };
    let changed = if o.changed { " *" } else { "" };
    format!("{name}{changed} - leoegui")
}

impl Gui {
    pub fn new(
        ctx: &egui::Context,
        mut app: App,
        cmd_is_ctrl: bool,
        screenshot: Option<PathBuf>,
        settings: &Config,
        theme_arg: Option<&str>,
    ) -> Self {
        // An editor shows line numbers; `:set nonumber` hides them.
        app.options.number = true;
        ctx.all_styles_mut(|s| {
            use egui::{FontId, TextStyle};
            s.text_styles
                .insert(TextStyle::Body, FontId::proportional(14.0));
            s.text_styles
                .insert(TextStyle::Button, FontId::proportional(14.0));
            s.text_styles
                .insert(TextStyle::Monospace, FontId::monospace(13.0));
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        });
        let title = title(&app);
        let tree_percent = app.tree_percent;
        Gui {
            app,
            input: Translator::new(cmd_is_ctrl),
            palette: CommandPalette::default(),
            themes: ThemePicker::default(),
            settings: SettingsDialog::default(),
            tree: Tree::default(),
            tabs: Tabs::default(),
            editor: Editor::default(),
            prompts: Prompts::default(),
            panel: None,
            about: false,
            theme: String::new(),
            appearance: settings.appearance,
            // `--theme` is this launch's theme, light or dark.
            theme_dark: theme_arg
                .or(settings.theme.as_deref())
                .unwrap_or(leoapp::app::DEFAULT_THEME)
                .to_string(),
            theme_light: theme_arg
                .or(settings.theme_light.as_deref())
                .unwrap_or(DEFAULT_LIGHT_THEME)
                .to_string(),
            light: None,
            title,
            tree_percent,
            hover_at: None,
            screenshot,
            started: Instant::now(),
            asked: false,
        }
    }

    /// Quit's chord: Leo's Ctrl-Q, and Cmd-Q on macOS.
    fn quit_chord(event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key: Key::Q, pressed: true, modifiers, .. }
            if !modifiers.shift && !modifiers.alt && (modifiers.command || modifiers.ctrl))
    }

    /// The settings' chord: Cmd-, on macOS, Ctrl-, elsewhere.
    fn settings_chord(event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key: Key::Comma, pressed: true, modifiers, .. }
            if !modifiers.shift && (modifiers.command || modifiers.ctrl))
    }

    /// Apply saved settings: what the window draws, and the servers.
    fn apply_settings(&mut self, ctx: &egui::Context, mut s: Config) {
        let old = self.app.settings.clone();
        self.appearance = s.appearance;
        self.theme_dark = s
            .theme
            .clone()
            .unwrap_or_else(|| leoapp::app::DEFAULT_THEME.to_string());
        self.theme_light = s
            .theme_light
            .clone()
            .unwrap_or_else(|| DEFAULT_LIGHT_THEME.to_string());
        self.light = None;
        if let Some(b) = s.number {
            self.app.options.number = b;
        }
        if let Some(b) = s.wrap {
            self.app.options.wrap = b;
        }
        if let Some(b) = s.syntax {
            self.app.options.syntax = b;
        }
        if let Some(p) = s.split_ratio {
            self.app.tree_percent = p;
        }
        self.input.cmd_is_ctrl = !s.mac_dont_swap_ctrl_and_meta;
        let c = ctx.clone();
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> =
            std::sync::Arc::new(move || c.request_repaint());
        if s.lsp != old.lsp || s.servers != old.servers {
            self.app.set_lsp(&s, wake.clone());
        }
        self.app.message = "settings saved".to_string();
        self.app.start_mcp(&mut s);
        if let Some(mcp) = &self.app.mcp {
            mcp.set_wake(wake);
        }
        self.app.settings = s;
        self.app.log_message();
    }

    /// The palette's chord: Ctrl-Shift-P, Cmd-Shift-P on macOS.
    fn palette_chord(event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key: Key::P, pressed: true, modifiers, .. }
            if modifiers.shift && (modifiers.command || modifiers.ctrl))
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        let (events, held) = ctx.input(|i| (i.events.clone(), i.modifiers));
        for event in &events {
            if let egui::Event::Screenshot { image, .. } = event {
                if let Some(path) = self.screenshot.take() {
                    if let Err(e) = crate::write_ppm(&path, image) {
                        eprintln!("leoegui: {}: {e}", path.display());
                    }
                    self.app.quit = true;
                }
                continue;
            }
            if Self::quit_chord(event) {
                self.palette.open = false;
                self.app.request_quit();
                continue;
            }
            if Self::palette_chord(event) {
                match self.palette.open {
                    true => self.palette.open = false,
                    false => self.palette.show_palette(),
                }
                continue;
            }
            if Self::settings_chord(event) {
                self.settings.show_dialog(&self.app);
                continue;
            }
            // A text field of the palette, the picker or the settings has
            // the keys.
            if self.palette.open || self.themes.open || self.settings.open {
                continue;
            }
            for action in self.input.translate(event, held) {
                match action {
                    // A dialog answers to one key: `y` or `n`, no Enter.
                    Action::Text(text) if self.app.mode == Mode::Confirm => {
                        match text.trim() {
                            "y" | "Y" => self.app.answer(true),
                            "n" | "N" => self.app.answer(false),
                            _ => {}
                        }
                        self.app.log_message();
                    }
                    Action::Key(key) => self.app.handle_key(key),
                    Action::Text(text) => self.app.handle_text(&text),
                    Action::Paste(text) => {
                        self.app.handle_paste(&text);
                        self.app.log_message();
                    }
                    Action::Preedit(text) => self.editor.preedit = text,
                    Action::Focused => {
                        self.app.check_disk();
                        self.app.log_message();
                    }
                }
            }
        }
    }

    fn take_action(&mut self, action: menus::Action) {
        match action {
            menus::Action::Palette => self.palette.show_palette(),
            menus::Action::Problems => self.panel = Some(PanelTab::Problems),
            menus::Action::Log => self.panel = Some(PanelTab::Log),
            menus::Action::About => self.about = true,
            menus::Action::Settings => self.settings.show_dialog(&self.app),
            menus::Action::Themes => {
                let light = self.light == Some(true);
                self.themes.show_picker(&self.app, light);
            }
            menus::Action::Appearance(a) => {
                self.appearance = a;
                if let Some(path) = &self.app.config_path {
                    if let Err(e) = leoapp::config::save_appearance(path, a) {
                        self.app.message = format!("appearance not saved: {e}");
                    }
                }
            }
        }
    }

    /// Load the theme the appearance asks for, when light or dark changes,
    /// and remember a `:theme` as the theme of the current one.
    fn follow_appearance(&mut self, ctx: &egui::Context) {
        let light = match self.appearance {
            Appearance::Light => true,
            Appearance::Dark => false,
            Appearance::System => ctx.system_theme() == Some(egui::Theme::Light),
        };
        if self.light == Some(light) {
            let name = self.app.theme.name().to_string();
            let mine = if light {
                &mut self.theme_light
            } else {
                &mut self.theme_dark
            };
            if *mine != name && name != leoapp::theme::Theme::builtin().name() {
                *mine = name;
            }
            return;
        }
        self.light = Some(light);
        let name = if light {
            &self.theme_light
        } else {
            &self.theme_dark
        };
        let message = self.app.message.clone();
        if !self.app.set_theme(name) {
            // A missing theme is said, and the built-in colours stand in.
            self.app.theme = leoapp::theme::Theme::builtin();
        } else {
            self.app.message = message;
        }
        self.app.theme_setting = if light { "theme-light" } else { "theme" };
        ctx.set_theme(if light {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        self.theme.clear();
    }
}

impl eframe::App for Gui {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_events(&ctx);
        // No widget keeps the keyboard but the palette: a clicked button
        // must not also take the Space or Enter the app is sent.
        if !self.palette.open && !self.themes.open && !self.settings.open {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        }
        self.app.poll();
        if let Some(after) = self.app.poll_after() {
            ctx.request_repaint_after(after);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.app.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.app.request_quit();
        }

        self.follow_appearance(&ctx);
        let colours = Palette::of(&self.app, self.light == Some(true));
        if self.theme != self.app.theme.name() {
            ctx.set_visuals(colours.visuals());
            self.theme = self.app.theme.name().to_string();
        }
        self.tabs.track(&self.app);

        let screen = ui.max_rect();
        let bar = egui::Frame::NONE
            .fill(colours.panel)
            .inner_margin(egui::Margin::symmetric(8, 2));
        let mut action = None;
        egui::Panel::top("menu").frame(bar).show(ui, |ui| {
            action = menus::bar(ui, &mut self.app, self.appearance);
        });
        let status_frame = egui::Frame::NONE
            .fill(colours.statusbar)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .stroke(egui::Stroke::new(1.0, colours.border));
        egui::Panel::bottom("status")
            .exact_size(26.0)
            .frame(status_frame)
            .show(ui, |ui| {
                if status::ui(ui, &mut self.app, &colours) {
                    action = Some(menus::Action::Problems);
                }
            });
        if let Some(mut tab) = self.panel {
            let frame = egui::Frame::NONE
                .fill(colours.panel)
                .inner_margin(egui::Margin::same(8))
                .stroke(egui::Stroke::new(1.0, colours.border));
            egui::Panel::bottom("panel")
                .resizable(true)
                .default_size(170.0)
                .frame(frame)
                .show(ui, |ui| {
                    let open = panel::ui(ui, &mut self.app, &mut tab, &colours);
                    self.panel = open.then_some(tab);
                });
        }

        // The divider: dragged, or moved by `Ctrl-w <` and `>`.
        let keyed = self.app.tree_percent != self.tree_percent;
        let width = screen.width() * f32::from(self.app.tree_percent) / 100.0;
        let mut side = egui::Panel::left("outline")
            .resizable(true)
            .size_range(160.0..=screen.width() * 0.7)
            .frame(
                egui::Frame::NONE
                    .fill(colours.panel)
                    .inner_margin(egui::Margin::same(4)),
            );
        side = match keyed {
            true => side.exact_size(width),
            false => side.default_size(width),
        };
        let shown = side.show(ui, |ui| {
            ui.label(
                egui::RichText::new("OUTLINE")
                    .size(11.5)
                    .color(colours.dim)
                    .strong(),
            );
            self.tree.ui(ui, &mut self.app, &colours, &mut self.tabs);
        });
        let percent = (shown.response.rect.width() / screen.width() * 100.0).round() as u16;
        if !keyed && percent != self.app.tree_percent {
            self.app.tree_percent = percent.clamp(10, 70);
        }
        self.tree_percent = self.app.tree_percent;

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(colours.bg))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::Frame::NONE.fill(colours.tab_strip).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    self.tabs.ui(ui, &mut self.app, &colours);
                });
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| tabs::breadcrumb(ui, &mut self.app, &colours));
                ui.separator();
                self.editor.ui(ui, &mut self.app, &colours);
            });
        self.hover_at = hover_anchor(&ctx);

        self.prompts
            .ui(&ctx, &mut self.app, &colours, self.hover_at);
        self.palette.ui(&ctx, &mut self.app, &colours);
        self.themes.ui(&ctx, &mut self.app, &colours);
        if let Some(saved) = self.settings.ui(&ctx, &self.app, &colours) {
            self.apply_settings(&ctx, saved.settings);
        }
        prompts::about(&ctx, &mut self.about);
        if let Some(a) = action {
            self.take_action(a);
        }

        let title = title(&self.app);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if self.screenshot.is_some() && !self.asked {
            let wait = Duration::from_secs(2).saturating_sub(self.started.elapsed());
            if wait.is_zero() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.asked = true;
            } else {
                ctx.request_repaint_after(wait);
            }
        }
        // The image arrives as an event some frames later, and only a frame
        // reads events.
        if self.screenshot.is_some() && self.asked {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if self.app.quit {
            // The close happens on the next frame, so ask for one: without it
            // the window waits for the next input to go.
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint();
        }
    }
}

/// Where the body's cursor was drawn, for a hover to open under. The IME
/// output carries it.
fn hover_anchor(ctx: &egui::Context) -> Option<Pos2> {
    ctx.output(|o| o.ime.as_ref().map(|i| i.cursor_rect.left_top()))
        .or_else(|| ctx.memory(|m| m.data.get_temp::<Pos2>(egui::Id::new(editor::CURSOR))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn cmd_q_and_ctrl_q_quit_and_nothing_else_does() {
        let cmd = egui::Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        let ctrl = egui::Modifiers {
            ctrl: true,
            command: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        assert!(Gui::quit_chord(&key(Key::Q, cmd)));
        assert!(Gui::quit_chord(&key(Key::Q, ctrl)));
        assert!(!Gui::quit_chord(&key(Key::Q, egui::Modifiers::NONE)));
        assert!(!Gui::quit_chord(&key(
            Key::Q,
            egui::Modifiers { shift: true, ..cmd }
        )));
        assert!(!Gui::quit_chord(&key(Key::W, cmd)));
    }
}
