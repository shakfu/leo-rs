//! The `@qmd` and `@rmd` reader, on any text: a kept read writes it back.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    leo_markdown::fuzz::read(text, false);
    leo_markdown::fuzz::read(text, true);
});
