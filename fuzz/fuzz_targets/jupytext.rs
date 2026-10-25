//! The `@jupytext` notebook and `py:percent` readers, on any text.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| leolib::fuzz::jupytext(text));
