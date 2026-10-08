//! The `.leo` reader, on any text: what reads must save and read again.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| leolib::fuzz::read_leo(text));
