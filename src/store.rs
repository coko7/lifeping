//! Append-only ping log: one RFC 3339 UTC timestamp per line.

use std::io;
use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

pub const LOG_FILE: &str = "pings.log";

pub struct Store {
    path: PathBuf,
    inner: Mutex<Inner>,
}

struct Inner {
    /// Sorted ascending, oldest first.
    pings: Vec<OffsetDateTime>,
    /// The file was edited by hand and lacks a final newline; the next
    /// append must start with one instead of gluing onto the last line.
    needs_newline: bool,
}

/// A consistent view of the log, taken under the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub latest: Option<OffsetDateTime>,
    /// Newest first.
    pub recent: Vec<OffsetDateTime>,
    pub total: usize,
}

impl Store {
    /// Loads the log from `data_dir`, creating the directory if needed.
    /// A missing file is an empty history; malformed lines are skipped.
    pub fn open(data_dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join(LOG_FILE);
        let (pings, needs_newline) = match std::fs::read(&path) {
            Ok(bytes) => {
                let contents = String::from_utf8_lossy(&bytes);
                let needs_newline = !contents.is_empty() && !contents.ends_with('\n');
                (parse_log(&contents), needs_newline)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (Vec::new(), false),
            Err(e) => return Err(e),
        };
        tracing::info!(count = pings.len(), path = %path.display(), "loaded pings");
        Ok(Self {
            path,
            inner: Mutex::new(Inner {
                pings,
                needs_newline,
            }),
        })
    }

    /// Records a ping at the current server time. The in-memory history is
    /// only updated once the line is durably on disk.
    pub async fn append(&self) -> io::Result<OffsetDateTime> {
        let mut inner = self.inner.lock().await;
        let now = OffsetDateTime::now_utc()
            .replace_nanosecond(0)
            .expect("0 is a valid nanosecond");

        let mut line = if inner.needs_newline {
            String::from("\n")
        } else {
            String::new()
        };
        line.push_str(&format_timestamp(now));
        line.push('\n');

        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.sync_data().await?;

        inner.needs_newline = false;
        // Keep the vector sorted even if the clock went backwards.
        let idx = inner.pings.partition_point(|p| *p <= now);
        inner.pings.insert(idx, now);
        Ok(now)
    }

    pub async fn snapshot(&self, history: usize) -> Snapshot {
        let inner = self.inner.lock().await;
        Snapshot {
            latest: inner.pings.last().copied(),
            recent: inner.pings.iter().rev().take(history).copied().collect(),
            total: inner.pings.len(),
        }
    }
}

/// Formats a timestamp the way it is stored and served: `2026-09-26T08:12:44Z`.
pub fn format_timestamp(ts: OffsetDateTime) -> String {
    ts.to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .expect("RFC 3339 formatting of a UTC timestamp cannot fail")
}

fn parse_log(contents: &str) -> Vec<OffsetDateTime> {
    let mut pings: Vec<OffsetDateTime> = contents
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            match OffsetDateTime::parse(line, &Rfc3339) {
                Ok(ts) => Some(ts.to_offset(UtcOffset::UTC)),
                Err(e) => {
                    tracing::warn!(line = idx + 1, error = %e, "skipping malformed line in ping log");
                    None
                }
            }
        })
        .collect();
    pings.sort();
    pings
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn parse_empty() {
        assert!(parse_log("").is_empty());
        assert!(parse_log("\n\n  \n").is_empty());
    }

    #[test]
    fn parse_valid_lines() {
        let pings =
            parse_log("2026-09-26T08:12:44Z\n  2026-09-26T19:03:10Z \n\n2026-09-27T07:58:02Z");
        assert_eq!(
            pings,
            [
                datetime!(2026-09-26 08:12:44 UTC),
                datetime!(2026-09-26 19:03:10 UTC),
                datetime!(2026-09-27 07:58:02 UTC),
            ]
        );
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let pings = parse_log("garbage\n2026-09-26T08:12:44Z\n2026-13-45T99:00:00Z\nyesterday\n");
        assert_eq!(pings, [datetime!(2026-09-26 08:12:44 UTC)]);
    }

    #[test]
    fn parse_sorts_and_normalizes_to_utc() {
        let pings = parse_log("2026-09-27T07:58:02Z\n2026-09-26T10:12:44+02:00\n");
        assert_eq!(
            pings,
            [
                datetime!(2026-09-26 08:12:44 UTC),
                datetime!(2026-09-27 07:58:02 UTC),
            ]
        );
        assert_eq!(pings[0].offset(), UtcOffset::UTC);
    }

    #[test]
    fn format_is_second_precision_with_z() {
        let ts = datetime!(2026-09-26 10:12:44 +02:00);
        assert_eq!(format_timestamp(ts), "2026-09-26T08:12:44Z");
    }

    #[tokio::test]
    async fn open_missing_file_and_dir() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("nested/data");
        let store = Store::open(&data_dir).unwrap();
        assert!(data_dir.is_dir());
        assert!(!data_dir.join(LOG_FILE).exists());
        let snap = store.snapshot(10).await;
        assert_eq!((snap.latest, snap.total), (None, 0));
    }

    #[tokio::test]
    async fn open_unsorted_file_with_garbage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(LOG_FILE),
            "2026-09-27T07:58:02Z\nnot a date\n2026-09-26T08:12:44Z\n",
        )
        .unwrap();
        let snap = Store::open(dir.path()).unwrap().snapshot(10).await;
        assert_eq!(snap.total, 2);
        assert_eq!(snap.latest, Some(datetime!(2026-09-27 07:58:02 UTC)));
        assert_eq!(
            snap.recent,
            [
                datetime!(2026-09-27 07:58:02 UTC),
                datetime!(2026-09-26 08:12:44 UTC),
            ]
        );
    }

    #[tokio::test]
    async fn append_writes_line_and_keeps_bad_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE);
        // Hand-edited file: a bad line and no trailing newline.
        std::fs::write(&path, "oops\n2020-01-01T00:00:00Z").unwrap();
        let store = Store::open(dir.path()).unwrap();

        let ts = store.append().await.unwrap();
        assert_eq!(ts.nanosecond(), 0);

        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            contents,
            format!("oops\n2020-01-01T00:00:00Z\n{}\n", format_timestamp(ts))
        );
        let snap = store.snapshot(1).await;
        assert_eq!(
            (snap.latest, snap.total, snap.recent),
            (Some(ts), 2, vec![ts])
        );
    }
}
