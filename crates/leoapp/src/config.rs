//! The user's settings file, `~/.config/leo-rs/settings.toml`, read by
//! leotui and leogui both. An older `config.toml` beside it is renamed to it
//! once, and an older `~/.config/leotui/` directory to `leo-rs/`.
//!
//! TOML, in the subset `theme` already reads: `key = "value"` lines and `#`
//! comments. The keys:
//!
//! - `theme`, `theme-light`, `appearance`, `split-ratio`;
//! - `number`, `wrap`, `syntax`: what `:set` starts as;
//! - `lsp`, off to start no language server, and `lsp-LANGUAGE`, the
//!   command of the server for Leo's language `LANGUAGE`;
//! - `mcp`, `mcp-edit`, `mcp-save`, `mcp-port`, `mcp-token`: the MCP server;
//! - `entangled`, the command `:entangled-tangle` runs;
//! - leogui's `qt-mac-dont-swap-ctrl-and-meta`.
//!
//! A line it does not understand is kept as a warning for the status line
//! rather than refused: a typo in a settings file should cost that setting,
//! not the session.

use std::io;
use std::path::{Path, PathBuf};

use crate::theme::{strip_comment, unquote};

/// `$XDG_CONFIG_HOME`, or `~/.config` when it is unset.
pub fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

/// The directory both front ends keep their files in: the settings, the
/// session, the recent outlines and themes.
pub fn dir() -> Option<PathBuf> {
    config_home().map(|c| dir_in(&c))
}

/// `home/leo-rs`, or `home/leotui`, its earlier name, while only that
/// exists: `migrate_dir` could not rename it.
fn dir_in(home: &Path) -> PathBuf {
    let (new, old) = (home.join("leo-rs"), home.join("leotui"));
    match !new.exists() && old.exists() {
        true => old,
        false => new,
    }
}

/// Rename `home/leotui` to `home/leo-rs` if only the old one exists. The
/// rename keeps a link a dotfiles manager made. Startup does it, in `load`,
/// rather than every lookup, so reading a theme never moves a directory.
fn migrate_dir(home: &Path) {
    let (new, old) = (home.join("leo-rs"), home.join("leotui"));
    if !new.exists() && old.exists() {
        let _ = std::fs::rename(&old, &new);
    }
}

/// Where the settings file lives.
pub fn path() -> Option<PathBuf> {
    dir().map(|d| d.join("settings.toml"))
}

/// Rename the settings file's old name, `config.toml`, to `settings.toml`
/// if only the old one exists. The rename keeps the file's comments and any
/// link a dotfiles manager made.
fn migrate(settings: &Path) {
    let old = settings.with_file_name("config.toml");
    if !settings.exists() && old.exists() {
        let _ = std::fs::rename(&old, settings);
    }
}

/// The MCP server's settings. Off, and read-only once on, until the user
/// says otherwise: a client may edit only with `mcp-edit`, and write files
/// only with `mcp-save` as well.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mcp {
    pub enabled: bool,
    pub edit: bool,
    pub save: bool,
    pub port: u16,
    /// The bearer token a client must send. None until the server is first
    /// turned on, when one is made and saved.
    pub token: Option<String>,
}

impl Default for Mcp {
    fn default() -> Self {
        Mcp {
            enabled: false,
            edit: false,
            save: false,
            port: 7341,
            token: None,
        }
    }
}

/// Whether leogui is light or dark, or follows the system's choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

impl Appearance {
    pub fn name(self) -> &'static str {
        match self {
            Appearance::System => "system",
            Appearance::Dark => "dark",
            Appearance::Light => "light",
        }
    }
}

