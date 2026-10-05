//! The window: menu bar, outline sidebar, tabs and body, bottom panel and
//! status bar, and who gets the keys.
//!
//! The keys go to the app, as in leotui, except while a text field of the
//! window's own has them: the palette's, a picker's or the find panel's.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Key, Pos2};
use leoapp::app::{App, Mode};
use leoapp::config::{Appearance, Config};

use crate::editor::{self, Editor};
use crate::find::FindPanel;
use crate::goto::GoToNode;
use crate::input::{Action, Translator};
use crate::menus;
use crate::outlines::{self, Parked, Pick};
use crate::palette::CommandPalette;
use crate::panel::{self, PanelTab};
use crate::prompts::{self, Prompts};
use crate::session::{self, SavedOutline, Session};
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
    goto: GoToNode,
    find: FindPanel,
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
    /// Every open outline in tab order, None at `active`: that one is
    /// `app`, `tree` and `tabs`.
    outlines: Vec<Option<Parked>>,
    active: usize,
    /// Quitting closes the outlines one by one, each asking if it must.
    closing_all: bool,
    /// File > Open Recent's outlines, newest first, and where they are kept.
    recent: Vec<String>,
    recent_path: Option<PathBuf>,
    /// The active outline's file as last added to `recent`.
    recorded: String,
    /// Where the session is written on quit; None writes none. The window's
    /// size.
    pub session_path: Option<PathBuf>,
    window: Option<[f32; 2]>,
    wake: leolsp::server::Wake,
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
        let recent_path = outlines::recent_path();
        let recent = recent_path
            .as_deref()
            .map(outlines::load_recent)
            .unwrap_or_default();
        let c = ctx.clone();
        let wake: leolsp::server::Wake = std::sync::Arc::new(move || c.request_repaint());
        Gui {
            app,
            input: Translator::new(cmd_is_ctrl),
            palette: CommandPalette::default(),
            goto: GoToNode::default(),
            find: FindPanel::default(),
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
            outlines: vec![None],
            active: 0,
            closing_all: false,
            recent,
            recent_path,
            recorded: String::new(),
            session_path: session::path(),
            window: None,
            wake,
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
    fn apply_settings(&mut self, mut s: Config) {
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
        let wake = self.wake.clone();
        let restart = s.lsp != old.lsp || s.servers != old.servers;
        if restart {
            self.app.set_lsp(&s, wake.clone());
        }
        for parked in self.outlines.iter_mut().flatten() {
            parked.app.settings = s.clone();
            if restart {
                parked.app.set_lsp(&s, wake.clone());
            }
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

    /// Go to node's chord: Ctrl-P, Cmd-P on macOS.
    fn goto_chord(event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key: Key::P, pressed: true, modifiers, .. }
            if !modifiers.shift && !modifiers.alt && (modifiers.command || modifiers.ctrl))
    }

    /// The code actions' chord: Ctrl-., Cmd-. on macOS, as VS Code's.
    fn code_action_chord(event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key: Key::Period, pressed: true, modifiers, .. }
            if !modifiers.shift && !modifiers.alt && (modifiers.command || modifiers.ctrl))
    }

    /// The find panel's chord: Ctrl-Shift-F, Cmd-Shift-F on macOS.
    fn find_chord(event: &egui::Event) -> bool {
        matches!(event, egui::Event::Key { key: Key::F, pressed: true, modifiers, .. }
            if modifiers.shift && (modifiers.command || modifiers.ctrl))
    }

    /// Whether a text field of the window's own has the keys.
    fn typing(&self) -> bool {
        self.palette.open
            || self.goto.open
            || self.themes.open
            || self.settings.open
            || self.find.editing
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
                    self.closing_all = true;
                }
                continue;
            }
            if Self::quit_chord(event) {
                self.palette.open = false;
                self.goto.open = false;
                self.quit_all();
                continue;
            }
            if Self::palette_chord(event) {
                self.goto.open = false;
                match self.palette.open {
                    true => self.palette.open = false,
                    false => self.palette.show_palette(),
                }
                continue;
            }
            if Self::goto_chord(event) {
                self.palette.open = false;
                match self.goto.open {
                    true => self.goto.open = false,
                    false => self.goto.show_picker(),
                }
                continue;
            }
            if Self::code_action_chord(event) && !self.typing() {
                self.app.run_chosen("lsp-code-action");
                continue;
            }
            if Self::find_chord(event) {
                self.take_action(menus::Action::Find);
                continue;
            }
            if Self::settings_chord(event) {
                self.settings.show_dialog(&self.app);
                continue;
            }
            // A text field of the palette, a picker or the settings has the
            // keys.
            if self.typing() {
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
            menus::Action::GoToNode => self.goto.show_picker(),
            menus::Action::Find => {
                self.panel = Some(PanelTab::Find);
                self.find.focus();
            }
            menus::Action::CodeActions => {
                self.app.run_chosen("lsp-code-action");
            }
            menus::Action::New => {
                self.open_outline(None);
            }
            menus::Action::Open => {
                if let Some(path) = outlines::pick_outline(&self.app.outline().file_name) {
                    self.open_outline(Some(&path));
                }
            }
            menus::Action::OpenRecent(i) => {
                if let Some(path) = self.recent.get(i).cloned() {
                    self.open_outline(Some(&path));
                }
            }
            menus::Action::SaveAs | menus::Action::SaveCopy => {
                let command = match action {
                    menus::Action::SaveAs => "save-as!",
                    _ => "save-to!",
                };
                if let Some(path) = outlines::pick_save(&self.app.outline().file_name) {
                    self.app.run_chosen(&format!("{command} {path}"));
                }
            }
            menus::Action::Import => {
                if let Some(path) = outlines::pick_file(&self.app.outline().file_name) {
                    self.app.run_chosen(&format!("import-at-file {path}"));
                }
            }
            menus::Action::CloseOutline => self.app.request_quit(),
            menus::Action::Quit => self.quit_all(),
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

    /// Open `path`, or a new unsaved outline, in a tab of its own; or show
    /// it, if it is open. An untouched unsaved outline gives up its tab.
    /// False, with a message, if it could not be opened.
    pub fn open_outline(&mut self, path: Option<&str>) -> bool {
        if let Some(path) = path {
            let full = leolib::util::finalize(path);
            let open =
                (0..self.outlines.len()).find(|&i| self.app_at(i).outline().file_name == full);
            if let Some(i) = open {
                self.switch_to(i);
                return self.active == i;
            }
        }
        let mut app = match self.app.open_beside(path, self.wake.clone()) {
            Ok(app) => app,
            Err(e) => {
                self.app.message = format!("open failed: {e}");
                self.app.log_message();
                return false;
            }
        };
        let o = self.app.outline();
        let untouched = o.file_name.is_empty() && !o.changed && path.is_some();
        if untouched {
            // The MCP server stays in the window.
            app.mcp = self.app.mcp.take();
            self.install(app);
            return true;
        }
        self.outlines.insert(self.active + 1, None);
        let parked = self.install(app);
        self.outlines[self.active] = Some(parked);
        self.active += 1;
        true
    }

    /// The app of outline `i`, shown or not.
    fn app_at(&self, i: usize) -> &App {
        match &self.outlines[i] {
            Some(parked) => &parked.app,
            None => &self.app,
        }
    }

    /// Show `app` in place of the active outline, with a fresh sidebar and
    /// tabs, and return what it replaced.
    fn install(&mut self, mut app: App) -> Parked {
        app.carry_session(&self.app);
        self.forget_positions();
        Parked {
            app: std::mem::replace(&mut self.app, app),
            tree: std::mem::take(&mut self.tree),
            tabs: std::mem::take(&mut self.tabs),
        }
    }

    /// Drop what holds positions of the outline that was shown.
    fn forget_positions(&mut self) {
        self.goto = GoToNode::default();
        self.find.forget();
    }

    /// Show outline `i`.
    pub fn switch_to(&mut self, i: usize) {
        if i == self.active || i >= self.outlines.len() {
            return;
        }
        if !self.app.run_chosen("") {
            return;
        }
        let parked = self.outlines[i].take().expect("parked");
        let shown = self.install(parked.app);
        self.tree = parked.tree;
        self.tabs = parked.tabs;
        self.outlines[self.active] = Some(shown);
        self.active = i;
    }

    /// Close the active outline and show its neighbour. The MCP server moves
    /// to the neighbour, so a client keeps a server while a window is open.
    fn drop_active(&mut self) {
        self.outlines.remove(self.active);
        self.active = self.active.min(self.outlines.len() - 1);
        let parked = self.outlines[self.active].take().expect("parked");
        let mcp = self.app.mcp.take();
        self.install(parked.app);
        self.tree = parked.tree;
        self.tabs = parked.tabs;
        if self.app.mcp.is_none() {
            self.app.mcp = mcp;
        }
    }

    /// Quit: save the session, then close every outline, each asking
    /// first if it must.
    fn quit_all(&mut self) {
        if let Some(path) = &self.session_path {
            if let Err(e) = session::save(path, &self.session()) {
                self.app.message = format!("session not saved: {e}");
                self.app.log_message();
            }
        }
        self.closing_all = true;
        self.app.request_quit();
    }

    /// The open outlines as a session. An outline never saved is left out;
    /// if it was shown, the saved one before it is.
    fn session(&self) -> Session {
        let mut s = Session {
            panel: self.panel.map(|p| p.name().to_string()),
            window: self.window,
            ..Default::default()
        };
        for i in 0..self.outlines.len() {
            let (app, tabs) = match &self.outlines[i] {
                Some(parked) => (&parked.app, &parked.tabs),
                None => (&self.app, &self.tabs),
            };
            let o = app.outline();
            if o.file_name.is_empty() {
                continue;
            }
            if i <= self.active {
                s.active = s.outlines.len();
            }
            s.outlines.push(SavedOutline {
                path: o.file_name.clone(),
                select: Some((app.current.gnx(o).to_string(), app.editor.cursor)),
                tabs: tabs.saved(),
            });
        }
        s
    }

    /// Open a saved session's outlines, as they were left. An outline whose
    /// file is gone is skipped.
    pub fn restore(&mut self, s: &Session) {
        let mut active = None;
        for (i, saved) in s.outlines.iter().enumerate() {
            if !std::path::Path::new(&saved.path).exists() {
                continue;
            }
            if !self.open_outline(Some(&saved.path)) {
                continue;
            }
            if let Some((gnx, cursor)) = &saved.select {
                self.app.restore_selection(gnx, *cursor);
            }
            self.tabs.restore(&saved.tabs);
            if i == s.active {
                active = Some(self.active);
            }
        }
        if let Some(i) = active {
            self.switch_to(i);
        }
        self.panel = s.panel.as_deref().and_then(PanelTab::from_name);
    }

    /// Act on a quit the active outline agreed to: close it, and go on to
    /// the next if the window is closing. The last closes the window.
    fn settle_quits(&mut self, ctx: &egui::Context) {
        loop {
            if !self.app.quit {
                // A "no" to the question ends the round.
                if self.closing_all && self.app.mode != Mode::Confirm {
                    self.closing_all = false;
                }
                return;
            }
            if self.outlines.len() == 1 {
                // The close happens on the next frame, so ask for one:
                // without it the window waits for the next input to go.
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx.request_repaint();
                return;
            }
            self.drop_active();
            if self.closing_all {
                self.app.request_quit();
            }
        }
    }

    /// Add the active outline's file to Open Recent when it changes.
    fn record_recent(&mut self) {
        let file = &self.app.outline().file_name;
        if file.is_empty() || *file == self.recorded {
            return;
        }
        self.recorded = file.clone();
        outlines::remember(&mut self.recent, file);
        if let Some(path) = &self.recent_path {
            if let Err(e) = outlines::save_recent(path, &self.recent) {
                self.app.message = format!("recent outlines not saved: {e}");
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
        if !self.typing() {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
        }
        self.app.poll();
        if let Some(after) = self.app.poll_after() {
            ctx.request_repaint_after(after);
        }
        // An outline not shown still hears from its servers and clients.
        for parked in self.outlines.iter_mut().flatten() {
            parked.app.poll();
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.app.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.quit_all();
        }
        self.record_recent();
        if let Some(r) = ctx.input(|i| i.viewport().inner_rect) {
            self.window = Some([r.width(), r.height()]);
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
            action = menus::bar(ui, &mut self.app, self.appearance, &self.recent);
        });
        let mut pick = None;
        if self.outlines.len() > 1 {
            let names: Vec<String> = (0..self.outlines.len())
                .map(|i| outlines::name(self.app_at(i)))
                .collect();
            let strip = egui::Frame::NONE
                .fill(colours.tab_strip)
                .inner_margin(egui::Margin::symmetric(8, 2));
            egui::Panel::top("outlines").frame(strip).show(ui, |ui| {
                pick = outlines::strip(ui, &names, self.active, &colours);
            });
        }
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
                    let open = panel::ui(ui, &mut self.app, &mut tab, &mut self.find, &colours);
                    self.panel = open.then_some(tab);
                });
        }

        if self.panel != Some(PanelTab::Find) {
            self.find.editing = false;
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
                disk_bar(ui, &mut self.app, &colours);
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
        self.goto.ui(&ctx, &mut self.app, &colours);
        self.themes.ui(&ctx, &mut self.app, &colours);
        if let Some(saved) = self.settings.ui(&ctx, &self.app, &colours) {
            self.apply_settings(saved.settings);
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
        match pick {
            Some(Pick::Show(i)) => self.switch_to(i),
            Some(Pick::Close(i)) => {
                self.switch_to(i);
                if self.active == i {
                    self.app.request_quit();
                }
            }
            None => {}
        }
        self.settle_quits(&ctx);
    }
}

