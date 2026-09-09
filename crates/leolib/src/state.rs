//! Which nodes are unfolded and which are marked, remembered between sessions.
//!
//! Neither fact is stored in the `.leo` file. Leo keeps them in a sqlite cache
//! under `~/.leo/db`; this keeps them in a text file under `~/.leo/leo-rs/`,
//! deliberately separate, so nothing here can corrupt Leo's own cache. The
//! format is one record per line because the whole schema is three fields.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::node::status;
use crate::outline::Outline;
use crate::util;

/// Where this outline's state lives.
///
/// Named after the outline and keyed by its full path, so two files with the
/// same name in different directories do not share a record.
fn state_path(file_name: &str) -> Option<PathBuf> {
    if file_name.is_empty() {
        return None;
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in file_name.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name = util::os_path_basename(file_name);
    let dir = PathBuf::from(util::home_dir()).join(".leo").join("leo-rs");
    Some(dir.join(format!("{name}-{hash:016x}.state")))
}

/// Restore the fold and mark state recorded for this outline.
pub fn load(o: &mut Outline) {
    let Some(path) = state_path(&o.file_name) else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let known: HashSet<String> = o
        .all_unique_nodes()
        .into_iter()
        .map(|v| o.gnx(v).to_string())
        .collect();
    for line in text.lines() {
        let Some((kind, value)) = line.split_once(' ') else {
            continue;
        };
        // A gnx that is no longer in the outline is dropped rather than kept:
        // stale entries would otherwise accumulate for the life of the file.
        if !known.contains(value) {
            continue;
        }
        match kind {
            "expanded" => {
                o.expanded.insert(value.to_string());
            }
            "marked" => {
                if let Some(v) = o.find_gnx(value) {
                    o.node_mut(v).set_bit(status::MARKED);
                }
            }
            _ => {}
        }
    }
}

/// Record this outline's fold and mark state. Failure is not an error: the
/// state is a convenience, and a read-only home directory must not stop a save.
pub fn save(o: &Outline) {
    let Some(path) = state_path(&o.file_name) else {
        return;
    };
    let mut out = String::new();
    let mut expanded: Vec<&String> = o.expanded.iter().collect();
    expanded.sort();
    for gnx in expanded {
        out.push_str(&format!("expanded {gnx}\n"));
    }
    let mut marked: Vec<String> = o
        .all_unique_nodes()
        .into_iter()
        .filter(|v| o.node(*v).is_marked())
        .map(|v| o.gnx(v).to_string())
        .collect();
    marked.sort();
    for gnx in marked {
        out.push_str(&format!("marked {gnx}\n"));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outline_with_no_file_name_has_no_state_file() {
        assert!(state_path("").is_none());
    }

    #[test]
    fn the_state_path_distinguishes_directories() {
        let a = state_path("/one/x.leo").unwrap();
        let b = state_path("/two/x.leo").unwrap();
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("x.leo-"));
    }
}
