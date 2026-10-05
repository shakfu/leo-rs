//! The settings dialog: every setting in `settings.toml`, edited as a
//! draft and written when saved. Only the keys that changed are written,
//! so comments and settings this version does not know survive.

use eframe::egui::{self, RichText};
use leoapp::app::App;
use leoapp::config::{self, Appearance, Config};

use crate::style::Palette;

#[derive(Default)]
pub struct SettingsDialog {
    pub open: bool,
    /// The settings as they were when the dialog opened, and the draft.
    original: Option<Config>,
    draft: Option<Config>,
    /// The language servers as editable rows: language, command.
    servers: Vec<(String, String)>,
    themes: Vec<String>,
    error: Option<String>,
}

/// What a save asks the window to apply that the app does not hold.
pub struct Saved {
    pub settings: Config,
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace(['"', '\\', '\n', '\r'], ""))
}

/// The settings file lines that turn `old` into `new`: changed keys only.
fn changes(old: &Config, new: &Config) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut put = |key: &str, a: String, b: String| {
        if a != b {
            out.push((key.to_string(), Some(b)));
        }
    };
    let name = |t: &Option<String>| t.as_deref().map(quoted).unwrap_or_default();
    put("theme", name(&old.theme), name(&new.theme));
    put(
        "theme-light",
        name(&old.theme_light),
        name(&new.theme_light),
    );
    put(
        "appearance",
        quoted(old.appearance.name()),
        quoted(new.appearance.name()),
    );
    let flag = |b: Option<bool>| b.map(|b| b.to_string()).unwrap_or_default();
    put("number", flag(old.number), flag(new.number));
    put("wrap", flag(old.wrap), flag(new.wrap));
    put("syntax", flag(old.syntax), flag(new.syntax));
    let pct = |p: Option<u16>| p.map(|p| p.to_string()).unwrap_or_default();
    put("split-ratio", pct(old.split_ratio), pct(new.split_ratio));
    put(
        "qt-mac-dont-swap-ctrl-and-meta",
        old.mac_dont_swap_ctrl_and_meta.to_string(),
        new.mac_dont_swap_ctrl_and_meta.to_string(),
    );
    put("lsp", old.lsp.to_string(), new.lsp.to_string());
    put(
        "mcp",
        old.mcp.enabled.to_string(),
        new.mcp.enabled.to_string(),
    );
    put(
        "mcp-edit",
        old.mcp.edit.to_string(),
        new.mcp.edit.to_string(),
    );
    put(
        "mcp-save",
        old.mcp.save.to_string(),
        new.mcp.save.to_string(),
    );
    put(
        "mcp-port",
        old.mcp.port.to_string(),
        new.mcp.port.to_string(),
    );
    put("mcp-token", name(&old.mcp.token), name(&new.mcp.token));
    // An empty value for a key that was never set is no change.
    out.retain(|(_, v)| v.as_deref() != Some(""));
    for s in &old.servers {
        if !new.servers.iter().any(|n| n.language == s.language) {
            out.push((format!("lsp-{}", s.language), None));
        }
    }
    for s in &new.servers {
        let was = old.servers.iter().find(|o| o.language == s.language);
        if was.map(|o| &o.command) != Some(&s.command) {
            out.push((format!("lsp-{}", s.language), Some(quoted(&s.command))));
        }
    }
    out
}

impl SettingsDialog {
    pub fn show_dialog(&mut self, app: &App) {
        self.open = true;
        self.error = None;
        self.original = Some(app.settings.clone());
        self.draft = Some(app.settings.clone());
        self.servers = app
            .settings
            .servers
            .iter()
            .map(|s| (s.language.clone(), s.command.clone()))
            .collect();
        self.themes = leoapp::theme::names();
    }

