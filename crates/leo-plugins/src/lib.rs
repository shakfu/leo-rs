//! The plugins a leo-rs front end offers, registered once at startup.
//!
//! leolib reads only Leo's kinds, and leoapp has no plugin of its own: a
//! plugin crate that implements `leoapp::plugins::AppPlugin` depends on
//! leoapp, so leoapp cannot depend on it. This crate depends on both and
//! is what leotui and leogui call. Cargo features choose the plugins. The
//! design is in `docs/dev/plugins.md`.

use leoapp::plugins::AppPlugin;
use leolib::ext::Kinds;

/// The leolib kinds this build offers.
pub fn kinds() -> Kinds {
    #[allow(unused_mut)]
    let mut kinds = Kinds::empty();
    #[cfg(feature = "markdown")]
    {
        kinds = kinds.with(leo_markdown::QMD).expect("not Leo's");
        kinds = kinds.with(leo_markdown::RMD).expect("not Leo's");
    }
    #[cfg(feature = "wiki")]
    {
        kinds = kinds.with_tree(leo_wiki::Wiki).expect("not Leo's");
    }
    kinds
}

/// The leoapp plugins this build offers.
pub fn app_plugins() -> Vec<&'static dyn AppPlugin> {
    let plugins: &[&'static dyn AppPlugin] = &[
        #[cfg(feature = "wiki")]
        &leo_wiki::app::WikiPlugin,
    ];
    plugins.to_vec()
}

/// Register this build's kinds and app plugins with leoapp, before any
/// outline is opened. False if leoapp had already chosen its plugins.
pub fn register() -> bool {
    let kinds = leoapp::plugins::register_kinds(kinds());
    let plugins = leoapp::plugins::register(app_plugins());
    kinds && plugins
}
