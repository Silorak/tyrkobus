//! Stage E: write a GTFS feed (plus a provenance file) from built lines.

use crate::line::Line;
use chrono::{Days, NaiveDate};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// One PDF version of a line, as built, with where it came from.
pub struct Source<'a> {
    pub line: &'a Line,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    /// Placed at the village/town centre, not at the actual stop.
    pub approximate: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub routes: usize,
    pub trips: usize,
    pub stops: usize,
    pub stops_approximate: usize,
    /// Stops with no location at all; trips using them are left out.
    pub stops_unlocated: Vec<String>,
    pub trips_without_location: usize,
    pub first_date: Option<NaiveDate>,
    pub last_date: Option<NaiveDate>,
}

/// City served by a town-bus PDF, from its URL (`…/cp/mhd/nove-zamky/…`).
pub fn mhd_city(url: &str) -> Option<&'static str> {
    let slug = url.split("/cp/mhd/").nth(1)?.split('/').next()?;
    Some(match slug {
        "komarno" => "Komárno",
        "levice" => "Levice",
        "nove-zamky" => "Nové Zámky",
        "sahy" => "Šahy",
        "sala" => "Šaľa",
        "sturovo" => "Štúrovo",
        "surany" => "Šurany",
        _ => return None,
    })
}

/// The name a stop is published under. Town-bus PDFs print "Námestie - Fő tér"
/// without the town; regional PDFs print "Komárno, AS".
pub fn stop_name(printed: &str, city: Option<&str>) -> String {
    static COMMA: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\s*,\s*").unwrap());
    let slovak = printed.split(" - ").next().unwrap_or(printed).trim();
    let slovak = COMMA.replace_all(slovak, ", ").to_string();
    match city {
        Some(c) if !slovak.starts_with(c) => format!("{c}, {slovak}"),
        _ => slovak,
    }
}

/// ASCII id from a name: "Komárno, Ďulov dvor" → "komarno-dulov-dvor".
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.to_lowercase().chars() {
        let c = match ch {
            'á' | 'ä' => 'a',
            'č' => 'c',
            'ď' => 'd',
            'é' | 'ě' => 'e',
            'í' => 'i',
            'ĺ' | 'ľ' => 'l',
            'ň' => 'n',
            'ó' | 'ô' | 'ö' | 'ő' => 'o',
            'ŕ' => 'r',
            'š' => 's',
            'ť' => 't',
            'ú' | 'ů' | 'ü' | 'ű' => 'u',
            'ý' => 'y',
            'ž' => 'z',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if c != '-' || !out.ends_with('-') {
            out.push(c);
        }
    }
    out.trim_matches('-').to_string()
}

fn hms(minutes: u32) -> String {
    format!("{:02}:{:02}:00", minutes / 60, minutes % 60)
}

fn route_names(line: &Line, city: Option<&str>) -> (String, String) {
    static TOWN_LINE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"linka\s+č\.\s*(\w+)").unwrap());
    match (city, TOWN_LINE.captures(&line.meta.name)) {
        (Some(c), Some(m)) => (m[1].to_string(), format!("MHD {c}")),
        _ => (line.meta.line.clone(), line.meta.name.clone()),
    }
}

/// A trip's path along the roads and each call's metres along it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRef {
    pub id: String,
    pub points: Vec<(f64, f64)>,
    pub dist: Vec<f64>,
}

/// Write `dir/*.txt` without road shapes. `locate(stop name)` gives coordinates.
pub fn write(
    dir: &Path,
    sources: &[Source],
    locate: &dyn Fn(&str) -> Option<Location>,
    built: NaiveDate,
) -> io::Result<Report> {
    write_with_shapes(dir, sources, locate, &|_| None, built)
}