/// What the settings file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub theme: Option<String>,
    /// The outline pane's share of the width, in percent, as `:set split=N`.
    pub split_ratio: Option<u16>,
    /// Leo's setting of the same name: on macOS, Cmd is not Ctrl. leogui
    /// reads it; a terminal never sees Cmd.
    pub mac_dont_swap_ctrl_and_meta: bool,
    /// leogui's light or dark choice, and the theme it uses when light;
    /// `theme` is the dark one.
    pub appearance: Appearance,
    pub theme_light: Option<String>,
    /// False to start no language server, whatever `servers` names.
    pub lsp: bool,
    /// `lsp-python = "pylsp"`: a server per language, none unless named.
    pub servers: Vec<leolsp::ServerConfig>,
    /// What `:set number`, `wrap` and `syntax` start as, when given.
    pub number: Option<bool>,
    pub wrap: Option<bool>,
    pub syntax: Option<bool>,
    pub mcp: Mcp,
    /// `entangled = "/usr/local/bin/entangled"`: the command `:entangled-tangle`
    /// and `:entangled-check` run, split on blanks. `entangled` when unset.
    pub entangled: Option<String>,
    /// Lines that were read but meant nothing, first one first.
    pub warnings: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: None,
            split_ratio: None,
            mac_dont_swap_ctrl_and_meta: false,
            appearance: Appearance::default(),
            theme_light: None,
            lsp: true,
            servers: Vec::new(),
            number: None,
            wrap: None,
            syntax: None,
            mcp: Mcp::default(),
            entangled: None,
            warnings: Vec::new(),
        }
    }
}

/// Read the settings file. A missing file is an empty one.
pub fn load() -> Config {
    if let Some(home) = config_home() {
        migrate_dir(&home);
    }
    let Some(path) = path() else {
        return Config::default();
    };
    migrate(&path);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(_) => Config::default(),
    }
}

/// `true` or `false`, else a warning.
fn flag(key: &str, v: &str, n: usize, warnings: &mut Vec<String>) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => {
            warnings.push(format!("config line {n}: {key} is not true or false: {v}"));
            None
        }
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
            ("entangled", "") => config
                .warnings
                .push(format!("config line {n}: entangled is empty")),
            ("entangled", command) => config.entangled = Some(command.to_string()),
            ("split-ratio", v) => match v.parse::<u16>() {
                Ok(pct) => config.split_ratio = Some(pct.clamp(15, 85)),
                Err(_) => config
                    .warnings
                    .push(format!("config line {n}: split-ratio is not a number: {v}")),
            },
            ("appearance", v) => match v {
                "system" => config.appearance = Appearance::System,
                "dark" => config.appearance = Appearance::Dark,
                "light" => config.appearance = Appearance::Light,
                _ => config.warnings.push(format!(
                    "config line {n}: appearance is not dark, light or system: {v}"
                )),
            },
            ("theme-light", "") => config
                .warnings
                .push(format!("config line {n}: theme-light is empty")),
            ("theme-light", name) => config.theme_light = Some(name.to_string()),
            ("qt-mac-dont-swap-ctrl-and-meta", v) => {
                if let Some(b) = flag("qt-mac-dont-swap-ctrl-and-meta", v, n, &mut config.warnings)
                {
                    config.mac_dont_swap_ctrl_and_meta = b;
                }
            }
            (key @ ("lsp" | "mcp" | "mcp-edit" | "mcp-save"), v) => {
                if let Some(b) = flag(key, v, n, &mut config.warnings) {
                    match key {
                        "lsp" => config.lsp = b,
                        "mcp" => config.mcp.enabled = b,
                        "mcp-edit" => config.mcp.edit = b,
                        _ => config.mcp.save = b,
                    }
                }
            }
            (key @ ("number" | "wrap" | "syntax"), v) => {
                let b = flag(key, v, n, &mut config.warnings);
                match key {
                    "number" => config.number = b,
                    "wrap" => config.wrap = b,
                    _ => config.syntax = b,
                }
            }
            ("mcp-port", v) => match v.parse::<u16>() {
                Ok(port) if port > 0 => config.mcp.port = port,
                _ => config
                    .warnings
                    .push(format!("config line {n}: mcp-port is not a port: {v}")),
            },
            ("mcp-token", "") => {}
            ("mcp-token", token) => config.mcp.token = Some(token.to_string()),
            (key, command) if key.starts_with("lsp-") && key.len() > 4 => match command {
                "" => config
                    .warnings
                    .push(format!("config line {n}: {key} is empty")),
                command => config.servers.push(leolsp::ServerConfig {
                    language: key[4..].to_string(),
                    command: command.to_string(),
                }),
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
    save_theme_as(path, "theme", name)
}

