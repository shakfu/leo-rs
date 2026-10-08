//! The gnx allocator.
//!
//! A gnx is `<user id>.<local timestamp>.<n>`. The format is structural:
//! every .leo file and every external file on disk names nodes this way, so
//! `NodeIndices` reproduces Leo's `leoNodes.NodeIndices` exactly, including
//! the rule that `n` restarts at 1 whenever the second changes.

/// Seconds since the epoch, formatted as Leo's `%Y%m%d%H%M%S`.
///
/// UTC, where Leo uses local time. Nothing compares a gnx against a clock; the
/// timestamp only has to advance and to be the same for two gnxs minted in the
/// same second, and UTC keeps that true across a DST change, which local time
/// does not.
fn time_string() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = civil_from_unix(secs);
    format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}")
}

/// Days-to-calendar conversion (Howard Hinnant's civil_from_days).
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// The allocator [`new_gnx`] shares across the process.
static SHARED: once_cell::sync::Lazy<std::sync::Mutex<NodeIndices>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(NodeIndices::new(&default_user_id())));

/// Mint a gnx from the process's one allocator.
///
/// Process-wide, never per outline: nodes are copied between outlines, and
/// two allocators sharing a user id mint the same gnx within the same second.
pub fn new_gnx() -> String {
    // A panic elsewhere while holding the lock leaves the counter intact.
    let mut ni = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    ni.new_gnx()
}

/// The id a gnx starts with, in Leo's order: the first line of
/// `~/.leo/.leoID.txt`, then the login name.
fn default_user_id() -> String {
    let file = std::path::Path::new(&crate::util::home_dir())
        .join(".leo")
        .join(".leoID.txt");
    id_from_file(&file)
        .or_else(|| {
            let login = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .ok()?;
            clean(&login)
        })
        .unwrap_or_else(|| "leo-rs".to_string())
}

/// The id on the first line of a `.leoID.txt` file, if it holds a valid one.
fn id_from_file(path: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    clean(text.lines().next()?)
}

/// Leo's `cleanLeoID`: no `.`, `,`, quotes or whitespace, which would make
/// gnxs Leo never mints. None for an id under three characters, which Leo
/// refuses.
fn clean(id: &str) -> Option<String> {
    let id: String = id
        .chars()
        .filter(|c| !matches!(c, '.' | ',' | '"' | '\'') && !c.is_whitespace())
        .collect();
    (id.chars().count() >= 3).then_some(id)
}

/// [`clean`], with this port's fallback.
#[cfg(test)]
fn clean_user_id(id: &str) -> String {
    clean(id).unwrap_or_else(|| "leo-rs".to_string())
}

/// Allocates gnxs. [`new_gnx`] holds the one every outline uses.
#[derive(Debug, Clone)]
pub struct NodeIndices {
    /// The id each gnx starts with.
    pub user_id: String,
    /// The `n` of the last gnx minted in `time_string`'s second.
    pub last_index: u64,
    /// Timestamp of the last gnx minted, as `%Y%m%d%H%M%S`.
    pub time_string: String,
}

impl NodeIndices {
    /// An allocator for `user_id`, with the clock read now.
    pub fn new(user_id: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            last_index: 0,
            time_string: time_string(),
        }
    }

    /// Update the timestamp and counter, then return the timestamp.
    fn update(&mut self) -> String {
        let t = time_string();
        if self.time_string == t {
            self.last_index += 1;
        } else {
            self.last_index = 1;
            self.time_string = t.clone();
        }
        t
    }

    /// Mint the next gnx. Leo's `NodeIndices.computeNewIndex`.
    pub fn new_gnx(&mut self) -> String {
        let t = self.update();
        format!("{}.{}.{}", self.user_id, t, self.last_index)
    }

    /// Split a gnx into (id, timestamp, n). Missing parts come back empty.
    pub fn scan_gnx(&self, s: &str) -> (String, String, String) {
        let s = s.trim();
        let mut parts = s.splitn(3, '.');
        let id = parts.next().unwrap_or("").to_string();
        let t = parts.next().unwrap_or("").to_string();
        let n = parts.next().unwrap_or("").to_string();
        let id = if id.is_empty() {
            self.user_id.clone()
        } else {
            id
        };
        (id, t, n)
    }

    /// Raise `last_index` so a later allocation cannot collide with `gnx`.
    pub fn update_last_index(&mut self, gnx: &str) {
        let (id, t, n) = self.scan_gnx(gnx);
        if id.is_empty() || n.is_empty() {
            return;
        }
        if id == self.user_id && t == self.time_string {
            if let Ok(n2) = n.parse::<u64>() {
                if n2 > self.last_index {
                    self.last_index = n2;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_name_is_cleaned_as_leo_cleans_it() {
        assert_eq!(clean_user_id("Jane Q. Doe"), "JaneQDoe");
        assert_eq!(clean_user_id("o'neil,\"x\""), "oneilx");
        assert_eq!(clean_user_id("a b"), "leo-rs");
        assert_eq!(clean_user_id(""), "leo-rs");
    }

    #[test]
    fn a_leo_id_file_gives_its_first_line_cleaned() {
        let dir = std::env::temp_dir().join(format!("leolib-leoid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(".leoID.txt");
        std::fs::write(&file, "e.k.r\nsecond\n").unwrap();
        assert_eq!(id_from_file(&file).as_deref(), Some("ekr"));
        std::fs::write(&file, "ab\n").unwrap();
        assert_eq!(
            id_from_file(&file),
            None,
            "too short: the login name is next"
        );
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(id_from_file(&dir.join("none")), None);
    }

    #[test]
    fn gnxs_are_unique_and_well_formed() {
        let mut ni = NodeIndices::new("test");
        let a = ni.new_gnx();
        let b = ni.new_gnx();
        assert_ne!(a, b);
        let (id, t, n) = ni.scan_gnx(&a);
        assert_eq!(id, "test");
        assert_eq!(t.len(), 14);
        assert!(n.parse::<u64>().is_ok());
    }

    #[test]
    fn scan_gnx_fills_in_the_default_id() {
        let ni = NodeIndices::new("test");
        assert_eq!(ni.scan_gnx(".20200101000000.1").0, "test");
    }
}