/// Write `dir/*.txt`; `shape_of(stop names of a trip)` gives its road shape.
pub fn write_with_shapes(
    dir: &Path,
    sources: &[Source],
    locate: &dyn Fn(&str) -> Option<Location>,
    shape_of: &dyn Fn(&[String]) -> Option<std::sync::Arc<ShapeRef>>,
    built: NaiveDate,
) -> io::Result<Report> {
    std::fs::create_dir_all(dir)?;
    let csv_err = |e: csv::Error| io::Error::other(e);
    let mut report = Report::default();

    // A newer version of a line replaces the older one from its start date.
    let mut by_line: BTreeMap<&str, Vec<&Source>> = BTreeMap::new();
    for s in sources {
        by_line.entry(&s.line.meta.line).or_default().push(s);
    }
    let mut effective: Vec<(&Source, NaiveDate, NaiveDate)> = vec![];
    for versions in by_line.values_mut() {
        versions.sort_by_key(|s| s.line.meta.valid_from);
        for (i, s) in versions.iter().enumerate() {
            let mut to = s.line.meta.valid_to;
            if let Some(next) = versions.get(i + 1) {
                to = to.min(next.line.meta.valid_from - Days::new(1));
            }
            effective.push((s, s.line.meta.valid_from, to));
        }
    }

    let mut routes = csv::Writer::from_path(dir.join("routes.txt")).map_err(csv_err)?;
    routes
        .write_record([
            "route_id",
            "agency_id",
            "route_short_name",
            "route_long_name",
            "route_type",
        ])
        .map_err(csv_err)?;
    let mut trips = csv::Writer::from_path(dir.join("trips.txt")).map_err(csv_err)?;
    trips
        .write_record([
            "route_id",
            "service_id",
            "trip_id",
            "trip_headsign",
            "trip_short_name",
            "direction_id",
            "shape_id",
        ])
        .map_err(csv_err)?;
    let mut stop_times = csv::Writer::from_path(dir.join("stop_times.txt")).map_err(csv_err)?;
    stop_times
        .write_record([
            "trip_id",
            "arrival_time",
            "departure_time",
            "stop_id",
            "stop_sequence",
            "pickup_type",
            "drop_off_type",
            "timepoint",
            "shape_dist_traveled",
        ])
        .map_err(csv_err)?;
    let mut provenance = csv::Writer::from_path(dir.join("provenance.txt")).map_err(csv_err)?;
    provenance
        .write_record([
            "trip_id",
            "stop_sequence",
            "source_url",
            "pdf_sha256",
            "page",
            "x0",
            "y0",
            "x1",
            "y1",
        ])
        .map_err(csv_err)?;

    let mut services: BTreeMap<BTreeSet<NaiveDate>, String> = BTreeMap::new();
    // Keyed by stop_id: names differing only in punctuation are one stop (first name wins).
    let mut used_stops: BTreeMap<String, (String, Location)> = BTreeMap::new();
    let mut unlocated: BTreeSet<String> = BTreeSet::new();
    let mut routes_written: BTreeSet<String> = BTreeSet::new();
    let mut shapes: Option<csv::Writer<std::fs::File>> = None;
    let mut shapes_written: BTreeSet<String> = BTreeSet::new();

    for (src, from, to) in effective {
        let line = src.line;
        let city = mhd_city(&src.url);
        for trip in &line.trips {
            let dates: BTreeSet<NaiveDate> = trip
                .dates
                .iter()
                .copied()
                .filter(|d| *d >= from && *d <= to)
                .collect();
            if dates.is_empty() {
                continue;
            }
            let names: Vec<String> = trip
                .stop_times
                .iter()
                .map(|s| stop_name(&s.stop, city))
                .collect();
            let located: Vec<Option<Location>> = names.iter().map(|n| locate(n)).collect();
            if located.iter().any(Option::is_none) {
                for (n, l) in names.iter().zip(&located) {
                    if l.is_none() {
                        unlocated.insert(n.clone());
                    }
                }
                report.trips_without_location += 1;
                continue;
            }
            for (n, l) in names.iter().zip(&located) {
                used_stops
                    .entry(slug(n))
                    .or_insert_with(|| (n.clone(), l.unwrap()));
            }
            if routes_written.insert(line.meta.line.clone()) {
                let (short, long) = route_names(line, city);
                routes
                    .write_record([line.meta.line.as_str(), "arriva-nz", &short, &long, "3"])
                    .map_err(csv_err)?;
            }
            let next = format!("s{}", services.len() + 1);
            let service = services.entry(dates.clone()).or_insert(next).clone();
            let trip_id = format!(
                "{}-{}-{}",
                line.meta.line, line.meta.valid_from, trip.number
            );
            let direction = if trip.number % 2 == 0 { "1" } else { "0" };
            let shape = shape_of(&names).filter(|s| s.dist.len() == names.len());
            if let Some(sh) = &shape
                && shapes_written.insert(sh.id.clone())
            {
                if shapes.is_none() {
                    let mut w = csv::Writer::from_path(dir.join("shapes.txt")).map_err(csv_err)?;
                    w.write_record([
                        "shape_id",
                        "shape_pt_lat",
                        "shape_pt_lon",
                        "shape_pt_sequence",
                        "shape_dist_traveled",
                    ])
                    .map_err(csv_err)?;
                    shapes = Some(w);
                }
                let w = shapes.as_mut().expect("just created");
                let mut along = 0.0;
                for (i, p) in sh.points.iter().enumerate() {
                    if i > 0 {
                        along += crate::roads::metres(sh.points[i - 1], *p);
                    }
                    w.write_record([
                        sh.id.as_str(),
                        &format!("{:.6}", p.0),
                        &format!("{:.6}", p.1),
                        &(i + 1).to_string(),
                        &format!("{along:.2}"),
                    ])
                    .map_err(csv_err)?;
                }
            }
            let shape_id = shape.as_ref().map_or("", |s| s.id.as_str());
            trips
                .write_record([
                    line.meta.line.as_str(),
                    &service,
                    &trip_id,
                    names.last().unwrap(),
                    &trip.number.to_string(),
                    direction,
                    shape_id,
                ])
                .map_err(csv_err)?;
            for (i, (st, name)) in trip.stop_times.iter().zip(&names).enumerate() {
                let pickup = if st.request {
                    "3"
                } else if st.drop_off_only {
                    "1"
                } else {
                    "0"
                };
                let drop_off = if st.request {
                    "3"
                } else if st.pick_up_only {
                    "1"
                } else {
                    "0"
                };
                let seq = (i + 1).to_string();
                stop_times
                    .write_record([
                        trip_id.as_str(),
                        &hms(st.arrival),
                        &hms(st.departure),
                        &slug(name),
                        &seq,
                        pickup,
                        drop_off,
                        "1",
                        &shape
                            .as_ref()
                            .map_or(String::new(), |s| format!("{:.2}", s.dist[i])),
                    ])
                    .map_err(csv_err)?;
                let b = st.bbox;
                provenance
                    .write_record([
                        trip_id.as_str(),
                        &seq,
                        &src.url,
                        &src.sha256,
                        &st.page.to_string(),
                        &format!("{:.1}", b[0]),
                        &format!("{:.1}", b[1]),
                        &format!("{:.1}", b[2]),
                        &format!("{:.1}", b[3]),
                    ])
                    .map_err(csv_err)?;
            }
            report.trips += 1;
            report.first_date = report
                .first_date
                .min(dates.first().copied())
                .or(dates.first().copied());
            report.last_date = report.last_date.max(dates.last().copied());
        }
    }
    routes.flush()?;
    trips.flush()?;
    stop_times.flush()?;
    provenance.flush()?;
    if let Some(mut w) = shapes {
        w.flush()?;
    }

    let mut stops = csv::Writer::from_path(dir.join("stops.txt")).map_err(csv_err)?;
    stops
        .write_record(["stop_id", "stop_name", "stop_lat", "stop_lon", "stop_desc"])
        .map_err(csv_err)?;
    for (id, (name, l)) in &used_stops {
        let desc = if l.approximate {
            "poloha približná (stred obce)"
        } else {
            ""
        };
        stops
            .write_record([
                id,
                name,
                &format!("{:.6}", l.lat),
                &format!("{:.6}", l.lon),
                desc,
            ])
            .map_err(csv_err)?;
    }
    stops.flush()?;

    let mut cal = csv::Writer::from_path(dir.join("calendar_dates.txt")).map_err(csv_err)?;
    cal.write_record(["service_id", "date", "exception_type"])
        .map_err(csv_err)?;
    for (dates, id) in &services {
        for d in dates {
            cal.write_record([id.as_str(), &d.format("%Y%m%d").to_string(), "1"])
                .map_err(csv_err)?;
        }
    }
    cal.flush()?;

    let mut agency = csv::Writer::from_path(dir.join("agency.txt")).map_err(csv_err)?;
    agency
        .write_record([
            "agency_id",
            "agency_name",
            "agency_url",
            "agency_timezone",
            "agency_lang",
            "agency_phone",
        ])
        .map_err(csv_err)?;
    agency
        .write_record([
            "arriva-nz",
            "ARRIVA Nové Zámky, a.s.",
            "https://arriva.sk/nove-zamky/",
            "Europe/Bratislava",
            "sk",
            "+421 915 733 733",
        ])
        .map_err(csv_err)?;
    agency.flush()?;

    let fmt = |d: Option<NaiveDate>| {
        d.map(|d| d.format("%Y%m%d").to_string())
            .unwrap_or_default()
    };
    let mut info = csv::Writer::from_path(dir.join("feed_info.txt")).map_err(csv_err)?;
    info.write_record([
        "feed_publisher_name",
        "feed_publisher_url",
        "feed_lang",
        "feed_start_date",
        "feed_end_date",
        "feed_version",
        "feed_contact_url",
    ])
    .map_err(csv_err)?;
    info.write_record([
        "pdf2gtfs (unofficial, built from ARRIVA Nové Zámky PDF timetables)",
        "https://github.com/Silorak",
        "sk",
        &fmt(report.first_date),
        &fmt(report.last_date),
        &built.format("%Y-%m-%d").to_string(),
        "https://github.com/Silorak",
    ])
    .map_err(csv_err)?;
    info.flush()?;

    report.routes = routes_written.len();
    report.stops = used_stops.len();
    report.stops_approximate = used_stops.values().filter(|(_, l)| l.approximate).count();
    report.stops_unlocated = unlocated.into_iter().collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn line(name: &str) -> Line {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        crate::line::build(&crate::words::extract(&pdf).unwrap()).unwrap()
    }

    fn everywhere(_: &str) -> Option<Location> {
        Some(Location {
            lat: 47.87,
            lon: 18.19,
            approximate: false,
        })
    }

    fn read(dir: &Path, file: &str) -> Vec<BTreeMap<String, String>> {
        let mut r = csv::Reader::from_path(dir.join(file)).unwrap();
        r.deserialize().map(|x| x.unwrap()).collect()
    }

    const URL: &str = "https://arriva.sk/files/nz/cp/primesto/komarno/2026/401402.pdf";

    #[test]
    fn stop_names_for_town_buses_get_the_town_and_lose_the_translation() {
        assert_eq!(
            stop_name("Námestie - Fő tér", Some("Nové Zámky")),
            "Nové Zámky, Námestie"
        );
        assert_eq!(stop_name("Komárno, AS", None), "Komárno, AS");
        assert_eq!(stop_name("Komárno,AS", None), "Komárno, AS");
        assert_eq!(
            mhd_city("https://arriva.sk/files/nz/cp/mhd/nove-zamky/2026/404001.pdf"),
            Some("Nové Zámky")
        );
    }

    #[test]
    fn names_that_differ_only_in_punctuation_are_one_stop() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        let mut l2 = l.clone();
        l2.meta.line = "401499".into();
        for t in &mut l2.trips {
            for s in &mut t.stop_times {
                s.stop = s.stop.replace(", ", ",  ").replace("a.s.", "a. s.");
            }
        }
        let src = [
            Source {
                line: &l,
                url: URL.into(),
                sha256: "abc".into(),
            },
            Source {
                line: &l2,
                url: URL.into(),
                sha256: "def".into(),
            },
        ];
        write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        let stops = read(dir.path(), "stops.txt");
        let ids: BTreeSet<&String> = stops.iter().map(|s| &s["stop_id"]).collect();
        assert_eq!(ids.len(), stops.len(), "duplicate stop_id");
    }

    #[test]
    fn trips_get_their_road_shape_and_stops_their_distance_along_it() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        let shape_of = |names: &[String]| {
            Some(std::sync::Arc::new(ShapeRef {
                id: format!("sh{}", names.len()),
                points: vec![(47.87, 18.19), (47.88, 18.20), (47.89, 18.21)],
                dist: (0..names.len()).map(|i| i as f64 * 10.0).collect(),
            }))
        };
        write_with_shapes(
            dir.path(),
            &src,
            &everywhere,
            &shape_of,
            "2026-11-24".parse().unwrap(),
        )
        .unwrap();
        let trips = read(dir.path(), "trips.txt");
        assert!(trips.iter().all(|t| t["shape_id"].starts_with("sh")));
        let ids: BTreeSet<&String> = trips.iter().map(|t| &t["shape_id"]).collect();
        let shapes = read(dir.path(), "shapes.txt");
        assert_eq!(shapes.len(), ids.len() * 3, "each shape written once");
        let along: Vec<f64> = shapes
            .iter()
            .take(3)
            .map(|r| r["shape_dist_traveled"].parse().unwrap())
            .collect();
        assert!(along[0] == 0.0 && along[1] > 1000.0 && along[2] > along[1]);
        let st = read(dir.path(), "stop_times.txt");
        let first: Vec<f64> = st
            .iter()
            .filter(|r| r["trip_id"] == trips[0]["trip_id"])
            .map(|r| r["shape_dist_traveled"].parse().unwrap())
            .collect();
        assert!(first.len() > 2 && first.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn without_roads_there_are_no_shapes() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        assert!(!dir.path().join("shapes.txt").exists());
        assert!(
            read(dir.path(), "trips.txt")
                .iter()
                .all(|t| t["shape_id"].is_empty())
        );
    }

    #[test]
    fn feed_has_the_required_files_and_counts() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        let report = write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        for f in [
            "agency.txt",
            "routes.txt",
            "stops.txt",
            "trips.txt",
            "stop_times.txt",
            "calendar_dates.txt",
            "feed_info.txt",
            "provenance.txt",
        ] {
            assert!(dir.path().join(f).exists(), "{f}");
        }
        // Rendered PDF: 18 trips on page 1 and 21 on page 2.
        assert_eq!((report.routes, report.trips), (1, 39));
        assert_eq!(read(dir.path(), "trips.txt").len(), 39);
        let routes = read(dir.path(), "routes.txt");
        assert_eq!(routes[0]["route_short_name"], "401402");
        assert_eq!(
            routes[0]["route_long_name"],
            "Komárno-Kravany nad Dunajom-Nová Vieska"
        );
    }

    #[test]
    fn times_past_midnight_and_request_stops() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        let st = read(dir.path(), "stop_times.txt");
        // Rendered PDF, page 1, trip 39 ends at Kravany n. Dunajom, Jednota at 0 11.
        let t39: Vec<_> = st
            .iter()
            .filter(|r| r["trip_id"].ends_with("-39"))
            .collect();
        assert_eq!(t39.last().unwrap()["arrival_time"], "24:11:00");
        // Rendered PDF: "xKomárno, Ďulov dvor" is a request stop on both pages.
        let stops = read(dir.path(), "stops.txt");
        let dulov = stops
            .iter()
            .find(|s| s["stop_name"] == "Komárno, Ďulov dvor")
            .unwrap();
        let at_dulov: Vec<_> = st
            .iter()
            .filter(|r| r["stop_id"] == dulov["stop_id"])
            .collect();
        assert!(!at_dulov.is_empty());
        assert!(
            at_dulov
                .iter()
                .all(|r| r["pickup_type"] == "3" && r["drop_off_type"] == "3")
        );
    }

    #[test]
    fn newer_version_replaces_older_from_its_start_date() {
        let old = line("401402.pdf");
        let mut new = old.clone();
        new.meta.valid_from = "2026-07-01".parse().unwrap();
        for t in &mut new.trips {
            t.dates.retain(|d| *d >= new.meta.valid_from);
        }
        let dir = tempfile::tempdir().unwrap();
        let src = [
            Source {
                line: &old,
                url: URL.into(),
                sha256: "old".into(),
            },
            Source {
                line: &new,
                url: URL.into(),
                sha256: "new".into(),
            },
        ];
        write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        let trips = read(dir.path(), "trips.txt");
        let services: BTreeMap<String, String> = trips
            .iter()
            .map(|t| (t["service_id"].clone(), t["trip_id"].clone()))
            .collect();
        for d in read(dir.path(), "calendar_dates.txt") {
            let trip = &services[&d["service_id"]];
            if trip.contains("2026-04-01") {
                assert!(
                    d["date"].as_str() < "20260701",
                    "old version still runs on {}",
                    d["date"]
                );
            }
        }
    }

    #[test]
    fn provenance_points_back_into_the_pdf() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        write(dir.path(), &src, &everywhere, "2026-11-24".parse().unwrap()).unwrap();
        let p = read(dir.path(), "provenance.txt");
        assert_eq!(p.len(), read(dir.path(), "stop_times.txt").len());
        assert_eq!(p[0]["pdf_sha256"], "abc");
        assert!(p[0]["page"] == "1" || p[0]["page"] == "2");
    }

    #[test]
    fn trips_through_unlocated_stops_are_left_out_and_reported() {
        let l = line("401402.pdf");
        let dir = tempfile::tempdir().unwrap();
        let src = [Source {
            line: &l,
            url: URL.into(),
            sha256: "abc".into(),
        }];
        let no_dulov = |name: &str| {
            if name.contains("Ďulov") {
                None
            } else {
                everywhere(name)
            }
        };
        let r = write(dir.path(), &src, &no_dulov, "2026-11-24".parse().unwrap()).unwrap();
        assert_eq!(r.stops_unlocated, vec!["Komárno, Ďulov dvor".to_string()]);
        assert!(r.trips_without_location > 0);
        assert_eq!(r.trips + r.trips_without_location, 39);
    }
}
