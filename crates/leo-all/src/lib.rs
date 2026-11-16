//! leotui and leogui with every plugin: `leo-plugins`'s, with its `agent`
//! feature, and `@entangled`, which `leo-plugins` cannot name until it is
//! published. Not published itself.

/// Register every plugin. False if leoapp had already chosen its plugins.
pub fn register() -> bool {
    let kinds = leo_plugins::kinds()
        .with(leo_entangled::Entangled)
        .expect("not Leo's");
    let mut plugins = leo_plugins::app_plugins();
    plugins.push(&leo_entangled::app::Entangled);
    leo_plugins::register_with(kinds, plugins)
}
