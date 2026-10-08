//! Leo's `@settings` trees, for the settings that change what this crate
//! reads and writes. Others are ignored.
//!
//! Leo reads `leoSettings.leo`, then `myLeoSettings.leo`, then the outline's
//! own `@settings` tree, each overriding the last. [`Config::default`] stands
//! for the first. The second is read only once a front end names it with
//! [`use_user_settings`], so leolib alone reads no file it was not given.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::external::FileNote;
use crate::outline::{Config, Outline};
use crate::position::Position;
use crate::util;

/// A setting as an `@settings` tree spells it.
#[derive(Debug, Clone)]
struct Setting {
    /// `@int` gives `int`.
    kind: String,
    /// Canonical: lower case, no `-`, `_` or spaces.
    name: String,
    /// None for `None`, or no value.
    value: Option<String>,
    /// The node's headline and the file it is in, for a warning.
    headline: String,
    path: String,
}

/// The user's settings, read once.
static USER: OnceLock<Vec<Setting>> = OnceLock::new();

/// Leo's `myLeoSettings.leo`: `~/.leo/myLeoSettings.leo`.
pub fn user_settings_path() -> PathBuf {
    Path::new(&util::home_dir())
        .join(".leo")
        .join("myLeoSettings.leo")
}

/// Read the `@settings` tree of the `.leo` file at `path` into every
/// outline opened from now on, beneath the outline's own. A missing file
/// is no settings. False if user settings were already chosen.
pub fn use_user_settings(path: &Path) -> bool {
    let path = path.to_string_lossy().to_string();
    let settings = match crate::leofile::read_leo_file(&path) {
        Ok(o) => collect(&o, &path),
        Err(_) => Vec::new(),
    };
    USER.set(settings).is_ok()
}

/// Apply the user's settings, then the outline's own, to `o.config`.
/// Returns a note for each setting whose value is not valid.
pub(crate) fn apply(o: &mut Outline) -> Vec<FileNote> {
    let own = collect(o, &o.file_name.clone());
    let mut notes = Vec::new();
    for s in USER.get().into_iter().flatten().chain(&own) {
        if let Err(message) = set(&mut o.config, s) {
            notes.push(FileNote {
                headline: s.headline.clone(),
                path: s.path.clone(),
                message,
            });
        }
    }
    notes
}

/// The settings in o's `@settings` tree, in outline order.
fn collect(o: &Outline, path: &str) -> Vec<Setting> {
    // Leo's `settingsRoot`: the first `@settings` node, comments allowed after.
    let Some(root) = o
        .all_unique_positions()
        .into_iter()
        .find(|p| util::match_word(p.h(o).trim_end(), 0, "@settings"))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let after = root.node_after_tree(o);
    let mut p: Option<Position> = root.thread_next(o);
    while let Some(cur) = p {
        if Some(&cur) == after.as_ref() {
            break;
        }
        let headline = cur.h(o).to_string();
        let (kind, name, value) = parse_headline(&headline);
        let skip = match kind.as_str() {
            "ignore" => true,
            "ifplatform" => !name
                .split(',')
                .any(|n| n.trim().eq_ignore_ascii_case(platform())),
            "ifenv" => !env_matches(&name),
            "ifhostname" => !host_matches(&name, &machine_name()),
            // `@if EXPRESSION` is Python, which this port cannot run.
            k if k.starts_with("if") => true,
            _ => false,
        };
        if skip {
            p = cur.node_after_tree(o);
            continue;
        }
        if !kind.is_empty() && !name.is_empty() {
            out.push(Setting {
                kind,
                name: canonical(&name),
                value: value.filter(|v| !matches!(v.as_str(), "None" | "none" | "")),
                headline,
                path: path.to_string(),
            });
        }
        p = cur.thread_next(o);
    }
    out
}

/// Leo's `parseHeadline`: `@kind name = value`.
fn parse_headline(h: &str) -> (String, String, Option<String>) {
    let Some(rest) = h.strip_prefix('@') else {
        return (String::new(), String::new(), None);
    };
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(rest.len());
    let kind = canonical(&rest[..end]);
    let rest = &rest[end..];
    match rest.split_once('=') {
        Some((name, value)) => (
            kind,
            name.trim().to_string(),
            Some(value.trim().to_string()),
        ),
        None => (kind, rest.trim().to_string(), None),
    }
}

/// Leo's `canonicalizeSettingName`.
fn canonical(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' ' | '\n'))
        .collect()
}

/// Leo's `@ifenv NAME, VALUE, ...`: the variable's value, lower case, is one
/// of the values; an unset one is `none`.
fn env_matches(spec: &str) -> bool {
    let mut parts = spec.split(',');
    let Some(var) = parts.next().map(str::trim) else {
        return false;
    };
    let value = std::env::var(var)
        .map(|v| v.trim().to_lowercase())
        .unwrap_or_else(|_| "none".to_string());
    parts.any(|p| p.trim().to_lowercase() == value)
}

