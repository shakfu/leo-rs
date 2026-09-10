//! The user's settings file, `~/.config/leotui/config.toml`.
//!
//! TOML, in the subset `theme` already reads: `key = "value"` lines and `#`
//! comments. Two keys: `theme` and `split-ratio`. A line it does not understand is kept
//! as a warning for the status line rather than refused: a typo in a settings
//! file should cost that setting, not the session.

use std::io;
use std::path::{Path, PathBuf};

use crate::theme::{strip_comment, unquote};

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset.
pub fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// Where the settings file lives.
pub fn path() -> Option<PathBuf> {
    config_home().map(|c| c.join("leotui/config.toml"))
}

/// What the settings file says.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub theme: Option<String>,
    /// The outline pane's share of the width, in percent, as `:set split=N`.
    pub split_ratio: Option<u16>,
    /// Lines that were read but meant nothing, first one first.
    pub warnings: Vec<String>,
}

/// Read the settings file. A missing file is an empty one.
pub fn load() -> Config {
    match path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => parse(&text),
        None => Config::default(),
    }
}

fn parse(text: &str) -> Config {
    let mut config = Config::default();
    for (i, line) in text.lines().enumerate() {
        let line = strip_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        let n = i + 1;
        let Some((key, value)) = line.split_once('=') else {
            config
                .warnings
                .push(format!("config line {n}: expected key = value"));
            continue;
        };
        match (unquote(key.trim()), unquote(value)) {
            ("theme", "") => config
                .warnings
                .push(format!("config line {n}: theme is empty")),
            ("theme", name) => config.theme = Some(name.to_string()),
            ("split-ratio", v) => match v.parse::<u16>() {
                Ok(pct) => config.split_ratio = Some(pct.clamp(15, 85)),
                Err(_) => config
                    .warnings
                    .push(format!("config line {n}: split-ratio is not a number: {v}")),
            },
            (other, _) => config
                .warnings
                .push(format!("config line {n}: unknown setting {other}")),
        }
    }
    config
}

/// The theme to start with, and whether the user asked for it by name.
///
/// `--theme` beats the file, and the file beats the built-in default. Only a
/// theme that was asked for is worth a "not found" on the status line.
pub fn chosen_theme<'a>(
    arg: Option<&'a str>,
    config: &'a Config,
    default: &'a str,
) -> (&'a str, bool) {
    match (arg, config.theme.as_deref()) {
        (Some(name), _) | (None, Some(name)) => (name, true),
        (None, None) => (default, false),
    }
}

/// Record `name` as the theme in the settings file at `path`.
pub fn save_theme(path: &Path, name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains(['"', '\\', '\n', '\r']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a theme name: {name:?}"),
        ));
    }
    save(path, "theme", &format!("\"{name}\""))
}

/// Record `percent` as the outline pane's width in the settings file at `path`.
pub fn save_split_ratio(path: &Path, percent: u16) -> io::Result<()> {
    save(path, "split-ratio", &percent.to_string())
}

