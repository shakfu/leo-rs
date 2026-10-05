//! Several outlines in one window: those not shown, the strip of their
//! tabs, and the list File > Open Recent offers.

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};
use leoapp::app::App;

use crate::style::Palette;
use crate::tabs::Tabs;
use crate::tree::Tree;

/// An outline open but not shown, with its sidebar and tabs.
pub struct Parked {
    pub app: App,
    pub tree: Tree,
    pub tabs: Tabs,
}

/// What a click on the strip asks for.
pub enum Pick {
    Show(usize),
    Close(usize),
}

/// An outline's name as a tab shows it.
pub fn name(app: &App) -> String {
    let o = app.outline();
    let name = match o.file_name.is_empty() {
        true => "<unsaved>".to_string(),
        false => leolib::util::short_file_name(&o.file_name),
    };
    match o.changed {
        true => format!("{name} *"),
        false => name,
    }
}

/// One tab per outline, `names` in order with `active` lit.
pub fn strip(
    ui: &mut egui::Ui,
    names: &[String],
    active: usize,
    colours: &Palette,
) -> Option<Pick> {
    let mut pick = None;
    ui.horizontal(|ui| {
        for (i, name) in names.iter().enumerate() {
            let text = match i == active {
                true => RichText::new(name).strong().color(colours.fg),
                false => RichText::new(name).color(colours.dim),
            };
            if ui.selectable_label(i == active, text).clicked() {
                pick = Some(Pick::Show(i));
            }
            let x = RichText::new("x").size(12.0).color(colours.dim);
            let close = ui
                .add(egui::Button::new(x).frame(false))
                .on_hover_text("Close this outline");
            if close.clicked() {
                pick = Some(Pick::Close(i));
            }
            ui.separator();
        }
    });
    pick
}

/// How many outlines Open Recent lists.
const RECENT: usize = 10;

/// Where the recent outlines are kept: beside the settings.
pub fn recent_path() -> Option<PathBuf> {
    leoapp::config::path().map(|p| p.with_file_name("recent-outlines"))
}

/// The recent outlines, newest first; none if the file cannot be read.
pub fn load_recent(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Put `file` first, once, keeping the newest `RECENT`.
pub fn remember(list: &mut Vec<String>, file: &str) {
    list.retain(|f| f != file);
    list.insert(0, file.to_string());
    list.truncate(RECENT);
}

pub fn save_recent(path: &Path, list: &[String]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, list.join("\n") + "\n")
}

/// Ask for an outline to open, starting where `near` is.
pub fn pick_outline(near: &str) -> Option<String> {
    dialog(near)
        .add_filter("Leo outline", &["leo", "leojs", "db"])
        .add_filter("Any file", &["*"])
        .pick_file()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Ask for any file to open, as an import.
pub fn pick_file(near: &str) -> Option<String> {
    dialog(near)
        .pick_file()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Ask where to save the outline. The dialog asks before replacing a file.
pub fn pick_save(near: &str) -> Option<String> {
    let name = Path::new(near)
        .file_name()
        .map_or("untitled.leo".into(), |n| n.to_string_lossy().into_owned());
    dialog(near)
        .set_file_name(name)
        .add_filter("Leo outline", &["leo"])
        .save_file()
        .map(|p| p.to_string_lossy().into_owned())
}

/// A dialog opening in `near`'s directory, if it names one.
fn dialog(near: &str) -> rfd::FileDialog {
    let dir = std::path::absolute(near)
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .filter(|d| !near.is_empty() && d.is_dir());
    match dir {
        Some(dir) => rfd::FileDialog::new().set_directory(dir),
        None => rfd::FileDialog::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recent_list_keeps_each_file_once_newest_first() {
        let mut list = Vec::new();
        for i in 0..12 {
            remember(&mut list, &format!("/f{i}.leo"));
        }
        assert_eq!(list.len(), RECENT);
        assert_eq!(list[0], "/f11.leo");
        remember(&mut list, "/f5.leo");
        assert_eq!(list[0], "/f5.leo");
        assert_eq!(list.iter().filter(|f| *f == "/f5.leo").count(), 1);
    }

    #[test]
    fn the_recent_list_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("leoegui-recent-{}", std::process::id()));
        let path = dir.join("sub/recent-outlines");
        assert!(load_recent(&path).is_empty());
        let list = vec!["/a b.leo".to_string(), "/c.leo".to_string()];
        save_recent(&path, &list).unwrap();
        let back = load_recent(&path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back, list);
    }
}