    /// Draw the dialog. Some when the user saved and the file was written.
    pub fn ui(&mut self, ctx: &egui::Context, app: &App, colours: &Palette) -> Option<Saved> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let (mut save, mut cancel) = (false, false);
        let Some(draft) = self.draft.as_mut() else {
            self.open = false;
            return None;
        };
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size([560.0, 620.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 70.0).show(ui, |ui| {
                    section(ui, "Appearance");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut draft.appearance, Appearance::Dark, "Dark");
                        ui.radio_value(&mut draft.appearance, Appearance::Light, "Light");
                        ui.radio_value(&mut draft.appearance, Appearance::System, "Match System");
                    });
                    egui::Grid::new("themes").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                        ui.label("Dark theme");
                        theme_combo(ui, "dark-theme", &mut draft.theme, &self.themes);
                        ui.end_row();
                        ui.label("Light theme");
                        theme_combo(ui, "light-theme", &mut draft.theme_light, &self.themes);
                        ui.end_row();
                    });

                    section(ui, "Editor");
                    tri(ui, &mut draft.number, "Line numbers", app.options.number);
                    tri(ui, &mut draft.wrap, "Wrap long lines", app.options.wrap);
                    tri(ui, &mut draft.syntax, "Syntax colouring", app.options.syntax);
                    ui.horizontal(|ui| {
                        ui.label("Outline width");
                        let mut pct = draft.split_ratio.unwrap_or(app.tree_percent);
                        if ui.add(egui::Slider::new(&mut pct, 15..=85).suffix("%")).changed() {
                            draft.split_ratio = Some(pct);
                        }
                    });

                    section(ui, "Keys");
                    ui.checkbox(
                        &mut draft.mac_dont_swap_ctrl_and_meta,
                        "On macOS, Cmd is Meta, not Leo's Ctrl (qt-mac-dont-swap-ctrl-and-meta)",
                    );

                    section(ui, "Language servers");
                    ui.checkbox(&mut draft.lsp, "Start language servers");
                    ui.add_enabled_ui(draft.lsp, |ui| {
                        egui::Grid::new("servers").num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                            ui.label(RichText::new("language").color(colours.dim));
                            ui.label(RichText::new("command").color(colours.dim));
                            ui.end_row();
                            let mut remove = None;
                            for (i, (language, command)) in self.servers.iter_mut().enumerate() {
                                ui.add(egui::TextEdit::singleline(language).desired_width(110.0).hint_text("python"));
                                ui.add(egui::TextEdit::singleline(command).desired_width(260.0).hint_text("pylsp"));
                                if ui.small_button("Remove").clicked() {
                                    remove = Some(i);
                                }
                                ui.end_row();
                            }
                            if let Some(i) = remove {
                                self.servers.remove(i);
                            }
                        });
                        if ui.button("Add a server").clicked() {
                            self.servers.push((String::new(), String::new()));
                        }
                    });
                    ui.label(
                        RichText::new("A server runs code from the project around the outline; name only ones you trust.")
                            .size(12.0)
                            .color(colours.dim),
                    );

                    section(ui, "MCP");
                    ui.checkbox(&mut draft.mcp.enabled, "Let MCP clients read the outline");
                    ui.add_enabled_ui(draft.mcp.enabled, |ui| {
                        ui.checkbox(&mut draft.mcp.edit, "Allow them to edit nodes");
                        ui.add_enabled_ui(draft.mcp.edit, |ui| {
                            ui.checkbox(&mut draft.mcp.save, "Allow them to save files");
                        });
                        ui.horizontal(|ui| {
                            ui.label("Port on 127.0.0.1");
                            ui.add(egui::DragValue::new(&mut draft.mcp.port).range(1024..=65535));
                        });
                        ui.horizontal(|ui| {
                            let token = draft.mcp.token.clone().unwrap_or_default();
                            let shown = match token.len() > 8 {
                                true => format!("{}...", &token[..8]),
                                false => "none yet: made when saved".to_string(),
                            };
                            ui.label(format!("Token: {shown}"));
                            if ui.small_button("New token").clicked() {
                                draft.mcp.token = Some(leomcp::new_token());
                            }
                        });
                        if let Some(token) = &draft.mcp.token {
                            let command = format!(
                                "claude mcp add --transport http leo http://127.0.0.1:{}/mcp --header \"Authorization: Bearer {token}\"",
                                draft.mcp.port
                            );
                            ui.label(RichText::new("To connect Claude Code:").size(12.0).color(colours.dim));
                            let mut shown = command.clone();
                            ui.add(egui::TextEdit::multiline(&mut shown).desired_rows(2).font(egui::TextStyle::Monospace));
                            if ui.small_button("Copy").clicked() {
                                ui.ctx().copy_text(command);
                            }
                        }
                        let status = match &app.mcp {
                            Some(s) => format!("running on 127.0.0.1:{}", s.port()),
                            None => "not running".to_string(),
                        };
                        ui.label(RichText::new(status).size(12.0).color(colours.dim));
                    });
                });
                ui.separator();
                if let Some(e) = &self.error {
                    ui.label(RichText::new(e).color(colours.warning));
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                    let path = app.config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
                    ui.label(RichText::new(path).size(11.5).color(colours.dim));
                });
            });
        if !open || cancel {
            self.open = false;
            return None;
        }
        if !save {
            return None;
        }
        let mut draft = self.draft.clone()?;
        draft.servers = self
            .servers
            .iter()
            .filter(|(l, c)| !l.trim().is_empty() && !c.trim().is_empty())
            .map(|(l, c)| leolsp::ServerConfig {
                language: l.trim().to_string(),
                command: c.trim().to_string(),
            })
            .collect();
        if draft.mcp.enabled && draft.mcp.token.is_none() {
            draft.mcp.token = Some(leomcp::new_token());
        }
        draft.mcp.save &= draft.mcp.edit;
        let original = self.original.clone().unwrap_or_default();
        if let Some(path) = &app.config_path {
            if let Err(e) = config::update(path, &changes(&original, &draft)) {
                self.error = Some(format!("not saved: {e}"));
                return None;
            }
        }
        self.open = false;
        Some(Saved { settings: draft })
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.add_space(2.0);
}