/// A bar offering Reload or Keep while external files are changed on disk.
fn disk_bar(ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
    let changed = app.files_changed_on_disk();
    if changed.is_empty() {
        return;
    }
    let names: Vec<String> = changed
        .iter()
        .map(|p| leolib::util::short_file_name(p))
        .collect();
    let mut reload = false;
    let mut keep = false;
    egui::Frame::NONE
        .fill(colours.warning.gamma_multiply(0.18))
        .inner_margin(egui::Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let what = match names.len() {
                    1 => format!("{} changed on disk.", names[0]),
                    n => format!("{n} files changed on disk: {}.", names.join(", ")),
                };
                ui.label(egui::RichText::new(what).color(colours.fg));
                reload = ui
                    .button("Reload")
                    .on_hover_text("Read the files again, asking first over unwritten edits")
                    .clicked();
                keep = ui
                    .button("Keep")
                    .on_hover_text("Keep the outline's text; the next write overwrites the files")
                    .clicked();
            });
        });
    if (reload || keep) && app.run_chosen("") {
        match reload {
            true => app.reload_changed_files(),
            false => app.keep_changed_files(),
        }
        app.log_message();
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

    /// A window over an unsaved outline, writing no file of the user's.
    fn window(ctx: &egui::Context) -> Gui {
        let app = App::new(leolib::Document::new_empty(""));
        let mut gui = Gui::new(ctx, app, true, None, &Config::default(), None);
        gui.recent_path = None;
        gui.session_path = None;
        gui
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("leoegui-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn names(gui: &Gui) -> Vec<String> {
        (0..gui.outlines.len())
            .map(|i| outlines::name(gui.app_at(i)))
            .collect()
    }

    #[test]
    fn outlines_open_beside_each_other_once_each() {
        let ctx = egui::Context::default();
        let dir = scratch("tabs");
        let (a, b) = (dir.join("a.leo"), dir.join("b.leo"));
        let mut gui = window(&ctx);
        // The untouched unsaved outline gives up its tab.
        gui.open_outline(Some(a.to_str().unwrap()));
        gui.open_outline(Some(b.to_str().unwrap()));
        assert_eq!(names(&gui), ["a.leo", "b.leo"]);
        assert_eq!(gui.active, 1);
        gui.open_outline(Some(a.to_str().unwrap()));
        assert_eq!(names(&gui).len(), 2);
        assert_eq!(gui.active, 0);
        assert!(gui.app.outline().file_name.ends_with("a.leo"));
        gui.switch_to(1);
        assert!(gui.app.outline().file_name.ends_with("b.leo"));
        gui.open_outline(None);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names(&gui), ["a.leo", "b.leo", "<unsaved>"]);
        assert_eq!(gui.active, 2);
    }

    #[test]
    fn closing_an_outline_shows_its_neighbour() {
        let ctx = egui::Context::default();
        let dir = scratch("close");
        let mut gui = window(&ctx);
        gui.open_outline(Some(dir.join("a.leo").to_str().unwrap()));
        gui.open_outline(Some(dir.join("b.leo").to_str().unwrap()));
        std::fs::remove_dir_all(&dir).unwrap();
        gui.switch_to(0);
        gui.app.request_quit();
        gui.settle_quits(&ctx);
        assert_eq!(names(&gui), ["b.leo"]);
        assert!(!gui.app.quit);
    }

    #[test]
    fn quitting_asks_of_each_outline_and_a_no_stops_it() {
        let ctx = egui::Context::default();
        let dir = scratch("quit");
        let mut gui = window(&ctx);
        gui.open_outline(Some(dir.join("a.leo").to_str().unwrap()));
        gui.open_outline(Some(dir.join("b.leo").to_str().unwrap()));
        std::fs::remove_dir_all(&dir).unwrap();
        // `a` has unsaved work; `b`, shown, has none.
        gui.switch_to(0);
        let root = gui.app.current.clone();
        gui.app.doc.set_headline(&root, "edited");
        gui.switch_to(1);
        gui.quit_all();
        gui.settle_quits(&ctx);
        assert_eq!(names(&gui), ["a.leo *"]);
        assert_eq!(gui.app.mode, Mode::Confirm);
        gui.app.answer(false);
        gui.settle_quits(&ctx);
        assert!(!gui.closing_all);
        assert_eq!(names(&gui), ["a.leo *"]);
    }

    /// A saved outline at `path`: a root and a child with a two-line body.
    fn saved_outline(path: &std::path::Path) {
        let mut doc = leolib::Document::new_empty(path.to_str().unwrap());
        let root = doc.outline().root_position().unwrap();
        doc.set_headline(&root, "root");
        let child = doc.outline_mut_untracked().insert_as_last_child(&root);
        doc.set_headline(&child, "child");
        doc.set_body(&child, "one\ntwo\n");
        doc.save("").unwrap();
    }

    #[test]
    fn a_session_reopens_the_outlines_as_they_were() {
        let ctx = egui::Context::default();
        let dir = scratch("restore");
        let (a, b) = (dir.join("a.leo"), dir.join("b.leo"));
        saved_outline(&a);
        saved_outline(&b);
        let mut gui = window(&ctx);
        gui.open_outline(Some(a.to_str().unwrap()));
        let o = gui.app.outline();
        let child = o.all_positions()[1].clone();
        let gnx = child.gnx(o).to_string();
        gui.app.restore_selection(&gnx, (1, 2));
        gui.tabs.track(&gui.app);
        gui.tabs.pin(&gnx);
        gui.open_outline(Some(b.to_str().unwrap()));
        gui.open_outline(None);
        gui.panel = Some(PanelTab::Find);
        let saved = gui.session();
        // The unsaved outline is left out; `b` was shown last of the rest.
        assert_eq!(saved.outlines.len(), 2);
        assert_eq!(saved.active, 1);

        let mut again = window(&ctx);
        again.restore(&session::parse(&session::render(&saved)));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names(&again), ["a.leo", "b.leo"]);
        assert_eq!(again.active, 1);
        assert_eq!(again.panel, Some(PanelTab::Find));
        again.switch_to(0);
        assert_eq!(again.app.current.h(again.app.outline()), "child");
        assert_eq!(again.app.editor.cursor, (1, 2));
        assert_eq!(again.tabs.saved(), [(gnx, true)]);
    }

    #[test]
    fn go_to_node_is_cmd_p_and_the_palette_cmd_shift_p() {
        let cmd = egui::Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        let shift = egui::Modifiers { shift: true, ..cmd };
        assert!(Gui::goto_chord(&key(Key::P, cmd)));
        assert!(!Gui::goto_chord(&key(Key::P, shift)));
        assert!(Gui::palette_chord(&key(Key::P, shift)));
        assert!(!Gui::palette_chord(&key(Key::P, cmd)));
    }
}
