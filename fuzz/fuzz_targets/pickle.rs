//! The uA pickle reader, on any bytes; what it reads must write back.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| leolib::fuzz::pickle(bytes));
