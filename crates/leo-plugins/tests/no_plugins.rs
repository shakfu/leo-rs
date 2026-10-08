//! A build with no plugin features registers nothing, and leoapp reads
//! outlines as Leo does.
#![cfg(not(feature = "markdown"))]

#[test]
fn register_with_no_features_gives_leoapp_no_kinds() {
    assert!(leo_plugins::register());
    assert!(leoapp::plugins::kinds().directives().is_empty());
    assert!(leo_plugins::app_plugins().is_empty());
}