/// Record `name` under `key`, `theme` or leogui's `theme-light`.
pub fn save_theme_as(path: &Path, key: &str, name: &str) -> io::Result<()> {
    if name.is_empty() || name.contains(['"', '\\', '\n', '\r']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a theme name: {name:?}"),
        ));
    }
    save(path, key, &format!("\"{name}\""))
}

/// Record leogui's light or dark choice.
pub fn save_appearance(path: &Path, appearance: Appearance) -> io::Result<()> {
    save(path, "appearance", &format!("\"{}\"", appearance.name()))
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
    update(path, &[(key.to_string(), Some(value.to_string()))])
}

/// Set or remove several settings in one write, each value already in TOML
/// form (`"name"`, `true`, `42`). None removes the key. Everything else in
/// the file survives, as `save` keeps it.
pub fn update(path: &Path, changes: &[(String, Option<String>)]) -> io::Result<()> {
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
    let text = changes.iter().fold(text, |text, (key, value)| match value {
        Some(value) => with_setting(&text, key, value),
        None => without_setting(&text, key),
    });
    std::fs::write(&tmp, text)?;
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

/// `text` without any top-level line setting `key`.
fn without_setting(text: &str, key: &str) -> String {
    let top = text
        .lines()
        .position(|l| strip_comment(l).trim_start().starts_with('['))
        .unwrap_or(usize::MAX);
    let out: String = text
        .lines()
        .enumerate()
        .filter(|(i, l)| {
            *i >= top
                || !strip_comment(l)
                    .split_once('=')
                    .is_some_and(|(k, _)| unquote(k.trim()) == key)
        })
        .map(|(_, l)| format!("{l}\n"))
        .collect();
    out
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
    fn a_language_server_is_named_per_language() {
        let c = parse("lsp-python = \"pylsp --check\"\nlsp-c = \"\"\nlsp- = \"x\"\n");
        let servers: Vec<(&str, &str)> = c
            .servers
            .iter()
            .map(|s| (s.language.as_str(), s.command.as_str()))
            .collect();
        assert_eq!(servers, [("python", "pylsp --check")]);
        assert_eq!(
            c.warnings,
            [
                "config line 2: lsp-c is empty",
                "config line 3: unknown setting lsp-"
            ]
        );
    }

    #[test]
    fn the_appearance_and_light_theme_are_read() {
        let c = parse("appearance = \"light\"\ntheme-light = \"onelight\"\n");
        assert_eq!(c.appearance, Appearance::Light);
        assert_eq!(c.theme_light.as_deref(), Some("onelight"));
        let c = parse("appearance = dim\n");
        assert_eq!((c.appearance, c.warnings.len()), (Appearance::System, 1));
    }

    #[test]
    fn the_mac_key_swap_is_read_as_leo_names_it() {
        let c = parse("qt-mac-dont-swap-ctrl-and-meta = true\n");
        assert!(c.mac_dont_swap_ctrl_and_meta);
        let c = parse("qt-mac-dont-swap-ctrl-and-meta = yes\n");
        assert!(!c.mac_dont_swap_ctrl_and_meta);
        assert_eq!(c.warnings.len(), 1);
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
    fn the_old_file_name_is_moved_to_the_new_once() {
        let s = Scratch::new("migrate");
        std::fs::create_dir_all(&s.0).unwrap();
        let (old, new) = (s.0.join("config.toml"), s.0.join("settings.toml"));
        std::fs::write(&old, "appearance = \"dark\"  # mine\n").unwrap();
        migrate(&new);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(&new).unwrap(),
            "appearance = \"dark\"  # mine\n"
        );
        // With both there, neither is touched.
        std::fs::write(&old, "theme = \"x\"\n").unwrap();
        migrate(&new);
        assert!(old.exists());
    }

    #[test]
    fn update_sets_and_removes_keys_and_keeps_the_rest() {
        let s = Scratch::new("update");
        let path = s.0.join("settings.toml");
        std::fs::create_dir_all(&s.0).unwrap();
        std::fs::write(&path, "# my settings\nlsp-c = \"clangd\"\nfuture-key = 1\n").unwrap();
        update(
            &path,
            &[
                ("lsp-c".to_string(), None),
                ("lsp".to_string(), Some("false".to_string())),
                ("mcp-port".to_string(), Some("7400".to_string())),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "# my settings\nfuture-key = 1\nlsp = false\nmcp-port = 7400\n"
        );
        let c = parse(&text);
        assert!(!c.lsp && c.servers.is_empty());
        assert_eq!(c.mcp.port, 7400);
    }

    #[test]
    fn the_feature_switches_are_read_and_mcp_is_off_by_default() {
        let c = parse("");
        assert!(c.lsp && !c.mcp.enabled && !c.mcp.edit && !c.mcp.save);
        let c = parse(concat!(
            "lsp = false\nnumber = true\nwrap = false\nsyntax = false\n",
            "mcp = true\nmcp-edit = true\nmcp-token = \"abc\"\nmcp-port = 0\n",
        ));
        assert!(!c.lsp);
        assert_eq!(
            (c.number, c.wrap, c.syntax),
            (Some(true), Some(false), Some(false))
        );
        assert!(c.mcp.enabled && c.mcp.edit && !c.mcp.save);
        assert_eq!(c.mcp.token.as_deref(), Some("abc"));
        assert_eq!((c.mcp.port, c.warnings.len()), (7341, 1));
    }

    #[test]
    fn saving_creates_the_directory_and_the_file() {
        let s = Scratch::new("create");
        let path = s.0.join("leo-rs/settings.toml");
        save_theme(&path, "nord").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theme = \"nord\"\n"
        );
    }

    #[test]
    fn the_old_directory_is_renamed_with_everything_in_it() {
        let s = Scratch::new("migrate-dir");
        std::fs::create_dir_all(s.0.join("leotui/themes")).unwrap();
        std::fs::write(s.0.join("leotui/settings.toml"), "theme = \"nord\"\n").unwrap();
        std::fs::write(s.0.join("leotui/session"), "x").unwrap();
        assert_eq!(
            dir_in(&s.0),
            s.0.join("leotui"),
            "read in place until moved"
        );
        migrate_dir(&s.0);
        let dir = dir_in(&s.0);
        assert_eq!(dir, s.0.join("leo-rs"));
        assert!(!s.0.join("leotui").exists());
        assert!(dir.join("themes").is_dir());
        assert_eq!(std::fs::read_to_string(dir.join("session")).unwrap(), "x");
        assert_eq!(
            std::fs::read_to_string(dir.join("settings.toml")).unwrap(),
            "theme = \"nord\"\n"
        );
    }

    #[test]
    fn the_new_directory_wins_and_the_old_is_left_alone() {
        let s = Scratch::new("both");
        std::fs::create_dir_all(s.0.join("leotui")).unwrap();
        std::fs::create_dir_all(s.0.join("leo-rs")).unwrap();
        migrate_dir(&s.0);
        assert_eq!(dir_in(&s.0), s.0.join("leo-rs"));
        assert!(s.0.join("leotui").exists());
    }

    #[test]
    fn no_directory_is_made_when_there_is_nothing_to_move() {
        let s = Scratch::new("none");
        std::fs::create_dir_all(&s.0).unwrap();
        migrate_dir(&s.0);
        assert_eq!(dir_in(&s.0), s.0.join("leo-rs"));
        assert!(!s.0.join("leo-rs").exists());
    }

    #[test]
    fn a_name_that_would_break_the_file_is_refused() {
        let s = Scratch::new("refuse");
        let path = s.0.join("settings.toml");
        for name in ["", "a\"b", "a\nb", "a\\b"] {
            assert!(save_theme(&path, name).is_err(), "{name:?} was written");
        }
        assert!(!path.exists());
    }

    #[test]
    fn a_file_that_cannot_be_read_is_left_alone() {
        let s = Scratch::new("unreadable");
        std::fs::create_dir_all(&s.0).unwrap();
        let path = s.0.join("settings.toml");
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
        let link = s.0.join("settings.toml");
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
