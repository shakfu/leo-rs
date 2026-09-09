//! Import one file as @auto and show where the round trip diverges.
//!
//!     cargo run --example autodiff -- FILE [--tree]

fn main() {
    let mut args = std::env::args().skip(1);
    let path = leolib::util::finalize(&args.next().expect("usage: autodiff FILE"));
    let show_tree = args.any(|a| a == "--tree");
    let contents = std::fs::read_to_string(&path).unwrap();

    let mut o = leolib::Outline::new_empty();
    o.file_name = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
    let root = o.root_position().unwrap();
    o.set_headline(&root, &format!("@auto {path}"));

    let report = leolib::importers::import_string(&mut o, &root, &contents, &path).unwrap();
    if show_tree {
        for p in root.self_and_subtree(&o) {
            println!("{}{}", "  ".repeat(p.level()), p.h(&o));
        }
        println!("---");
    }
    let written = match leolib::importers::write_string(&o, &root, &path) {
        Ok(s) => s,
        Err(e) => {
            println!("write failed: {e}");
            return;
        }
    };
    let want = leolib::util::split_lines(&report.text);
    let got = leolib::util::split_lines(&written);
    println!(
        "nodes: {} want {} lines, got {}",
        report.nodes,
        want.len(),
        got.len()
    );
    for i in 0..want.len().max(got.len()) {
        let a = want.get(i).map(|s| s.as_str()).unwrap_or("<missing>");
        let b = got.get(i).map(|s| s.as_str()).unwrap_or("<missing>");
        if a != b {
            println!("first difference at line {}:", i + 1);
            for j in i.saturating_sub(3)..(i + 4) {
                let a = want.get(j).map(|s| s.as_str()).unwrap_or("<missing>");
                let b = got.get(j).map(|s| s.as_str()).unwrap_or("<missing>");
                let mark = if a == b { " " } else { "*" };
                println!("{mark} {:4} want {a:?}", j + 1);
                println!("{mark}      got  {b:?}");
            }
            return;
        }
    }
    println!("identical");
}
