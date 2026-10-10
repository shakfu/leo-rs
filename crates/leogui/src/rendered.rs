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
    pub fn ui(&mut self, ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
        let body = leoapp::editor::join(&app.body_buffer());
        let shown = rendered(app.outline(), &app.current, &body);
        let width = ui.available_width();
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match shown {
                Rendered::Markdown { text, base } => {
                    // A relative image path starts from the outline's directory.
                    let scheme = format!("file://{}/", base.display());
                    // A link to a node is a hook: drawn without its URL on
                    // hover, and followed here rather than in a browser.
                    let hooks = node_links(&text);
                    self.cache.link_hooks_clear();
                    for url in &hooks {
                        self.cache.add_link_hook(*url);
                    }
                    CommonMarkViewer::new()
                        .default_implicit_uri_scheme(scheme)
                        .max_image_width(Some(width as usize))
                        .show(ui, &mut self.cache, &text);
                    clicked = hooks
                        .into_iter()
                        .find(|url| self.cache.get_link_hook(url) == Some(true))
                        .map(str::to_string);
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
        if let Some(url) = clicked {
            app.follow_url(&url);
        }
    }
}

/// The destinations of the inline markdown links in `text` that are Leo
/// links to a node, `[text](unl:...)` or `[text](gnx:...)`, which the app
/// follows.
fn node_links(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text
        .match_indices("](")
        .filter_map(|(i, _)| {
            let rest = &text[i + 2..];
            let url = &rest[..rest.find([')', ' ', '\n'])?];
            (url.starts_with("unl:") || url.starts_with("gnx:")).then_some(url)
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::node_links;

    #[test]
    fn only_links_to_nodes_are_hooked() {
        let text = "[a](unl:gnx://#x.1) [b](https://e.org) [c](gnx:y.2)\n[a](unl:gnx://#x.1)";
        assert_eq!(node_links(text), ["gnx:y.2", "unl:gnx://#x.1"]);
    }
}
