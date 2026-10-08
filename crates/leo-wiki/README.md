# leo-wiki

The `@wiki` tree kind for [leo-rs](https://github.com/shakfu/leo-rs): markdown pages under an `@wiki NAME` node, linked by `[[Page]]`, `[[Parent/Page]]` and `[[other:Page]]`, checked against the wiki's rules, renamed with their links, and exported to one markdown file with GitHub anchors.

With the `leoapp` feature it is also a leoapp plugin: links followed, `[[` completed, the rules kept, and `:export-wiki`. leotui and leogui register it through `leo-plugins`. What it does: [docs/plugins.md](https://github.com/shakfu/leo-rs/blob/main/docs/plugins.md); the design: [docs/dev/wiki.md](https://github.com/shakfu/leo-rs/blob/main/docs/dev/wiki.md).

License: MIT.
