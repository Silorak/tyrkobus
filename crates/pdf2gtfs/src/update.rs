//! One `pdf2gtfs update` run: discover, download what changed, archive
//!. Building and publishing come later.

use crate::archive::{Archive, Run, Stored};
use crate::discover::timetable_links;
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

pub struct HttpResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: Vec<u8>,
}

/// Conditional GET. A 304 is a normal response, not an error.
pub trait Http {
    fn get(
        &self,
        url: &str,
        if_none_match: Option<&str>,
        if_modified_since: Option<&str>,
    ) -> Result<HttpResponse, String>;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub listed: usize,
    pub new_versions: Vec<String>,
    pub not_modified: usize,
    pub unchanged: usize,
    /// In the archive and listed last run, but not listed now. Kept, never deleted.
    pub delisted: Vec<String>,
    pub failed: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateError {
    Listing {
        url: String,
        reason: String,
    },
    /// Far fewer PDFs listed than last run: likely a site redesign, not real removals.
    TooFewLinks {
        found: usize,
        previous: usize,
    },
    Io(String),
}

pub fn update(
    http: &dyn Http,
    archive: &mut Archive,
    listing_pages: &[&str],
    now: DateTime<Utc>,
) -> Result<Report, UpdateError> {
    let mut listed = BTreeSet::new();
    for &page in listing_pages {
        let html = http
            .get(page, None, None)
            .and_then(|r| match r.status {
                200 => Ok(String::from_utf8_lossy(&r.body).into_owned()),
                s => Err(format!("HTTP {s}")),
            })
            .map_err(|reason| UpdateError::Listing {
                url: page.to_string(),
                reason,
            })?;
        listed.extend(timetable_links(&html));
    }

    let previous = archive
        .index
        .last_run
        .as_ref()
        .map(|r| r.listed.clone())
        .unwrap_or_default();
    if listed.is_empty() || listed.len() * 2 < previous.len() {
        return Err(UpdateError::TooFewLinks {
            found: listed.len(),
            previous: previous.len(),
        });
    }

    let mut report = Report {
        listed: listed.len(),
        delisted: previous
            .iter()
            .filter(|u| !listed.contains(*u))
            .cloned()
            .collect(),
        ..Report::default()
    };
    for url in &listed {
        let source = archive.index.sources.get(url);
        let etag = source.and_then(|s| s.etag.clone());
        let last_modified = source.and_then(|s| s.last_modified.clone());
        match http.get(url, etag.as_deref(), last_modified.as_deref()) {
            Ok(r) if r.status == 304 => report.not_modified += 1,
            Ok(r) if r.status == 200 => {
                match archive.store(url, &r.body, r.etag, r.last_modified, now) {
                    Ok(Stored::NewVersion { .. }) => report.new_versions.push(url.clone()),
                    Ok(Stored::Unchanged) => report.unchanged += 1,
                    Err(e) => return Err(UpdateError::Io(e.to_string())),
                }
            }
            Ok(r) => report
                .failed
                .push((url.clone(), format!("HTTP {}", r.status))),
            Err(e) => report.failed.push((url.clone(), e)),
        }
    }

    archive.index.last_run = Some(Run {
        at: now,
        listed: listed.into_iter().collect(),
    });
    archive.save().map_err(|e| UpdateError::Io(e.to_string()))?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    const LISTING: &str = "https://arriva.sk/listing/";

    fn pdf(n: u32) -> String {
        format!("https://arriva.sk/files/nz/cp/primesto/komarno/2026/{n}.pdf")
    }

    fn at(day: u32) -> DateTime<Utc> {
        format!("2026-11-{day:02}T06:00:00Z").parse().unwrap()
    }

    /// A fake arriva.sk: serves a listing page and PDFs, honours If-None-Match,
    /// and records every request.
    #[derive(Default)]
    struct FakeSite {
        listing: Option<String>,
        files: HashMap<String, (String, Vec<u8>)>, // url -> (etag, body)
        requests: RefCell<Vec<(String, Option<String>)>>,
    }

    impl FakeSite {
        fn with(pdfs: &[(String, &str)]) -> Self {
            let mut site = FakeSite::default();
            site.set(pdfs);
            site
        }
        fn set(&mut self, pdfs: &[(String, &str)]) {
            self.listing = Some(
                pdfs.iter()
                    .map(|(u, _)| format!("<a href=\"{u}\">CP</a>\n"))
                    .collect(),
            );
            self.files = pdfs
                .iter()
                .map(|(u, body)| (u.clone(), (format!("\"{body}\""), body.as_bytes().to_vec())))
                .collect();
        }
        fn pdf_requests(&self) -> Vec<(String, Option<String>)> {
            self.requests
                .borrow()
                .iter()
                .filter(|(u, _)| u != LISTING)
                .cloned()
                .collect()
        }
    }

    impl Http for FakeSite {
        fn get(
            &self,
            url: &str,
            inm: Option<&str>,
            _ims: Option<&str>,
        ) -> Result<HttpResponse, String> {
            self.requests
                .borrow_mut()
                .push((url.to_string(), inm.map(str::to_string)));
            if url == LISTING {
                let html = self.listing.clone().ok_or("listing down")?;
                return Ok(HttpResponse {
                    status: 200,
                    etag: None,
                    last_modified: None,
                    body: html.into_bytes(),
                });
            }
            let (etag, body) = self.files.get(url).ok_or(format!("404 {url}"))?;
            if inm == Some(etag.as_str()) {
                return Ok(HttpResponse {
                    status: 304,
                    etag: Some(etag.clone()),
                    last_modified: None,
                    body: vec![],
                });
            }
            Ok(HttpResponse {
                status: 200,
                etag: Some(etag.clone()),
                last_modified: None,
                body: body.clone(),
            })
        }
    }

    #[test]
    fn first_run_downloads_every_listed_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let site = FakeSite::with(&[(pdf(401401), "a"), (pdf(401402), "b")]);

        let report = update(&site, &mut archive, &[LISTING], at(23)).unwrap();

        assert_eq!(report.listed, 2);
        assert_eq!(report.new_versions, vec![pdf(401401), pdf(401402)]);
        assert_eq!(archive.index.sources.len(), 2);
    }

