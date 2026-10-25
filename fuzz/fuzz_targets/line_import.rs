//! The `@auto` line importers, org, otl, markdown and treepad, on any text.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| leolib::fuzz::line_import(text));
