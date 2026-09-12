//! Open an outline, write its external files, and report what changed.
//!
//!     cargo run --example writecheck -- FILE.leo [--edit HEADLINE]

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: writecheck FILE.leo [--edit H]");
    let mut edit: Option<String> = None;
    while let Some(a) = args.next() {
        if a == "--edit" {
            edit = args.next();
        }
    }
    let mut doc = leolib::Document::open(&path, true).expect("open failed");
    if let Some(headline) = &edit {
        let target = doc
            .outline
            .all_positions()
            .into_iter()
            .find(|p| p.h(&doc.outline) == headline)
            .unwrap_or_else(|| panic!("no node named {headline:?}"));
        let body = format!("{}# edited by writecheck\n", target.b(&doc.outline));
        doc.set_body(&target, &body);
    }
    let result = doc.write_external_files(true);
    println!("written: {}", result.written.len());
    for path in &result.written {
        println!("  {path}");
    }
    println!("unchanged: {}", result.unchanged);
    for e in &result.errors {
        println!("  ERROR {}: {}", e.headline, e.error);
    }
}
