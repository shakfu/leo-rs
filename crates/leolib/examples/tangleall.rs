//! Tangle every external file of a .leo outline and compare with the disk.
//!
//!     cargo run --example tangleall -- FILE.leo

fn main() {
    let path = std::env::args().nth(1).expect("usage: tangleall FILE.leo");
    let o = leolib::open_outline(&path, true).expect("open failed");
    let (files, ignored) = leolib::external::find_files_to_write(&o, false);
    let (mut same, mut differ, mut missing, mut failed) = (0, 0, 0, 0);
    for p in &files {
        let disk_path = o.full_path(p);
        let Ok(disk) = std::fs::read_to_string(&disk_path) else {
            missing += 1;
            continue;
        };
        match leolib::external::file_contents(&o, p) {
            Ok((text, _newline, _encoding)) => {
                if text == disk {
                    same += 1;
                } else {
                    differ += 1;
                    if differ <= 3 {
                        eprintln!("DIFFERS: {}", p.h(&o));
                        for (i, (a, b)) in text.lines().zip(disk.lines()).enumerate() {
                            if a != b {
                                eprintln!("  line {}: got {a:?}\n           want {b:?}", i + 1);
                                break;
                            }
                        }
                    }
                }
            }
            Err(e) => {
                failed += 1;
                if failed <= 5 {
                    eprintln!("FAILED: {} -- {e}", p.h(&o));
                }
            }
        }
    }
    println!(
        "files: {} identical: {same} differ: {differ} missing: {missing} failed: {failed} ignored: {}",
        files.len(),
        ignored.len()
    );
}