    #[test]
    fn unchanged_pdf_is_asked_for_with_its_etag_and_not_stored_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let site = FakeSite::with(&[(pdf(401401), "a")]);
        update(&site, &mut archive, &[LISTING], at(23)).unwrap();
        site.requests.borrow_mut().clear();

        let report = update(&site, &mut archive, &[LISTING], at(24)).unwrap();

        assert_eq!(
            site.pdf_requests(),
            vec![(pdf(401401), Some("\"a\"".to_string()))]
        );
        assert_eq!(report.not_modified, 1);
        assert!(report.new_versions.is_empty());
        assert_eq!(archive.index.sources[&pdf(401401)].versions.len(), 1);
    }

    #[test]
    fn changed_pdf_becomes_a_new_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let mut site = FakeSite::with(&[(pdf(401401), "a")]);
        update(&site, &mut archive, &[LISTING], at(23)).unwrap();
        site.set(&[(pdf(401401), "a-december")]);

        let report = update(&site, &mut archive, &[LISTING], at(24)).unwrap();

        assert_eq!(report.new_versions, vec![pdf(401401)]);
        assert_eq!(archive.index.sources[&pdf(401401)].versions.len(), 2);
    }

    #[test]
    fn far_fewer_links_than_last_run_stops_without_touching_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let all: Vec<_> = (0..10).map(|i| (pdf(401400 + i), "x")).collect();
        let mut site = FakeSite::with(&all);
        update(&site, &mut archive, &[LISTING], at(23)).unwrap();
        site.set(&[(pdf(401401), "changed")]);

        let err = update(&site, &mut archive, &[LISTING], at(24)).unwrap_err();

        assert_eq!(
            err,
            UpdateError::TooFewLinks {
                found: 1,
                previous: 10
            }
        );
        assert_eq!(archive.index.sources[&pdf(401401)].versions.len(), 1);
        assert_eq!(archive.index.last_run.as_ref().unwrap().at, at(23));
    }

    #[test]
    fn no_links_at_all_is_an_error_even_on_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let site = FakeSite::with(&[]);

        let err = update(&site, &mut archive, &[LISTING], at(23)).unwrap_err();

        assert_eq!(
            err,
            UpdateError::TooFewLinks {
                found: 0,
                previous: 0
            }
        );
    }

    #[test]
    fn listing_page_down_stops_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let site = FakeSite::default();

        let err = update(&site, &mut archive, &[LISTING], at(23)).unwrap_err();

        assert!(matches!(err, UpdateError::Listing { .. }));
    }

    #[test]
    fn pdf_gone_from_listing_is_reported_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let mut site = FakeSite::with(&[(pdf(401401), "a"), (pdf(401412), "old line")]);
        update(&site, &mut archive, &[LISTING], at(23)).unwrap();
        site.set(&[(pdf(401401), "a")]);

        let report = update(&site, &mut archive, &[LISTING], at(24)).unwrap();

        assert_eq!(report.delisted, vec![pdf(401412)]);
        assert!(archive.index.sources.contains_key(&pdf(401412)));
    }

    #[test]
    fn one_failed_download_does_not_stop_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let mut site = FakeSite::with(&[(pdf(401401), "a"), (pdf(401402), "b")]);
        site.files.remove(&pdf(401401));

        let report = update(&site, &mut archive, &[LISTING], at(23)).unwrap();

        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, pdf(401401));
        assert_eq!(report.new_versions, vec![pdf(401402)]);
    }

    #[test]
    fn run_is_saved_so_the_next_run_can_compare() {
        let dir = tempfile::tempdir().unwrap();
        let mut archive = Archive::open(dir.path()).unwrap();
        let site = FakeSite::with(&[(pdf(401401), "a")]);

        update(&site, &mut archive, &[LISTING], at(23)).unwrap();

        let reopened = Archive::open(dir.path()).unwrap();
        let run = reopened.index.last_run.unwrap();
        assert_eq!(run.at, at(23));
        assert_eq!(run.listed, vec![pdf(401401)]);
    }
}
