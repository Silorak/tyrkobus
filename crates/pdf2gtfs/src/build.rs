//! `pdf2gtfs build`: archive → GTFS feed + report.

use crate::archive::Archive;
use crate::gtfs::{self, Location, Source};
use crate::line::{self, Line};
use crate::osm::Extract;
use crate::plausible;
use crate::shapes;
use crate::stops::Locator;
use chrono::NaiveDate;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Default, Serialize, serde::Deserialize, Clone)]
pub struct BuildReport {
    pub built: String,
    pub pdfs: usize,
    pub pdfs_failed: Vec<(String, String)>,
    pub lines: usize,
    pub trips: usize,
    pub trips_excluded: Vec<(String, u32, String)>,
    pub trips_without_location: usize,
    pub stops: usize,
    pub stops_approximate: usize,
    pub stops_unlocated: Vec<String>,
    pub first_date: Option<NaiveDate>,
    pub last_date: Option<NaiveDate>,
    /// Line → last day its timetable is valid.
    #[serde(default)]
    pub lines_valid_to: BTreeMap<String, NaiveDate>,
    /// Stops whose matched position contradicted the timetable; shown approximate.
    #[serde(default)]
    pub stops_doubted: Vec<String>,
    /// Distinct trip paths drawn along the roads (GTFS shapes).
    #[serde(default)]
    pub shapes: usize,
    /// Stops OSM does not know, placed on the bus's road by timetable minutes.
    #[serde(default)]
    pub stops_placed_on_road: usize,
    /// Walks between nearby stops measured along footpaths and streets.
    #[serde(default)]
    pub walks: usize,
    /// Guessed stops whose road estimates from different trips disagreed; left as they were.
    #[serde(default)]
    pub stops_estimates_disagree: Vec<String>,
    /// Why this build was not published (empty when it was).
    #[serde(default)]
    pub refused: Vec<String>,
}

/// Fewer trips than this share of the live feed means something broke.
const MIN_TRIP_SHARE: f64 = 0.7;
/// The feed must cover at least this many days ahead.
const MIN_DAYS_AHEAD: u64 = 14;
/// More guessed stops than the live feed by this share of all stops means
/// stop matching broke (an empty OSM extract, a renamed field).
const MAX_APPROXIMATE_RISE: f64 = 0.10;
/// More unreadable PDFs than this share means the PDF format changed.
const MAX_PDFS_FAILED: f64 = 0.05;

/// reasons not to replace the live feed with `new`.
pub fn publish_check(
    live: Option<&BuildReport>,
    new: &BuildReport,
    today: NaiveDate,
) -> Vec<String> {
    let mut why = vec![];
    if let Some(live) = live {
        if (new.trips as f64) < live.trips as f64 * MIN_TRIP_SHARE {
            why.push(format!(
                "trips dropped from {} to {} (below {:.0}%)",
                live.trips,
                new.trips,
                MIN_TRIP_SHARE * 100.0
            ));
        }
        let share = |r: &BuildReport| r.stops_approximate as f64 / r.stops.max(1) as f64;
        if live.stops > 0 && share(new) > share(live) + MAX_APPROXIMATE_RISE {
            why.push(format!(
                "guessed stop positions rose from {} to {} of {} stops",
                live.stops_approximate, new.stops_approximate, new.stops
            ));
        }
        for (line, valid_to) in &live.lines_valid_to {
            if *valid_to >= today && !new.lines_valid_to.contains_key(line) {
                why.push(format!(
                    "line {line} vanished but its timetable runs until {valid_to}"
                ));
            }
        }
    }
    if new.pdfs_failed.len() as f64 > new.pdfs as f64 * MAX_PDFS_FAILED {
        why.push(format!(
            "{} of {} PDFs could not be read (format changed?)",
            new.pdfs_failed.len(),
            new.pdfs
        ));
    }
    let needed = today + chrono::Days::new(MIN_DAYS_AHEAD);
    match new.last_date {
        Some(last) if last >= needed => {}
        Some(last) => why.push(format!(
            "feed ends {last}, less than {MIN_DAYS_AHEAD} days ahead (next timetables not published yet?)"
        )),
        None => why.push("feed has no service dates".into()),
    }
    why
}

/// What makes up a published feed inside its directory.
const PARTS: [&str; 3] = ["feed", "gtfs.zip", "report.json"];

