//! The rendered view, Leo's `viewrendered`: the selected node as markdown, an
//! image, or plain text, in a pane beside the body. `leoapp::rendered` decides
//! what a node is; this draws it.

use eframe::egui::{self, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use leoapp::app::App;
use leoapp::rendered::{rendered, Rendered};

use crate::style::Palette;

#[derive(Default)]
pub struct RenderedView {
    pub open: bool,
    cache: CommonMarkCache,
}

impl RenderedView {
    pub fn ui(&mut self, ui: &mut egui::Ui, app: &App, colours: &Palette) {
        let body = leoapp::editor::join(&app.body_buffer());
        let shown = rendered(app.outline(), &app.current, &body);
        let width = ui.available_width();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match shown {
                Rendered::Markdown { text, base } => {
                    // A relative image path starts from the outline's directory.
                    let scheme = format!("file://{}/", base.display());
                    CommonMarkViewer::new()
                        .default_implicit_uri_scheme(scheme)
                        .max_image_width(Some(width as usize))
                        .show(ui, &mut self.cache, &text);
                }
                Rendered::Image(Ok(path)) => {
                    let uri = format!("file://{}", path.display());
                    ui.add(egui::Image::new(uri).max_width(width).shrink_to_fit());
                }
                Rendered::Image(Err(why)) => {
                    ui.label(RichText::new(why).color(colours.warning));
                }
                Rendered::Text { language, text } => {
                    ui.label(
                        RichText::new(format!("{language} is shown as text"))
                            .size(11.5)
                            .color(colours.dim),
                    );
                    ui.add_space(4.0);
                    ui.label(RichText::new(text).monospace());
                }
                Rendered::Nothing(why) => {
                    ui.label(RichText::new(why).color(colours.dim));
                }
            });
    }
}
