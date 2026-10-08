//! App plugins: the commands, settings, menu entries and background work a
//! leo-rs-only kind brings to the front ends. The kinds themselves are
//! `leolib::ext`'s; the design is in `docs/dev/plugins.md`.
//!
//! One list per process: the binary registers the plugins it offers, with
//! the `leo-plugins` crate, before first use. With none registered there
//! are none. The leolib kinds documents are opened with are registered the
//! same way ([`register_kinds`]).

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::app::App;
use crate::commands::Command;

/// A menu item a plugin adds: under `menu`, `label` runs `command`.
pub struct MenuEntry {
    pub menu: &'static str,
    pub label: &'static str,
    pub command: &'static str,
}

/// What a plugin adds to the app.
pub trait AppPlugin: Sync {
    /// Its name, as the cargo feature spells it.
    fn name(&self) -> &'static str;
    /// Its commands, as the core's table has them.
    fn commands(&self) -> &'static [Command] {
        &[]
    }
    /// What runs command `name` with an argument from the `:` line, if
    /// `name` takes one.
    fn with_argument(&self, _name: &str) -> Option<fn(&mut App, &str)> {
        None
    }
    /// The settings keys it reads, `entangled = PATH`.
    fn settings(&self) -> &'static [&'static str] {
        &[]
    }
    /// Its menu items.
    fn menu(&self) -> &'static [MenuEntry] {
        &[]
    }
    /// Collect finished background work. True if anything changed that a
    /// front end draws.
    fn poll(&self, _app: &mut App) -> bool {
        false
    }
    /// How soon `poll` has work, if it has any.
    fn poll_after(&self, _app: &App) -> Option<Duration> {
        None
    }
    /// Follow the link under the body cursor, if it is one of this plugin's.
    /// True if it was.
    fn open_url(&self, _app: &mut App) -> bool {
        false
    }
    /// What completes at the body cursor while typing, if this plugin offers
    /// anything there: the column the text starts at, and the candidates.
    fn complete(&self, _app: &App) -> Option<(usize, Vec<String>)> {
        None
    }
    /// The rules of this plugin's trees the outline breaks, one line each.
    /// The app undoes an edit that adds one.
    fn violations(&self, _o: &leolib::Outline) -> Vec<String> {
        Vec::new()
    }
}

static PLUGINS: OnceLock<Vec<&'static dyn AppPlugin>> = OnceLock::new();

static KINDS: OnceLock<Arc<leolib::ext::Kinds>> = OnceLock::new();

/// Offer `plugins`. False if the list was already in use.
pub fn register(plugins: Vec<&'static dyn AppPlugin>) -> bool {
    PLUGINS.set(plugins).is_ok()
}

/// The plugins this process offers.
pub fn all() -> &'static [&'static dyn AppPlugin] {
    PLUGINS.get_or_init(Vec::new)
}

/// Open and create documents with `kinds` beyond Leo's. False if the kinds
/// were already in use.
pub fn register_kinds(kinds: leolib::ext::Kinds) -> bool {
    KINDS.set(Arc::new(kinds)).is_ok()
}

/// The leolib kinds documents are opened and created with.
pub fn kinds() -> Arc<leolib::ext::Kinds> {
    KINDS
        .get_or_init(|| Arc::new(leolib::ext::Kinds::empty()))
        .clone()
}

/// Every plugin's commands.
pub fn commands() -> impl Iterator<Item = &'static Command> {
    all().iter().flat_map(|p| p.commands())
}

/// What runs plugin command `name` with an argument, if it takes one.
pub fn with_argument(name: &str) -> Option<fn(&mut App, &str)> {
    all().iter().find_map(|p| p.with_argument(name))
}

/// Whether a plugin reads the settings key `key`.
pub fn reads_setting(key: &str) -> bool {
    all().iter().any(|p| p.settings().contains(&key))
}

/// The items plugins add to the menu titled `menu`.
pub fn menu(menu: &str) -> impl Iterator<Item = &'static MenuEntry> + '_ {
    all()
        .iter()
        .flat_map(|p| p.menu())
        .filter(move |e| e.menu == menu)
}

/// Follow the link under the body cursor with the first plugin that knows it.
pub fn open_url(app: &mut App) -> bool {
    all().iter().any(|p| p.open_url(app))
}

/// What the first plugin with anything to offer completes at the cursor.
pub fn complete(app: &App) -> Option<(usize, Vec<String>)> {
    all().iter().find_map(|p| p.complete(app))
}

/// Every plugin's broken rules.
pub fn violations(o: &leolib::Outline) -> Vec<String> {
    all().iter().flat_map(|p| p.violations(o)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_none_registered_there_are_no_plugins_or_kinds() {
        assert!(crate::commands::find("entangled-tangle").is_none());
        assert!(!reads_setting("entangled"));
        assert!(all().is_empty());
        assert!(kinds().directives().is_empty());
    }
}
