//! A heading of an `@qmd` or `@rmd` file, renamed to any text: the file
//! written reads back to the renamed tree.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (bool, u8, &str, &str)| {
    let (rmd, which, headline, text) = input;
    leo_markdown::fuzz::rename(text, rmd, which.into(), headline);
});
