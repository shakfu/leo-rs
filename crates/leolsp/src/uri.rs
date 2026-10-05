//! `file:` URIs, which name every document a server sees.

use std::path::{Path, PathBuf};

/// Bytes a URI path keeps as they are: RFC 3986's unreserved characters and
/// the separators a path needs.
fn keep(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~/".contains(&b)
}

/// The `file:` URI of an absolute path, every other byte percent-encoded.
pub fn from_path(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    // A Windows path, `C:/x`, becomes `file:///C:/x`.
    if !path.starts_with('/') {
        out.push('/');
    }
    for (i, b) in path.bytes().enumerate() {
        // The drive's colon stays, as servers write it.
        let drive = i == 1 && b == b':' && path.as_bytes()[0].is_ascii_alphabetic();
        match keep(b) || drive {
            true => out.push(b as char),
            false => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The path a `file:` URI names, or None for another scheme.
pub fn to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://host/x` names a host, which a local editor has no use for.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                bytes.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                bytes.push(c);
                i += 1;
            }
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    // `/C:/x` is the Windows path `C:/x`.
    let path = match path.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => path[1..].to_string(),
        _ => path,
    };
    Some(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_round_trips_through_its_uri() {
        let path = Path::new("/home/a b/caf\u{e9}#1.py");
        let uri = from_path(path);
        assert_eq!(uri, "file:///home/a%20b/caf%C3%A9%231.py");
        assert_eq!(to_path(&uri).as_deref(), Some(path));
    }

    #[test]
    fn a_windows_drive_keeps_its_colon() {
        assert_eq!(from_path(Path::new("C:\\x\\y.rs")), "file:///C:/x/y.rs");
        assert_eq!(
            to_path("file:///C:/x/y.rs"),
            Some(PathBuf::from("C:/x/y.rs"))
        );
        assert_eq!(to_path("file:///c%3A/x"), Some(PathBuf::from("c:/x")));
    }

    #[test]
    fn another_scheme_names_no_path() {
        assert_eq!(to_path("untitled:leo/x"), None);
    }
}
