# leo-plugins

The plugins a [leo-rs](https://github.com/shakfu/leo-rs) front end offers, chosen by cargo feature: `markdown` gives `@qmd` and `@rmd` from `leo-markdown`, and `wiki` gives `@wiki` from `leo-wiki`; both are on by default. A front end calls `leo_plugins::register()` once at startup, before opening an outline. Design: [docs/dev/plugins.md](https://github.com/shakfu/leo-rs/blob/main/docs/dev/plugins.md).

License: MIT.
