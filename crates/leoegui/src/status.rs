//! The status bar: mode, file, message, and the cursor's place.

use eframe::egui::{self, vec2, RichText, Sense};
use leoapp::app::{App, Focus};
use leoapp::view::Severity;

use crate::menus::Action;
use crate::style::{self, Palette};

/// Draw the bar. The panel to open, if the problem counts or the servers'
/// state were clicked.
pub fn ui(ui: &mut egui::Ui, app: &mut App, colours: &Palette) -> Option<Action> {
    let mut clicked = None;
    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.visuals_mut().override_text_color = Some(colours.statusbar_text);
        // The mode, as a pill.
        let label = app.mode.label();
        let (pill, ink) = colours.mode(app.mode);
        let text = RichText::new(label).strong().size(12.0).color(ink);
        let galley = egui::WidgetText::from(text.clone()).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        );
        let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(16.0, 4.0), Sense::hover());
        ui.painter().rect_filled(rect, 8.0, pill);
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            style::text_on(pill),
        );

        let o = app.outline();
        let name = match o.file_name.is_empty() {
            true => "<unsaved>".to_string(),
            false => leolib::util::short_file_name(&o.file_name),
        };
        let mut file = RichText::new(name).size(12.5);
        if o.changed {
            file = file.italics();
        }
        ui.label(file);
        if o.changed {
            ui.label(RichText::new("modified").size(12.0).color(colours.warning));
        }

        let message = match app.message.is_empty() {
            true => app.status_hint(),
            false => app.message.clone(),
        };
        ui.label(RichText::new(message).size(12.5).color(colours.dim));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rows, current) = (app.row_count(), app.current_row());
            ui.label(
                RichText::new(format!("node {}/{}", current + 1, rows))
                    .size(12.0)
                    .color(colours.dim),
            );
            if let Some(language) = leoapp::highlight::language_of(app.outline(), &app.current) {
                ui.label(RichText::new(language).size(12.0).color(colours.dim));
            }
            if app.focus == Focus::Body {
                let (row, col) = app.editor.cursor;
                ui.label(
                    RichText::new(format!("Ln {}, Col {}", row + 1, col + 1))
                        .size(12.0)
                        .color(colours.dim),
                );
            }
            let errors = app
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count();
            let warnings = app
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .count();
            if app.lsp.is_some() {
                let text = RichText::new(format!("E {errors}  W {warnings}"))
                    .size(12.0)
                    .color(if errors > 0 {
                        colours.mark(Severity::Error).gutter
                    } else {
                        colours.dim
                    });
                let link = ui.add(egui::Label::new(text).sense(Sense::click()));
                if link.clicked() {
                    clicked = Some(Action::Problems);
                }
                link.on_hover_text("Problems in this node");
                if servers(ui, app, colours) {
                    clicked = Some(Action::Servers);
                }
            }
        });
    });
    clicked
}

/// "LSP" with a dot for the servers' state: lit while one runs, the error
/// colour if one failed. True if clicked.
fn servers(ui: &mut egui::Ui, app: &App, colours: &Palette) -> bool {
    let Some(lsp) = &app.lsp else { return false };
    let states: Vec<leolsp::ServerState> = lsp.status().into_iter().map(|(_, s)| s).collect();
    let failed = states
        .iter()
        .any(|s| matches!(s, leolsp::ServerState::Failed(_)));
    let running = states.contains(&leolsp::ServerState::Running);
    let dot = if failed {
        colours.mark(Severity::Error).gutter
    } else if running {
        colours.accent
    } else {
        colours.dim
    };
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new("LSP").size(12.0).color(colours.dim));
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 12.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 3.5, dot);
        })
        .response
        .interact(Sense::click());
    let response = response.on_hover_text(app.lsp_status_lines().join("\n"));
    response.clicked()
}