/// A checkbox for a setting that may be unset, showing what is in force.
fn tri(ui: &mut egui::Ui, value: &mut Option<bool>, label: &str, now: bool) {
    let mut on = value.unwrap_or(now);
    if ui.checkbox(&mut on, label).changed() {
        *value = Some(on);
    }
}

fn theme_combo(ui: &mut egui::Ui, id: &str, value: &mut Option<String>, themes: &[String]) {
    let shown = value.clone().unwrap_or_else(|| "(default)".to_string());
    egui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(220.0)
        .show_ui(ui, |ui| {
            for name in themes {
                if ui
                    .selectable_label(value.as_deref() == Some(name), name)
                    .clicked()
                {
                    *value = Some(name.clone());
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_changed_is_written() {
        let old = Config {
            servers: vec![leolsp::ServerConfig {
                language: "c".into(),
                command: "clangd".into(),
            }],
            ..Config::default()
        };
        let mut new = old.clone();
        assert!(changes(&old, &new).is_empty());
        new.lsp = false;
        new.mcp.enabled = true;
        new.mcp.token = Some("tok".into());
        new.servers = vec![leolsp::ServerConfig {
            language: "python".into(),
            command: "pylsp".into(),
        }];
        new.number = Some(true);
        let mut got = changes(&old, &new);
        got.sort();
        let want: Vec<(String, Option<String>)> = vec![
            ("lsp".into(), Some("false".into())),
            ("lsp-c".into(), None),
            ("lsp-python".into(), Some("\"pylsp\"".into())),
            ("mcp".into(), Some("true".into())),
            ("mcp-token".into(), Some("\"tok\"".into())),
            ("number".into(), Some("true".into())),
        ];
        assert_eq!(got, want);
    }
}
