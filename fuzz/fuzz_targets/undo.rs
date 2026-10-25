//! Random outline edits from any bytes, each undone and redone.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| leolib::fuzz::undo(bytes));
