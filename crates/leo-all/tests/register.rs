//! Every plugin is registered: the default ones, the agents and `@entangled`.

#[test]
fn every_plugin_is_registered() {
    assert!(leo_all::register());
    let kinds = leoapp::plugins::kinds();
    for h in ["@qmd r.qmd", "@rmd r.Rmd", "@entangled README.md"] {
        assert!(kinds.find(h).is_some(), "{h}");
    }
    assert!(kinds.tree_directives().contains(&"@wiki"));
    let names: Vec<_> = leoapp::plugins::commands().map(|c| c.name).collect();
    for c in [
        "agent-run",
        "agent-cancel",
        "entangled-tangle",
        "export-wiki",
    ] {
        assert!(names.contains(&c), "{c} not in {names:?}");
    }
    assert!(leoapp::plugins::with_argument("ask").is_some());
}
