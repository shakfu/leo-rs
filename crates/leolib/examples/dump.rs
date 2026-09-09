//! Read a .leo file and print a digest of it. For checking the port against Leo.
//!
//!     cargo run --example dump -- FILE.leo [--hash] [--external]

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: dump FILE.leo [--hash] [--external]");
        std::process::exit(2);
    };
    let read_external = args.iter().any(|a| a == "--external");
    let o = match leolib::open_outline(path, read_external) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let all = o.all_unique_positions();
    if !args.iter().any(|a| a == "--hash") {
        println!("nodes: {}", all.len());
    } else {
        eprintln!("nodes: {}", all.len());
    }
    if args.iter().any(|a| a == "--hash") {
        // The same byte stream the Python side digests, so `shasum` compares
        // the two implementations directly.
        let mut out = std::io::BufWriter::new(std::io::stdout().lock());
        for p in &all {
            write!(out, "{}\0{}\0{}\0", p.gnx(&o), p.h(&o), p.b(&o)).unwrap();
        }
    }
}
