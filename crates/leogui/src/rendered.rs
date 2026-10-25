//! The rendered view, Leo's `viewrendered`: the selected node as markdown, an
//! image, or plain text, in a pane beside the body. `leoapp::rendered` decides
//! what a node is; this draws it.

use eframe::egui::{self, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use leoapp::app::App;
use leoapp::rendered::{rendered, Rendered};
use leolib::Position;

use crate::style::Palette;

/// What a rendering was built from: the outline's generation, the node, the
/// body being typed and the outline's file, which relative paths start from.
type Key = (u64, Position, String, String);

#[derive(Default)]
pub struct RenderedView {
    pub open: bool,
    cache: CommonMarkCache,
    /// The last rendering, kept until its key changes: building one reads
    /// `include=` files and image paths from disk.
    shown: Option<(Key, Rendered)>,
}

impl RenderedView {
    /// Build the next frame afresh, to see files changed on disk.
    pub fn refresh(&mut self) {
        self.shown = None;
    }

    /// What the view shows for the current node, rebuilt only when it changed.
    fn shown(&mut self, app: &App) -> &Rendered {
        let o = app.outline();
        let body = leoapp::editor::join(&app.body_buffer());
        let key = (o.generation, app.current.clone(), body, o.file_name.clone());
        if self.shown.as_ref().is_none_or(|(k, _)| *k != key) {
            let built = rendered(o, &app.current, &key.2);
            self.shown = Some((key, built));
        }
        &self.shown.as_ref().expect("set above").1
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
        let shown = self.shown(app).clone();
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
    use super::*;
    use leolib::Document;

    #[test]
    fn a_rendering_is_rebuilt_only_when_the_node_changes() {
        let dir = std::env::temp_dir().join(format!("leogui-rendered-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let image = dir.join("a.png");
        std::fs::write(&image, "").unwrap();
        let mut app = App::new(Document::new_empty(""));
        let p = app.current.clone();
        app.doc.set_headline(&p, "@image");
        app.doc.set_body(&p, &format!("{}\n", image.display()));
        let mut view = RenderedView::default();
        assert_eq!(view.shown(&app), &Rendered::Image(Ok(image.clone())));
        // The disk is read once per change, not once per frame.
        std::fs::remove_file(&image).unwrap();
        assert_eq!(view.shown(&app), &Rendered::Image(Ok(image.clone())));
        view.refresh();
        let gone = view.shown(&app).clone();
        std::fs::write(&image, "").unwrap();
        app.doc.set_headline(&p, "@image ");
        let edited = view.shown(&app).clone();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(matches!(gone, Rendered::Image(Err(_))), "{gone:?}");
        assert_eq!(edited, Rendered::Image(Ok(image)));
    }

    #[test]
    fn only_links_to_nodes_are_hooked() {
        let text = "[a](unl:gnx://#x.1) [b](https://e.org) [c](gnx:y.2)\n[a](unl:gnx://#x.1)";
        assert_eq!(node_links(text), ["gnx:y.2", "unl:gnx://#x.1"]);
    }
}