/// Set `key` to `value`, already written as TOML, in the settings file at `path`.
///
/// Only that line changes: comments, blank lines and settings this version
/// does not know survive byte for byte. The new file replaces the old by a
/// rename, so a crash mid-write leaves the old one whole. A file that exists
/// but cannot be read is an error rather than something to overwrite.
fn save(path: &Path, key: &str, value: &str) -> io::Result<()> {
    // A dotfiles manager links the settings file from elsewhere. Renaming
    // onto the link would swap it for a copy and cut it off, so write to
    // what it points at.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, with_setting(&text, key, value))?;
    if let Ok(meta) = std::fs::metadata(&path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// `text` with its top-level `key` set to `value`.
///
/// `parse` obeys the last top-level line for a key, so that is the one
/// rewritten, keeping any comment after it. With none, a line goes in before
/// the first `[table]`, where TOML still reads it as top-level.
fn with_setting(text: &str, key: &str, value: &str) -> String {
    let setting = format!("{key} = {value}");
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let top = lines
        .iter()
        .position(|l| strip_comment(l).trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let existing = lines[..top].iter().rposition(|l| {
        strip_comment(l)
            .split_once('=')
            .is_some_and(|(k, _)| unquote(k.trim()) == key)
    });
    match existing {
        Some(i) => {
            let code = strip_comment(&lines[i]).trim_end().len();
            let rest = &lines[i][code..];
            let comment = if rest.contains('#') { rest } else { "" };
            lines[i] = format!("{setting}{comment}");
        }
        None => lines.insert(top, setting),
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_is_read_with_or_without_quotes() {
        assert_eq!(
            parse("theme = \"onedark\"").theme.as_deref(),
            Some("onedark")
        );
        assert_eq!(parse("theme = 'onedark'").theme.as_deref(), Some("onedark"));
        assert_eq!(parse("theme=onedark").theme.as_deref(), Some("onedark"));
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let c = parse("# my settings\n\ntheme = \"onedark\"  # dark, for now\n");
        assert_eq!(c.theme.as_deref(), Some("onedark"));
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
    }

    #[test]
    fn a_line_that_means_nothing_is_a_warning_and_not_a_failure() {
        let c = parse("them = \"onedark\"\nnonsense\ntheme = \"\"\ntheme = \"nord\"");
        assert_eq!(c.theme.as_deref(), Some("nord"));
        assert_eq!(
            c.warnings,
            [
                "config line 1: unknown setting them",
                "config line 2: expected key = value",
                "config line 3: theme is empty",
            ]
        );
    }

    #[test]
    fn an_empty_file_says_nothing() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn the_flag_beats_the_file_and_the_file_beats_the_default() {
        let file = parse("theme = \"nord\"");
        let none = Config::default();
        assert_eq!(
            chosen_theme(Some("onedark"), &file, "sonokai"),
            ("onedark", true)
        );
        assert_eq!(chosen_theme(None, &file, "sonokai"), ("nord", true));
        assert_eq!(chosen_theme(None, &none, "sonokai"), ("sonokai", false));
    }

    /// `text` with its theme set to `name`, as `save_theme` writes it.
    fn with_theme(text: &str, name: &str) -> String {
        with_setting(text, "theme", &format!("\"{name}\""))
    }

    #[test]
    fn saving_rewrites_only_the_theme_line() {
        let text = "# mine\ntheme = \"onedark\"  # dark, for now\n\nother = 1\n";
        assert_eq!(
            with_theme(text, "nord"),
            "# mine\ntheme = \"nord\"  # dark, for now\n\nother = 1\n"
        );
    }

    #[test]
    fn saving_adds_a_theme_line_where_toml_reads_it_as_top_level() {
        assert_eq!(with_theme("", "nord"), "theme = \"nord\"\n");
        assert_eq!(with_theme("# mine\n", "nord"), "# mine\ntheme = \"nord\"\n");
        // After a table header it would belong to the table.
        assert_eq!(
            with_theme("# mine\n[keys]\nx = 1\n", "nord"),
            "# mine\ntheme = \"nord\"\n[keys]\nx = 1\n"
        );
    }

    #[test]
    fn the_split_ratio_is_read_clamped_and_saved_beside_the_theme() {
        assert_eq!(parse("split-ratio = 25").split_ratio, Some(25));
        assert_eq!(parse("split-ratio = 5").split_ratio, Some(15));
        let bad = parse("split-ratio = wide");
        assert_eq!(bad.split_ratio, None);
        assert_eq!(
            bad.warnings,
            ["config line 1: split-ratio is not a number: wide"]
        );
        let text = "theme = \"nord\"\nsplit-ratio = 40  # narrow\n";
        let saved = with_setting(text, "split-ratio", "25");
        assert_eq!(saved, "theme = \"nord\"\nsplit-ratio = 25  # narrow\n");
        assert_eq!(parse(&saved).theme.as_deref(), Some("nord"));
    }

    #[test]
    fn saving_rewrites_the_theme_line_that_reading_obeys() {
        // `parse` takes the last of two, so that is the one to change.
        let saved = with_theme("theme = \"a\"\ntheme = \"b\"\n", "nord");
        assert_eq!(saved, "theme = \"a\"\ntheme = \"nord\"\n");
        assert_eq!(parse(&saved).theme.as_deref(), Some("nord"));
    }

    /// A directory of the test's own, removed when it goes out of scope.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("leotui-config-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn saving_creates_the_directory_and_the_file() {
        let s = Scratch::new("create");
        let path = s.0.join("leotui/config.toml");
        save_theme(&path, "nord").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme = \"nord\"\n"
        );
    }

    #[test]
    fn a_name_that_would_break_the_file_is_refused() {
        let s = Scratch::new("refuse");
        let path = s.0.join("config.toml");
        for name in ["", "a\"b", "a\nb", "a\\b"] {
            assert!(save_theme(&path, name).is_err(), "{name:?} was written");
        }
        assert!(!path.exists());
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_alone() {
        let s = Scratch::new("unreadable");
        std::fs::create_dir_all(&s.0).unwrap();
        let path = s.0.join("config.toml");
        let bytes: &[u8] = b"theme = \"a\"\n\xff\xfe not utf-8\n";
        std::fs::write(&path, bytes).unwrap();
        assert!(save_theme(&path, "nord").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let s = Scratch::new("symlink");
        std::fs::create_dir_all(&s.0).unwrap();
        let real = s.0.join("real.toml");
        std::fs::write(&real, "# tracked\n").unwrap();
        let link = s.0.join("config.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        save_theme(&link, "nord").unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read_to_string(&real).unwrap(),
            "# tracked\ntheme = \"nord\"\n"
        );
    }
}