/// Leo's `@ifhostname NAME` or `@ifhostname !NAME`.
fn host_matches(spec: &str, host: &str) -> bool {
    match spec.trim().strip_prefix('!') {
        Some(not) => host != not,
        None => host == spec.trim(),
    }
}

/// Leo's `computeMachineName`: `$HOSTNAME`, `$COMPUTERNAME`, else the
/// system's host name.
fn machine_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            let out = std::process::Command::new("hostname").output().ok()?;
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        })
        .unwrap_or_default()
}

/// Python's `sys.platform` for this machine, which `@ifplatform` names.
fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        os => os,
    }
}

/// Set the field `s` names. An unknown name is not this crate's.
fn set(config: &mut Config, s: &Setting) -> Result<(), String> {
    let d = Config::default();
    let v = s.value.as_deref();
    let int = |default: i32| match v {
        None => Ok(default),
        Some(v) => v
            .parse::<i32>()
            .map_err(|_| format!("{v} is not a valid int for {}", s.name)),
    };
    // Leo's getBool gives None, which its callers read as false.
    let bool = || match v {
        None => Ok(false),
        Some("True" | "true" | "1") => Ok(true),
        Some("False" | "false" | "0") => Ok(false),
        Some(v) => Err(format!("{v} is not a valid bool for {}", s.name)),
    };
    let string = |default: String| v.map_or(default, str::to_string);
    let wanted = match s.name.as_str() {
        "tabwidth" | "pagewidth" => "int",
        "outputnewline" | "targetlanguage" | "defaultderivedfileencoding" => "string",
        "createnonexistentdirectories" | "forcenewlinesinatnosentbodies" => "bool",
        _ => return Ok(()),
    };
    if s.kind != wanted {
        return Err(format!(
            "{} must be an @{wanted}, not an @{}",
            s.name, s.kind
        ));
    }
    match s.name.as_str() {
        "tabwidth" => config.tab_width = int(d.tab_width)?,
        "pagewidth" => config.page_width = int(d.page_width)?,
        "outputnewline" => config.output_newline = string(d.output_newline),
        "targetlanguage" => config.target_language = string(d.target_language),
        "defaultderivedfileencoding" => {
            config.default_derived_file_encoding = string(d.default_derived_file_encoding)
        }
        "createnonexistentdirectories" => config.create_nonexistent_directories = bool()?,
        "forcenewlinesinatnosentbodies" => config.force_newlines_in_at_nosent_bodies = bool()?,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_and_host_conditions_are_tested_as_leo_tests_them() {
        // PATH is set wherever tests run; this name is not.
        let path = std::env::var("PATH").unwrap().to_lowercase();
        assert!(env_matches(&format!("PATH, {path}")));
        assert!(env_matches("LEO_RS_UNSET_VARIABLE, None"));
        assert!(!env_matches("LEO_RS_UNSET_VARIABLE, yes"));
        assert!(host_matches("bob", "bob") && !host_matches("bob", "harry"));
        assert!(host_matches("!harry", "bob") && !host_matches("!bob", "bob"));
    }

    #[test]
    fn a_settings_headline_is_parsed_as_leo_parses_it() {
        let (k, n, v) = parse_headline("@int tab-width = 8");
        assert_eq!(
            (k.as_str(), n.as_str(), v.as_deref()),
            ("int", "tab-width", Some("8"))
        );
        let (k, n, v) = parse_headline("@bool Create_Nonexistent-Directories=True");
        assert_eq!(
            (k.as_str(), canonical(&n).as_str(), v.as_deref()),
            ("bool", "createnonexistentdirectories", Some("True"))
        );
        assert_eq!(parse_headline("plain node").0, "");
    }

    #[test]
    fn an_outlines_settings_set_its_config_and_bad_values_are_reported() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@settings # mine");
        let organizer = o.insert_as_last_child(&root);
        o.set_headline(&organizer, "files");
        for h in [
            "@int tab-width = 8",
            "@string output-newline = crlf",
            "@bool create-nonexistent-directories = True",
            "@int page-width = wide",
            "@string force-newlines-in-at-nosent-bodies = no",
        ] {
            let p = o.insert_as_last_child(&organizer);
            o.set_headline(&p, h);
        }
        let ignored = o.insert_as_last_child(&root);
        o.set_headline(&ignored, "@ignore");
        let p = o.insert_as_last_child(&ignored);
        o.set_headline(&p, "@string target-language = rust");
        let notes = apply(&mut o);
        let c = &o.config;
        assert_eq!((c.tab_width, c.output_newline.as_str()), (8, "crlf"));
        assert!(c.create_nonexistent_directories);
        assert_eq!(c.page_width, Config::default().page_width);
        assert_eq!(c.target_language, Config::default().target_language);
        let messages: Vec<&str> = notes.iter().map(|n| n.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "wide is not a valid int for pagewidth",
                "forcenewlinesinatnosentbodies must be an @bool, not an @string"
            ]
        );
    }
}
