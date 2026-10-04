//! The `@file` sentinel reader, on any text.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| leolib::fuzz::read_into_root(text));