fn read_report(path: &Path) -> io::Result<Option<BuildReport>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(io::Error::other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Move the parts of a feed from one directory to another (replacing).
fn move_parts(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for part in PARTS {
        let (a, b) = (from.join(part), to.join(part));
        if !a.exists() {
            continue;
        }
        if b.is_dir() {
            std::fs::remove_dir_all(&b)?;
        } else if b.exists() {
            std::fs::remove_file(&b)?;
        }
        std::fs::rename(&a, &b)?;
    }
    Ok(())
}

/// swap a passing `out/staging` build in as the live feed
/// (`out/feed`, `out/gtfs.zip`, `out/report.json`); the live one moves to
/// `out/previous`. A refused build stays in `out/staging` with the reasons in
/// its report, and the live feed is not touched. Returns the reasons.
pub fn publish(out: &Path, today: NaiveDate) -> io::Result<Vec<String>> {
    let staging = out.join("staging");
    let mut new = read_report(&staging.join("report.json"))?
        .ok_or_else(|| io::Error::other("no build in staging/"))?;
    let live = read_report(&out.join("report.json"))?;
    let why = publish_check(live.as_ref(), &new, today);
    if !why.is_empty() {
        new.refused = why.clone();
        std::fs::write(
            staging.join("report.json"),
            serde_json::to_vec_pretty(&new).map_err(io::Error::other)?,
        )?;
        return Ok(why);
    }
    let previous = out.join("previous");
    let _ = std::fs::remove_dir_all(&previous);
    if live.is_some() {
        move_parts(out, &previous)?;
    }
    move_parts(&staging, out)?;
    std::fs::remove_dir_all(&staging)?;
    Ok(why)
}

/// Swap the live feed with `out/previous` (run again to undo).
pub fn rollback(out: &Path) -> io::Result<()> {
    let (previous, swap) = (out.join("previous"), out.join("rollback-swap"));
    if !previous.join("report.json").exists() {
        return Err(io::Error::other("no previous feed to go back to"));
    }
    let _ = std::fs::remove_dir_all(&swap);
    move_parts(out, &swap)?;
    move_parts(&previous, out)?;
    move_parts(&swap, &previous)?;
    std::fs::remove_dir_all(&swap)
}

pub fn build(
    archive_dir: &Path,
    stops_file: &Path,
    out: &Path,
    today: NaiveDate,
) -> io::Result<BuildReport> {
    let archive = Archive::open(archive_dir)?;
    let extract: Extract =
        serde_json::from_slice(&std::fs::read(stops_file)?).map_err(io::Error::other)?;
    let locator = Locator::new(&extract);

    let mut report = BuildReport {
        built: today.to_string(),
        ..Default::default()
    };
    // (line, valid_from) → newest version seen, so a PDF listed twice counts once.
    let mut built: BTreeMap<(String, NaiveDate), (Line, String, String)> = BTreeMap::new();
    for (url, source) in &archive.index.sources {
        for version in &source.versions {
            report.pdfs += 1;
            let path = archive.pdf_path(&version.sha256);
            let result = crate::words::extract(&path)
                .map_err(|e| e.to_string())
                .and_then(|w| line::build(&w));
            match result {
                Err(e) => report.pdfs_failed.push((url.clone(), e)),
                Ok(l) if l.meta.valid_to < today => {} // expired
                Ok(l) => {
                    let key = (l.meta.line.clone(), l.meta.valid_from);
                    built.insert(key, (l, url.clone(), version.sha256.clone()));
                }
            }
        }
    }
    for (l, _, _) in built.values() {
        for (n, why) in &l.excluded {
            report
                .trips_excluded
                .push((l.meta.line.clone(), *n, why.clone()));
        }
    }
    // Stage E: where is each stop, and does that agree with the timetable?
    // Names as the feed writes them: town buses get their town ("AS" → "Komárno, AS").
    let mut located: BTreeMap<String, Option<Location>> = BTreeMap::new();
    let mut hops: Vec<plausible::Hop> = vec![];
    // Distinct stop sequences, with the departure minutes of the first trip seen.
    let mut patterns: BTreeMap<Vec<String>, Vec<(u32, u32)>> = BTreeMap::new();
    for (l, url, _) in built.values() {
        let city = gtfs::mhd_city(url);
        for t in &l.trips {
            let names: Vec<String> = t
                .stop_times
                .iter()
                .map(|st| gtfs::stop_name(&st.stop, city))
                .collect();
            for n in &names {
                located
                    .entry(n.clone())
                    .or_insert_with(|| locator.locate(n));
            }
            patterns.entry(names.clone()).or_insert_with(|| {
                t.stop_times
                    .iter()
                    .map(|st| (st.arrival, st.departure))
                    .collect()
            });
            for (i, w) in t.stop_times.windows(2).enumerate() {
                let minutes = w[1].arrival.saturating_sub(w[0].departure);
                hops.push((names[i].clone(), names[i + 1].clone(), minutes));
            }
        }
    }
    let known: BTreeMap<String, Location> = located
        .iter()
        .filter_map(|(n, l)| l.map(|l| (n.clone(), l)))
        .collect();
    let doubted = plausible::implausible(&hops, &known);
    for name in &doubted {
        let wrong = known[name];
        let fallback = locator.approximate(name).unwrap_or(Location {
            approximate: true,
            ..wrong
        });
        located.insert(name.clone(), Some(fallback));
    }
    report.stops_doubted = doubted.into_iter().collect();

    // Stage E3/E4: trips along the roads; guessed stops placed on the road.
    let mut shape_by_names: std::collections::HashMap<Vec<String>, std::sync::Arc<gtfs::ShapeRef>> =
        Default::default();
    let roads_file = stops_file.with_file_name("roads.bin");
    match crate::roads::Roads::load(&roads_file) {
        Err(e) => eprintln!(
            "no road shapes ({}: {e}); trips are drawn stop to stop",
            roads_file.display()
        ),
        Ok(roads) => {
            let mut route = shapes::Cached::new(crate::roads::Router::new(&roads));
            let calls_of =
                |names: &Vec<String>,
                 times: &Vec<(u32, u32)>,
                 located: &BTreeMap<String, Option<Location>>| {
                    names
                        .iter()
                        .zip(times)
                        .map(|(n, &(arrival, departure))| {
                            located.get(n).copied().flatten().map(|at| shapes::Call {
                                at,
                                arrival,
                                departure,
                            })
                        })
                        .collect::<Option<Vec<_>>>()
                };
            // Pass 1: where each trip's road says its guessed stops are.
            let mut estimates: BTreeMap<String, Vec<shapes::Point>> = BTreeMap::new();
            for (names, times) in &patterns {
                let Some(calls) = calls_of(names, times, &located) else {
                    continue;
                };
                let traced = shapes::trace(&calls, &mut route);
                for (n, p) in names.iter().zip(&traced.placed) {
                    if let Some(p) = p {
                        estimates.entry(n.clone()).or_default().push(*p);
                    }
                }
            }
            // A stop on several routes: used only where the routes agree.
            for (name, ps) in estimates {
                match shapes::consensus(&ps, 500.0) {
                    Some((lat, lon)) => {
                        located.insert(
                            name,
                            Some(Location {
                                lat,
                                lon,
                                approximate: true,
                            }),
                        );
                        report.stops_placed_on_road += 1;
                    }
                    None => report.stops_estimates_disagree.push(name),
                }
            }
            // Pass 2: the shapes, from the stops' final positions.
            for (names, times) in &patterns {
                let Some(calls) = calls_of(names, times, &located) else {
                    continue;
                };
                let traced = shapes::trace(&calls, &mut route);
                let id = format!("sh{}", shape_by_names.len() + 1);
                shape_by_names.insert(
                    names.clone(),
                    std::sync::Arc::new(gtfs::ShapeRef {
                        id,
                        points: traced.points,
                        dist: traced.dist,
                    }),
                );
            }
            report.shapes = shape_by_names.len();
        }
    }

    let sources: Vec<Source> = built
        .values()
        .map(|(l, url, sha)| Source {
            line: l,
            url: url.clone(),
            sha256: sha.clone(),
        })
        .collect();

    let feed = out.join("feed");
    let _ = std::fs::remove_dir_all(&feed);
    let locate = |name: &str| -> Option<Location> {
        match located.get(name) {
            Some(l) => *l,
            None => locator.locate(name),
        }
    };
    let shape_of = |names: &[String]| shape_by_names.get(names).cloned();
    let g = gtfs::write_with_shapes(&feed, &sources, &locate, &shape_of, today)?;
    // Walks between nearby stops, along the footpaths.
    let walk_file = stops_file.with_file_name("walk.bin");
    match crate::roads::Roads::load(&walk_file) {
        Err(e) => eprintln!(
            "no walking paths ({}: {e}); walks are drawn straight",
            walk_file.display()
        ),
        Ok(net) => {
            let mut stops = vec![];
            let mut r = csv::Reader::from_path(feed.join("stops.txt")).map_err(io::Error::other)?;
            for row in r.deserialize::<BTreeMap<String, String>>() {
                let row = row.map_err(io::Error::other)?;
                let num = |k: &str| row.get(k).and_then(|v| v.parse::<f64>().ok());
                if let (Some(id), Some(lat), Some(lon)) =
                    (row.get("stop_id"), num("stop_lat"), num("stop_lon"))
                {
                    stops.push(crate::walks::StopAt {
                        id: id.clone(),
                        lat,
                        lon,
                        exact: row.get("stop_desc").is_none_or(|d| d.is_empty()),
                    });
                }
            }
            let walks = crate::walks::walks(&stops, &crate::roads::Router::walking(&net));
            crate::walks::write(&feed, &walks)?;
            report.walks = walks.len();
        }
    }
    zip_dir(&feed, &out.join("gtfs.zip"))?;

    report.lines = g.routes;
    report.trips = g.trips;
    report.trips_without_location = g.trips_without_location;
    report.stops = g.stops;
    report.stops_approximate = g.stops_approximate;
    report.stops_unlocated = g.stops_unlocated;
    report.first_date = g.first_date;
    report.last_date = g.last_date;
    std::fs::write(
        out.join("report.json"),
        serde_json::to_vec_pretty(&report).map_err(io::Error::other)?,
    )?;
    Ok(report)
}

fn zip_dir(dir: &Path, zip_path: &Path) -> io::Result<()> {
    let file = std::fs::File::create(zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut names: Vec<_> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    names.sort();
    for path in names {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        zip.start_file(name, options).map_err(io::Error::other)?;
        zip.write_all(&std::fs::read(&path)?)?;
    }
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn report(trips: usize, last: &str, lines: &[(&str, &str)]) -> BuildReport {
        BuildReport {
            trips,
            last_date: Some(d(last)),
            lines_valid_to: lines.iter().map(|(l, v)| (l.to_string(), d(v))).collect(),
            ..Default::default()
        }
    }

    const TODAY: &str = "2026-11-24";

    #[test]
    fn first_build_is_published_when_it_covers_two_weeks() {
        let new = report(2842, "2026-12-12", &[("401401", "2026-12-12")]);
        assert!(publish_check(None, &new, d(TODAY)).is_empty());
    }

    #[test]
    fn a_similar_rebuild_is_published() {
        let live = report(2842, "2026-12-12", &[("401401", "2026-12-12")]);
        let new = report(2800, "2026-12-12", &[("401401", "2026-12-12")]);
        assert!(publish_check(Some(&live), &new, d(TODAY)).is_empty());
    }

    #[test]
    fn a_sharp_drop_in_trips_is_refused() {
        let live = report(2842, "2026-12-12", &[]);
        let new = report(1500, "2026-12-12", &[]);
        let why = publish_check(Some(&live), &new, d(TODAY));
        assert_eq!(why.len(), 1);
        assert!(
            why[0].contains("1500") && why[0].contains("2842"),
            "{why:?}"
        );
    }

    #[test]
    fn a_line_vanishing_before_its_timetable_ends_is_refused() {
        let live = report(
            100,
            "2026-12-12",
            &[("401401", "2026-12-12"), ("404434", "2026-12-12")],
        );
        let new = report(100, "2026-12-12", &[("401401", "2026-12-12")]);
        let why = publish_check(Some(&live), &new, d(TODAY));
        assert!(why.iter().any(|w| w.contains("404434")), "{why:?}");
    }

    #[test]
    fn a_line_whose_timetable_ended_may_go() {
        let live = report(
            100,
            "2027-12-11",
            &[("401401", "2027-12-11"), ("404434", "2026-11-20")],
        );
        let new = report(100, "2027-12-11", &[("401401", "2027-12-11")]);
        assert!(publish_check(Some(&live), &new, d(TODAY)).is_empty());
    }

    #[test]
    fn a_feed_ending_within_two_weeks_is_refused() {
        let new = report(2842, "2026-12-01", &[]);
        let why = publish_check(None, &new, d(TODAY));
        assert!(why.iter().any(|w| w.contains("2026-12-01")), "{why:?}");
    }

    #[test]
    fn many_more_guessed_stops_than_before_is_refused() {
        // Stop matching broke (e.g. OSM extract empty): 20% → 60% approximate.
        let mut live = report(2842, "2026-12-12", &[]);
        (live.stops, live.stops_approximate) = (1380, 280);
        let mut new = report(2842, "2026-12-12", &[]);
        (new.stops, new.stops_approximate) = (1380, 830);
        let why = publish_check(Some(&live), &new, d(TODAY));
        assert!(why.iter().any(|w| w.contains("830")), "{why:?}");
        // A few more is normal churn.
        new.stops_approximate = 300;
        assert!(publish_check(Some(&live), &new, d(TODAY)).is_empty());
    }

    #[test]
    fn many_unreadable_pdfs_are_refused() {
        let mut new = report(2842, "2026-12-12", &[]);
        new.pdfs = 116;
        new.pdfs_failed = (0..15)
            .map(|i| (format!("{i}.pdf"), "no grid".into()))
            .collect();
        let why = publish_check(None, &new, d(TODAY));
        assert!(why.iter().any(|w| w.contains("15 of 116")), "{why:?}");
        new.pdfs_failed.truncate(2);
        assert!(publish_check(None, &new, d(TODAY)).is_empty());
    }

    /// A feed directory as `build` leaves it: feed/, gtfs.zip, report.json.
    fn built(dir: &Path, r: &BuildReport, marker: &str) {
        std::fs::create_dir_all(dir.join("feed")).unwrap();
        std::fs::write(dir.join("feed/feed_info.txt"), marker).unwrap();
        std::fs::write(dir.join("gtfs.zip"), marker).unwrap();
        std::fs::write(dir.join("report.json"), serde_json::to_vec(r).unwrap()).unwrap();
    }

    fn marker(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("feed/feed_info.txt")).unwrap()
    }

    #[test]
    fn a_good_build_replaces_the_live_feed_and_the_old_one_is_kept() {
        let out = tempfile::tempdir().unwrap();
        built(out.path(), &report(2842, "2026-12-12", &[]), "old");
        built(
            &out.path().join("staging"),
            &report(2840, "2026-12-12", &[]),
            "new",
        );
        let why = publish(out.path(), d(TODAY)).unwrap();
        assert!(why.is_empty(), "{why:?}");
        assert_eq!(marker(out.path()), "new");
        assert_eq!(
            std::fs::read_to_string(out.path().join("gtfs.zip")).unwrap(),
            "new"
        );
        assert_eq!(marker(&out.path().join("previous")), "old");
        assert!(!out.path().join("staging").exists());
    }

    #[test]
    fn a_refused_build_leaves_the_live_feed_alone() {
        let out = tempfile::tempdir().unwrap();
        built(out.path(), &report(2842, "2026-12-12", &[]), "old");
        built(
            &out.path().join("staging"),
            &report(900, "2026-12-12", &[]),
            "broken",
        );
        let why = publish(out.path(), d(TODAY)).unwrap();
        assert_eq!(why.len(), 1, "{why:?}");
        assert_eq!(marker(out.path()), "old");
        assert_eq!(
            std::fs::read_to_string(out.path().join("gtfs.zip")).unwrap(),
            "old"
        );
        // Kept for a look, with the reasons written into its report.
        let r: BuildReport =
            serde_json::from_slice(&std::fs::read(out.path().join("staging/report.json")).unwrap())
                .unwrap();
        assert_eq!(r.refused, why);
    }

    #[test]
    fn the_first_build_is_published_when_it_passes() {
        let out = tempfile::tempdir().unwrap();
        built(
            &out.path().join("staging"),
            &report(2842, "2026-12-12", &[]),
            "first",
        );
        assert!(publish(out.path(), d(TODAY)).unwrap().is_empty());
        assert_eq!(marker(out.path()), "first");
    }

    #[test]
    fn rollback_brings_the_previous_feed_back() {
        let out = tempfile::tempdir().unwrap();
        built(out.path(), &report(2842, "2026-12-12", &[]), "old");
        built(
            &out.path().join("staging"),
            &report(2840, "2026-12-12", &[]),
            "new",
        );
        publish(out.path(), d(TODAY)).unwrap();
        rollback(out.path()).unwrap();
        assert_eq!(marker(out.path()), "old");
        assert_eq!(marker(&out.path().join("previous")), "new");
    }
}
