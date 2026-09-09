//! Digest the @auto tree of every file named on stdin, one per line.
//!
//! Batched so a whole corpus can be compared against Leo's importers without
//! starting a process per file.

use std::io::Read;

/// FNV-1a, so the Python side can compute the same digest.
fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    for path in input.split_whitespace() {
        let path = leolib::util::finalize(path);
        let Ok(contents) = leolib::external::read_file_to_string(&path) else {
            println!("{path}\tERROR unreadable");
            continue;
        };
        let mut o = leolib::Outline::new_empty();
        o.file_name = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("@auto {path}"));
        match leolib::importers::import_string(&mut o, &root, &contents, &path) {
            Err(e) => println!("{path}\tERROR {e}"),
            Ok(_) => {
                let mut text = String::new();
                for p in root.self_and_subtree(&o) {
                    text.push_str(&format!(
                        "{}|{}|{}\n{}\u{1}\n",
                        p.level(),
                        p.h(&o),
                        p.b(&o).chars().count(),
                        p.b(&o)
                    ));
                }
                println!("{path}\t{:016x}", fnv1a(&text));
            }
        }
    }
}
