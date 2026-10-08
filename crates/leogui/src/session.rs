//! Session restore, as Leo's `leo.session`: the open outlines, each one's
//! selection, body cursor and tabs, the bottom panel and the window size.
//!
//! Written when leogui quits, read when it starts with no outline named.
//! One line per fact, so a hand edit or an older file reads as far as it
//! makes sense:
//!
//! ```text
//! window 1280 800
//! panel find
//! outline /home/me/a.leo
//! active
//! select ekr.20240101.1 3 4
//! tab ekr.20240101.1
//! pinned ekr.20240101.7
//! ```

use std::path::{Path, PathBuf};

/// One outline as the session left it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SavedOutline {
    pub path: String,
    /// The selected node's gnx and the body cursor.
    pub select: Option<(String, (usize, usize))>,
    /// Each tab's gnx, and whether it is pinned.
    pub tabs: Vec<(String, bool)>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Session {
    pub outlines: Vec<SavedOutline>,
    /// The index of the outline shown.
    pub active: usize,
    /// The bottom panel's tab, if it was open: `problems`, `log` or `find`.
    pub panel: Option<String>,
    /// Whether the rendered view was open.
    pub rendered: bool,
    pub window: Option<[f32; 2]>,
}

/// Where the session is kept: beside the settings.
pub fn path() -> Option<PathBuf> {
    leoapp::config::path().map(|p| p.with_file_name("session"))
}

/// The session in `path`, if there is one.
pub fn load(path: &Path) -> Option<Session> {
    std::fs::read_to_string(path).ok().map(|s| parse(&s))
}

pub fn save(path: &Path, session: &Session) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, render(session))
}

pub fn parse(text: &str) -> Session {
    let mut s = Session::default();
    for line in text.lines() {
        let line = line.trim();
        let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let last = s.outlines.last_mut();
        match (word, last) {
            ("outline", _) if !rest.is_empty() => s.outlines.push(SavedOutline {
                path: rest.to_string(),
                ..Default::default()
            }),
            ("active", Some(_)) => s.active = s.outlines.len() - 1,
            ("select", Some(o)) => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if let [gnx, row, col] = parts[..] {
                    if let (Ok(row), Ok(col)) = (row.parse(), col.parse()) {
                        o.select = Some((gnx.to_string(), (row, col)));
                    }
                }
            }
            ("tab", Some(o)) if !rest.is_empty() => o.tabs.push((rest.to_string(), false)),
            ("pinned", Some(o)) if !rest.is_empty() => o.tabs.push((rest.to_string(), true)),
            ("panel", _) if !rest.is_empty() => s.panel = Some(rest.to_string()),
            ("rendered", _) => s.rendered = true,
            ("window", _) => {
                let size: Vec<f32> = rest
                    .split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect();
                if let [w, h] = size[..] {
                    s.window = Some([w, h]);
                }
            }
            _ => {}
        }
    }
    s
}

pub fn render(s: &Session) -> String {
    let mut out =
        String::from("# leogui session: written on quit, read when no outline is named.\n");
    if let Some([w, h]) = s.window {
        out += &format!("window {} {}\n", w.round(), h.round());
    }
    if let Some(panel) = &s.panel {
        out += &format!("panel {panel}\n");
    }
    if s.rendered {
        out += "rendered\n";
    }
    for (i, o) in s.outlines.iter().enumerate() {
        out += &format!("outline {}\n", o.path);
        if i == s.active {
            out += "active\n";
        }
        if let Some((gnx, (row, col))) = &o.select {
            out += &format!("select {gnx} {row} {col}\n");
        }
        for (gnx, pinned) in &o.tabs {
            let word = if *pinned { "pinned" } else { "tab" };
            out += &format!("{word} {gnx}\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_its_text() {
        let s = Session {
            outlines: vec![
                SavedOutline {
                    path: "/a b/one.leo".into(),
                    select: Some(("x.1".into(), (3, 4))),
                    tabs: vec![("x.1".into(), false), ("x.2".into(), true)],
                },
                SavedOutline {
                    path: "/two.leo".into(),
                    ..Default::default()
                },
            ],
            active: 1,
            panel: Some("find".into()),
            rendered: true,
            window: Some([1280.0, 800.0]),
        };
        assert_eq!(parse(&render(&s)), s);
    }

    #[test]
    fn a_line_that_makes_no_sense_is_skipped() {
        let s = parse("select x 1 2\noutline /a.leo\nselect x one 2\nwindow 9\nfuture thing\n");
        assert_eq!(s.outlines.len(), 1);
        assert_eq!(s.outlines[0].select, None);
        assert_eq!(s.window, None);
    }

    #[test]
    fn a_session_saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("leogui-session-{}", std::process::id()));
        let path = dir.join("sub/session");
        assert!(load(&path).is_none());
        let s = Session {
            panel: Some("log".into()),
            ..Default::default()
        };
        save(&path, &s).unwrap();
        let back = load(&path);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back, Some(s));
    }
}
