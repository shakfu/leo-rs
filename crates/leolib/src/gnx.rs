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

/// Allocates gnxs for one process.
///
/// Process-wide, never per outline: Leo copies and clones nodes between
/// outlines, so two allocators sharing a user id would mint the same gnx
/// within the same second.
#[derive(Debug, Clone)]
pub struct NodeIndices {
    pub user_id: String,
    pub last_index: u64,
    pub time_string: String,
}

impl NodeIndices {
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
