//! Content-addressed store of every timetable PDF version ever downloaded
//!. Layout: `<dir>/pdf/<sha256>.pdf` + `<dir>/index.json`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Index {
    /// Keyed by PDF URL.
    pub sources: BTreeMap<String, Source>,
    pub last_run: Option<Run>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub versions: Vec<Version>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub sha256: String,
    pub first_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub at: DateTime<Utc>,
    /// URLs found on the listing pages in that run.
    pub listed: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Stored {
    NewVersion { sha256: String },
    Unchanged,
}

pub struct Archive {
    dir: PathBuf,
    pub index: Index,
}

impl Archive {
    pub fn open(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir.join("pdf"))?;
        let index = match std::fs::read(dir.join("index.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Index::default(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            index,
        })
    }

    pub fn pdf_path(&self, sha256: &str) -> PathBuf {
        self.dir.join("pdf").join(format!("{sha256}.pdf"))
    }

    /// Store a downloaded body for `url`. Files are never overwritten or deleted.
    pub fn store(
        &mut self,
        url: &str,
        body: &[u8],
        etag: Option<String>,
        last_modified: Option<String>,
        now: DateTime<Utc>,
    ) -> io::Result<Stored> {
        let sha256: String = Sha256::digest(body)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let path = self.pdf_path(&sha256);
        if !path.exists() {
            write_atomically(&path, body)?;
        }
        let source = self.index.sources.entry(url.to_string()).or_insert(Source {
            etag: None,
            last_modified: None,
            versions: vec![],
        });
        source.etag = etag;
        source.last_modified = last_modified;
        if source.versions.last().is_some_and(|v| v.sha256 == sha256) {
            return Ok(Stored::Unchanged);
        }
        source.versions.push(Version {
            sha256: sha256.clone(),
            first_seen: now,
        });
        Ok(Stored::NewVersion { sha256 })
    }

    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(&self.index).map_err(io::Error::other)?;
        write_atomically(&self.dir.join("index.json"), &json)
    }
}

/// Write to a temp file and rename, so a crash never leaves a half-written file.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://arriva.sk/files/nz/cp/primesto/komarno/2026/401401.pdf";

    fn at(day: u32) -> DateTime<Utc> {
        format!("2026-11-{day:02}T06:00:00Z").parse().unwrap()
    }

    #[test]
    fn new_download_is_kept_under_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Archive::open(dir.path()).unwrap();

        let stored = a
            .store(URL, b"v1", Some("\"e1\"".into()), None, at(23))
            .unwrap();

        // sha256("v1")
        let sha = "3bfc269594ef649228e9a74bab00f042efc91d5acc6fbee31a382e80d42388fe";
        assert_eq!(stored, Stored::NewVersion { sha256: sha.into() });
        assert_eq!(std::fs::read(a.pdf_path(sha)).unwrap(), b"v1");
        assert_eq!(a.index.sources[URL].etag.as_deref(), Some("\"e1\""));
    }

    #[test]
    fn same_bytes_again_add_no_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Archive::open(dir.path()).unwrap();
        a.store(URL, b"v1", None, None, at(23)).unwrap();

        let stored = a.store(URL, b"v1", None, None, at(24)).unwrap();

        assert_eq!(stored, Stored::Unchanged);
        assert_eq!(a.index.sources[URL].versions.len(), 1);
        assert_eq!(a.index.sources[URL].versions[0].first_seen, at(23));
    }

    #[test]
    fn changed_bytes_add_a_version_and_keep_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Archive::open(dir.path()).unwrap();
        let Stored::NewVersion { sha256: old } = a.store(URL, b"v1", None, None, at(23)).unwrap()
        else {
            panic!("first store must be new")
        };

        let stored = a
            .store(URL, b"v2", Some("\"e2\"".into()), None, at(24))
            .unwrap();

        assert!(matches!(stored, Stored::NewVersion { .. }));
        let versions = &a.index.sources[URL].versions;
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[1].first_seen, at(24));
        assert_eq!(std::fs::read(a.pdf_path(&old)).unwrap(), b"v1");
        assert_eq!(a.index.sources[URL].etag.as_deref(), Some("\"e2\""));
    }

    #[test]
    fn index_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = Archive::open(dir.path()).unwrap();
        a.store(
            URL,
            b"v1",
            Some("\"e1\"".into()),
            Some("Fri, 28 Nov 2025 09:15:57 GMT".into()),
            at(23),
        )
        .unwrap();
        a.save().unwrap();

        let b = Archive::open(dir.path()).unwrap();

        let s = &b.index.sources[URL];
        assert_eq!(s.etag.as_deref(), Some("\"e1\""));
        assert_eq!(
            s.last_modified.as_deref(),
            Some("Fri, 28 Nov 2025 09:15:57 GMT")
        );
        assert_eq!(s.versions.len(), 1);
    }
}
