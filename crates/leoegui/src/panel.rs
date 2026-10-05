//! The bottom panel: the node's problems, the message log, and find.

use eframe::egui::{self, RichText, Sense};
use leoapp::app::App;

use crate::find::FindPanel;
use crate::style::Palette;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    Problems,
    Log,
    Find,
    Servers,
}

impl PanelTab {
    /// The tab's name in the session file.
    pub fn name(self) -> &'static str {
        match self {
            PanelTab::Problems => "problems",
            PanelTab::Log => "log",
            PanelTab::Find => "find",
            PanelTab::Servers => "servers",
        }
    }

    pub fn from_name(name: &str) -> Option<PanelTab> {
        [
            PanelTab::Problems,
            PanelTab::Log,
            PanelTab::Find,
            PanelTab::Servers,
        ]
        .into_iter()
        .find(|t| t.name() == name)
    }
}

/// Draw the panel. False if it was closed.
pub fn ui(
    ui: &mut egui::Ui,
    app: &mut App,
    tab: &mut PanelTab,
    find: &mut FindPanel,
    colours: &Palette,
) -> bool {
    let mut open = true;
    ui.horizontal(|ui| {
        let problems = format!("Problems ({})", app.diagnostics.len());
        ui.selectable_value(tab, PanelTab::Problems, problems);
        ui.selectable_value(tab, PanelTab::Log, "Log");
        ui.selectable_value(tab, PanelTab::Find, "Find");
        ui.selectable_value(tab, PanelTab::Servers, "Servers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Close").clicked() {
                open = false;
            }
        });
    });
    ui.separator();
    if *tab == PanelTab::Find {
        find.ui(ui, app, colours);
        return open;
    }
    egui::ScrollArea::vertical()
        .id_salt(*tab as u8)
        .auto_shrink([false, false])
        .stick_to_bottom(matches!(*tab, PanelTab::Log | PanelTab::Servers))
        .show(ui, |ui| match tab {
            PanelTab::Problems => problems(ui, app, colours),
            PanelTab::Find => {}
            PanelTab::Servers => servers(ui, app, colours),
            PanelTab::Log => {
                for line in &app.messages {
                    ui.label(RichText::new(line).size(12.5));
                }
                if app.messages.is_empty() {
                    ui.label(RichText::new("no messages").color(colours.dim));
                }
            }
        });
    open
}

/// Each configured server's state, then what they logged.
fn servers(ui: &mut egui::Ui, app: &App, colours: &Palette) {
    for line in app.lsp_status_lines() {
        ui.label(RichText::new(line).size(12.5).strong());
    }
    let Some(lsp) = &app.lsp else { return };
    ui.separator();
    let mut any = false;
    for line in lsp.log() {
        any = true;
        ui.label(RichText::new(line).monospace().size(12.0));
    }
    if !any {
        ui.label(RichText::new("Nothing logged.").color(colours.dim));
    }
}

fn problems(ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
    if app.lsp.is_none() {
        ui.label(
            RichText::new("No language server. Name one in the settings: lsp-python = \"pylsp\".")
                .color(colours.dim),
        );
        return;
    }
    let mut go = None;
    for d in &app.diagnostics {
        let row = ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), Sense::hover());
            ui.painter()
                .circle_filled(dot.center(), 3.5, colours.mark(d.severity).gutter);
            ui.label(RichText::new(d.message.lines().next().unwrap_or("")).size(12.5));
            ui.label(
                RichText::new(format!("Ln {}, Col {}", d.row + 1, d.col + 1))
                    .size(12.0)
                    .color(colours.dim),
            );
        });
        let click = ui.interact(
            row.response.rect,
            ui.id().with((d.row, d.col)),
            Sense::click(),
        );
        if click.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if click.clicked() {
            go = Some((d.row, d.col));
        }
    }
    if app.diagnostics.is_empty() {
        ui.label(RichText::new("No problems in this node.").color(colours.dim));
    }
    if let Some((row, col)) = go {
        if app.run_chosen("") {
            let gnx = app.current.gnx(app.outline()).to_string();
            app.go_to_body(&gnx, row, col);
        }
    }
}
