//! The GTFS feed in memory and the questions the UI asks it.

use chrono::{Days, NaiveDate, NaiveDateTime};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Stop {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub approximate: bool,
}

#[derive(Debug, Clone)]
pub struct Route {
    pub id: String,
    pub short_name: String,
    pub long_name: String,
}

#[derive(Debug, Clone)]
pub struct StopTime {
    pub stop: usize,
    pub arrival: u32,
    pub departure: u32,
    pub pickup: u8,
    pub drop_off: u8,
    pub page: u32,
    /// Metres along the trip's shape (GTFS shape_dist_traveled).
    pub dist: Option<f64>,
}

type ShapePoint = (u32, (f64, f64), f64);

/// A trip's path along the roads: points and metres along it at each point.
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub points: Vec<(f64, f64)>,
    pub along: Vec<f64>,
}

impl Shape {
    /// The point `d` metres along the shape.
    pub fn at(&self, d: f64) -> (f64, f64) {
        let i = self.along.partition_point(|&a| a < d);
        if i == 0 {
            return self.points[0];
        }
        if i >= self.points.len() {
            return self.points[self.points.len() - 1];
        }
        let (a, b) = (self.along[i - 1], self.along[i]);
        let f = if b > a { (d - a) / (b - a) } else { 0.0 };
        let (p, q) = (self.points[i - 1], self.points[i]);
        (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f)
    }
}

#[derive(Debug, Clone)]
pub struct Trip {
    pub id: String,
    pub route: usize,
    pub service: usize,
    pub headsign: String,
    pub number: String,
    pub direction: u8,
    pub source_url: String,
    pub stop_times: Vec<StopTime>,
    /// Index into `Feed::shapes`.
    pub shape: Option<usize>,
}

#[derive(Debug, Default)]
pub struct Feed {
    pub stops: Vec<Stop>,
    pub routes: Vec<Route>,
    pub trips: Vec<Trip>,
    /// service index → dates it runs
    pub services: Vec<BTreeSet<NaiveDate>>,
    pub first_date: Option<NaiveDate>,
    pub last_date: Option<NaiveDate>,
    pub version: String,
    stop_index: HashMap<String, usize>,
    route_index: HashMap<String, usize>,
    /// stop → (trip, position in trip)
    calls: Vec<Vec<(usize, usize)>>,
    /// The same stops and trips (same indexes) for journey planning.
    pub network: planner::Network,
    /// stop → every stop of the same place (itself included)
    stations: Vec<Vec<usize>>,
    /// (stop, stop) → walking path between them, along the footpaths
    walk_paths: HashMap<(usize, usize), Vec<(f64, f64)>>,
    pub shapes: Vec<Shape>,
}

/// One bus leaving a stop at a moment in time.
#[derive(Debug, Clone, PartialEq)]
pub struct Departure {
    pub trip: usize,
    /// Service day the trip belongs to (times can pass 24:00).
    pub service_date: NaiveDate,
    /// Wall-clock departure.
    pub at: NaiveDateTime,
    pub request_stop: bool,
}

type Row = BTreeMap<String, String>;

fn rows(dir: &Path, file: &str) -> io::Result<Vec<Row>> {
    let mut r = csv::Reader::from_path(dir.join(file)).map_err(io::Error::other)?;
    r.deserialize()
        .map(|x| x.map_err(io::Error::other))
        .collect()
}

fn minutes(hms: &str) -> u32 {
    let mut p = hms.split(':').map(|x| x.parse::<u32>().unwrap_or(0));
    p.next().unwrap_or(0) * 60 + p.next().unwrap_or(0)
}

fn words(s: &str) -> Vec<String> {
    pdf2gtfs::gtfs::slug(s)
        .split('-')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Stops that are one place for a rider: platforms of a bus station
/// ("AS", "AS-ŽST, nást.č.28", "Autobusová stanica") and town-bus
/// variants ("ŽST MHD") share a key.
pub fn station_key(name: &str) -> String {
    let (town, rest) = name.split_once(',').unwrap_or((name, ""));
    let rest = pdf2gtfs::stops::normalize(rest);
    let mut words: Vec<&str> = vec![];
    let mut tokens = rest.split(' ').filter(|w| !w.is_empty()).peekable();
    while let Some(w) = tokens.next() {
        match w {
            "mhd" | "ul" | "ulica" => {}
            // "nást.č.28": a platform number.
            "nast" => {
                tokens.next_if(|n| n.starts_with(|c: char| c.is_ascii_digit()));
            }
            _ => words.push(w),
        }
    }
    let town = pdf2gtfs::stops::normalize(town);
    if words.contains(&"as") {
        format!("{town}|as")
    } else {
        format!("{town}|{}", words.join(" "))
    }
}

/// Line numbers in number order: town lines 1, 4, 10 before regional 401412.
pub fn line_order(a: &str, b: &str) -> std::cmp::Ordering {
    let num = |s: &str| s.parse::<u64>().ok();
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    }
}

/// Minutes to change between platforms of one station.
pub const PLATFORM_CHANGE: u32 = 3;

impl Feed {
    pub fn load(dir: &Path) -> io::Result<Feed> {
        let mut f = Feed::default();
        for r in rows(dir, "stops.txt")? {
            f.stop_index.insert(r["stop_id"].clone(), f.stops.len());
            f.stops.push(Stop {
                id: r["stop_id"].clone(),
                name: r["stop_name"].clone(),
                lat: r["stop_lat"].parse().unwrap_or(0.0),
                lon: r["stop_lon"].parse().unwrap_or(0.0),
                approximate: r.get("stop_desc").is_some_and(|d| !d.is_empty()),
            });
        }
        for r in rows(dir, "routes.txt")? {
            f.route_index.insert(r["route_id"].clone(), f.routes.len());
            f.routes.push(Route {
                id: r["route_id"].clone(),
                short_name: r["route_short_name"].clone(),
                long_name: r["route_long_name"].clone(),
            });
        }
        let mut service_index: HashMap<String, usize> = HashMap::new();
        for r in rows(dir, "calendar_dates.txt")? {
            let Ok(date) = NaiveDate::parse_from_str(&r["date"], "%Y%m%d") else {
                continue;
            };
            let next = service_index.len();
            let i = *service_index.entry(r["service_id"].clone()).or_insert(next);
            if i == f.services.len() {
                f.services.push(BTreeSet::new());
            }
            f.services[i].insert(date);
        }
        // Road shapes are optional: without them trips are drawn stop to stop.
        let mut shape_index: HashMap<String, usize> = HashMap::new();
        // shape → (sequence, point, metres along)
        let mut points: Vec<Vec<ShapePoint>> = vec![];
        for r in rows(dir, "shapes.txt").unwrap_or_default() {
            let next = shape_index.len();
            let i = *shape_index.entry(r["shape_id"].clone()).or_insert(next);
            if i == points.len() {
                points.push(vec![]);
            }
            points[i].push((
                r["shape_pt_sequence"].parse().unwrap_or(0),
                (
                    r["shape_pt_lat"].parse().unwrap_or(0.0),
                    r["shape_pt_lon"].parse().unwrap_or(0.0),
                ),
                r["shape_dist_traveled"].parse().unwrap_or(0.0),
            ));
        }
        f.shapes = points
            .into_iter()
            .map(|mut p| {
                p.sort_by_key(|x| x.0);
                Shape {
                    points: p.iter().map(|x| x.1).collect(),
                    along: p.iter().map(|x| x.2).collect(),
                }
            })
            .collect();
        let mut trip_index: HashMap<String, usize> = HashMap::new();
        for r in rows(dir, "trips.txt")? {
            let (Some(&route), Some(&service)) = (
                f.route_index.get(&r["route_id"]),
                service_index.get(&r["service_id"]),
            ) else {
                continue;
            };
            trip_index.insert(r["trip_id"].clone(), f.trips.len());
            f.trips.push(Trip {
                id: r["trip_id"].clone(),
                route,
                service,
                headsign: r["trip_headsign"].clone(),
                number: r["trip_short_name"].clone(),
                direction: r["direction_id"].parse().unwrap_or(0),
                source_url: String::new(),
                stop_times: vec![],
                shape: r
                    .get("shape_id")
                    .and_then(|id| shape_index.get(id))
                    .copied(),
            });
        }
        let mut seq: Vec<Vec<(u32, StopTime)>> = vec![vec![]; f.trips.len()];
        for r in rows(dir, "stop_times.txt")? {
            let (Some(&t), Some(&s)) = (
                trip_index.get(&r["trip_id"]),
                f.stop_index.get(&r["stop_id"]),
            ) else {
                continue;
            };
            seq[t].push((
                r["stop_sequence"].parse().unwrap_or(0),
                StopTime {
                    stop: s,
                    arrival: minutes(&r["arrival_time"]),
                    departure: minutes(&r["departure_time"]),
                    pickup: r["pickup_type"].parse().unwrap_or(0),
                    drop_off: r["drop_off_type"].parse().unwrap_or(0),
                    page: 0,
                    dist: r.get("shape_dist_traveled").and_then(|d| d.parse().ok()),
                },
            ));
        }
        for (t, mut st) in seq.into_iter().enumerate() {
            st.sort_by_key(|(n, _)| *n);
            f.trips[t].stop_times = st.into_iter().map(|(_, s)| s).collect();
        }
        if let Ok(prov) = rows(dir, "provenance.txt") {
            for r in prov {
                let Some(&t) = trip_index.get(&r["trip_id"]) else {
                    continue;
                };
                let i: usize = r["stop_sequence"].parse().unwrap_or(1);
                if let Some(st) = f.trips[t].stop_times.get_mut(i.saturating_sub(1)) {
                    st.page = r["page"].parse().unwrap_or(0);
                }
                f.trips[t].source_url = r["source_url"].clone();
            }
        }
        if let Some(info) = rows(dir, "feed_info.txt")?.into_iter().next() {
            f.version = info.get("feed_version").cloned().unwrap_or_default();
            let d = |k: &str| {
                info.get(k)
                    .and_then(|v| NaiveDate::parse_from_str(v, "%Y%m%d").ok())
            };
            f.first_date = d("feed_start_date");
            f.last_date = d("feed_end_date");
        }
        f.calls = vec![vec![]; f.stops.len()];
        for (t, trip) in f.trips.iter().enumerate() {
            for (i, st) in trip.stop_times.iter().enumerate() {
                f.calls[st.stop].push((t, i));
            }
        }
        f.network = planner::Network::new(
            f.stops
                .iter()
                .map(|s| planner::Stop {
                    lat: s.lat,
                    lon: s.lon,
                    exact: !s.approximate,
                })
                .collect(),
            f.trips
                .iter()
                .map(|t| planner::Trip {
                    service: t.service,
                    calls: t
                        .stop_times
                        .iter()
                        .map(|st| planner::Call {
                            stop: st.stop,
                            arrival: st.arrival,
                            departure: st.departure,
                            board: st.pickup != 1,
                            alight: st.drop_off != 1,
                        })
                        .collect(),
                })
                .collect(),
            f.services.clone(),
        );
        // Walks measured along the footpaths, when the build had the map.
        if let Ok(transfers) = rows(dir, "transfers.txt") {
            let walks: Vec<(usize, usize, u32)> = transfers
                .iter()
                .filter_map(|r| {
                    let a = f.stop(r.get("from_stop_id")?)?;
                    let b = f.stop(r.get("to_stop_id")?)?;
                    let secs: u32 = r.get("min_transfer_time")?.parse().ok()?;
                    Some((a, b, secs.div_ceil(60).max(1)))
                })
                .collect();
            f.network.set_walks(walks);
        }
        if let Ok(paths) = rows(dir, "walks.txt") {
            for r in paths {
                let (Some(a), Some(b)) = (
                    r.get("from_stop_id").and_then(|s| f.stop(s)),
                    r.get("to_stop_id").and_then(|s| f.stop(s)),
                ) else {
                    continue;
                };
                let points: Option<Vec<(f64, f64)>> = r
                    .get("points")
                    .map(String::as_str)
                    .unwrap_or("")
                    .split(';')
                    .map(|p| {
                        let (lat, lon) = p.split_once(' ')?;
                        Some((lat.parse().ok()?, lon.parse().ok()?))
                    })
                    .collect();
                if let Some(points) = points.filter(|p| p.len() >= 2) {
                    f.walk_paths.insert((a, b), points);
                }
            }
        }
        let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, s) in f.stops.iter().enumerate() {
            by_key.entry(station_key(&s.name)).or_default().push(i);
        }
        f.stations = (0..f.stops.len()).map(|i| vec![i]).collect();
        for group in by_key.values().filter(|g| g.len() > 1) {
            for &a in group {
                f.stations[a] = group.clone();
                for &b in group {
                    f.network.link(a, b, PLATFORM_CHANGE);
                }
            }
        }
        Ok(f)
    }

    /// The way on foot from stop `a` to stop `b`, when it was measured.
    pub fn walk_path(&self, a: usize, b: usize) -> Option<Vec<(f64, f64)>> {
        if let Some(p) = self.walk_paths.get(&(a, b)) {
            return Some(p.clone());
        }
        let mut p = self.walk_paths.get(&(b, a))?.clone();
        p.reverse();
        Some(p)
    }

    /// What a bus drives from call `a` to call `b` of `trip`: its road shape
    /// between those stops, or straight lines between them without one.
    pub fn path(&self, trip: usize, a: usize, b: usize) -> Vec<(f64, f64)> {
        let t = &self.trips[trip];
        let (da, db) = (t.stop_times[a].dist, t.stop_times[b].dist);
        if let (Some(sh), Some(da), Some(db)) = (t.shape.map(|i| &self.shapes[i]), da, db) {
            let mut out = vec![sh.at(da)];
            for (p, d) in sh.points.iter().zip(&sh.along) {
                if *d > da && *d < db {
                    out.push(*p);
                }
            }
            out.push(sh.at(db));
            return out;
        }
        t.stop_times[a..=b]
            .iter()
            .map(|st| (self.stops[st.stop].lat, self.stops[st.stop].lon))
            .collect()
    }

    /// Where to draw call `k` of `trip`, and whether that place is a guess:
    /// where the bus stops on its road (the platform is a few metres beside
    /// it), so the circle sits on the drawn line. Without a shape, the stop.
    pub fn call_position(&self, trip: usize, k: usize) -> (f64, f64, bool) {
        let t = &self.trips[trip];
        let st = &t.stop_times[k];
        let stop = &self.stops[st.stop];
        match (t.shape, st.dist) {
            (Some(sh), Some(d)) => {
                let (lat, lon) = self.shapes[sh].at(d);
                (lat, lon, stop.approximate)
            }
            _ => (stop.lat, stop.lon, stop.approximate),
        }
    }

    /// Every stop of the place `stop` belongs to (platforms of one station).
    pub fn station(&self, stop: usize) -> &[usize] {
        &self.stations[stop]
    }

    pub fn stop(&self, id: &str) -> Option<usize> {
        self.stop_index.get(id).copied()
    }

    pub fn route(&self, id: &str) -> Option<usize> {
        self.route_index.get(id).copied()
    }

    /// The next `n` departures from `stop` at or after `now` (local time),
    /// looking up to two days ahead. A trip's last stop is not a departure.
    pub fn departures(&self, stop: usize, now: NaiveDateTime, n: usize) -> Vec<Departure> {
        let mut out = vec![];
        let today = now.date();
        for offset in [-1i64, 0, 1, 2] {
            let service_date = if offset < 0 {
                today - Days::new(1)
            } else {
                today + Days::new(offset as u64)
            };
            let midnight = service_date.and_hms_opt(0, 0, 0).unwrap();
            for &(t, i) in &self.calls[stop] {
                let trip = &self.trips[t];
                let st = &trip.stop_times[i];
                if i + 1 == trip.stop_times.len()
                    || st.pickup == 1
                    || !self.runs_on(t, service_date)
                {
                    continue;
                }
                let at = midnight + chrono::Duration::minutes(i64::from(st.departure));
                if at >= now {
                    out.push(Departure {
                        trip: t,
                        service_date,
                        at,
                        request_stop: st.pickup == 3,
                    });
                }
            }
        }
        out.sort_by(|a, b| {
            a.at.cmp(&b.at)
                .then(self.trips[a.trip].id.cmp(&self.trips[b.trip].id))
        });
        out.truncate(n);
        out
    }

    /// Trips of `route` running on `date`, by direction, in departure order.
    pub fn timetable(&self, route: usize, date: NaiveDate) -> BTreeMap<u8, Vec<usize>> {
        let mut out: BTreeMap<u8, Vec<usize>> = BTreeMap::new();
        for (t, trip) in self.trips.iter().enumerate() {
            if trip.route == route && self.runs_on(t, date) {
                out.entry(trip.direction).or_default().push(t);
            }
        }
        for trips in out.values_mut() {
            trips.sort_by_key(|&t| self.trips[t].stop_times[0].departure);
        }
        out
    }

    /// Stops whose name contains every word of `query` (diacritics ignored),
    /// busiest first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<usize> {
        let want = words(query);
        if want.is_empty() {
            return vec![];
        }
        let mut hits: Vec<usize> = (0..self.stops.len())
            .filter(|&s| {
                let have = words(&self.stops[s].name);
                want.iter()
                    .all(|w| have.iter().any(|h| h.starts_with(w.as_str())))
            })
            .collect();
        hits.sort_by_key(|&s| std::cmp::Reverse(self.calls[s].len()));
        hits.truncate(limit);
        hits
    }

    /// Stops with the most departures: good starting points.
    pub fn busiest(&self, limit: usize) -> Vec<usize> {
        let mut all: Vec<usize> = (0..self.stops.len()).collect();
        all.sort_by_key(|&s| std::cmp::Reverse(self.calls[s].len()));
        all.truncate(limit);
        all
    }

    /// Routes calling at `stop`, by short name.
    pub fn routes_at(&self, stop: usize) -> Vec<usize> {
        let set: BTreeSet<usize> = self.calls[stop]
            .iter()
            .map(|&(t, _)| self.trips[t].route)
            .collect();
        let mut v: Vec<usize> = set.into_iter().collect();
        v.sort_by(|a, b| line_order(&self.routes[*a].short_name, &self.routes[*b].short_name));
        v
    }

    pub fn runs_on(&self, trip: usize, date: NaiveDate) -> bool {
        self.services[self.trips[trip].service].contains(&date)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static FEED: OnceLock<(tempfile::TempDir, Feed)> = OnceLock::new();

    fn fixture_dir() -> &'static std::path::Path {
        feed();
        FEED.get().unwrap().0.path()
    }

    /// A feed built from tests/fixtures/401402.pdf of the pdf2gtfs crate
    /// (valid 1 Apr – 12 Dec 2026).
    pub(crate) fn feed() -> &'static Feed {
        &FEED
            .get_or_init(|| {
                let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../pdf2gtfs/tests/fixtures/401402.pdf");
                let line = pdf2gtfs::line::build(&pdf2gtfs::words::extract(&pdf).unwrap()).unwrap();
                let dir = tempfile::tempdir().unwrap();
                let src = [pdf2gtfs::gtfs::Source {
                    line: &line,
                    url: "https://arriva.sk/files/nz/cp/primesto/komarno/2026/401402.pdf".into(),
                    sha256: "test".into(),
                }];
                let here = |_: &str| {
                    Some(pdf2gtfs::gtfs::Location {
                        lat: 47.87,
                        lon: 18.19,
                        approximate: true,
                    })
                };
                // Every trip drives a straight road north from 47.87, stops 100 m apart.
                let road = |names: &[String]| {
                    Some(std::sync::Arc::new(pdf2gtfs::gtfs::ShapeRef {
                        id: format!("sh{}", names.len()),
                        points: vec![(47.87, 18.19), (48.0, 18.19)],
                        dist: (0..names.len()).map(|i| i as f64 * 100.0).collect(),
                    }))
                };
                pdf2gtfs::gtfs::write_with_shapes(
                    dir.path(),
                    &src,
                    &here,
                    &road,
                    "2025-12-01".parse().unwrap(),
                )
                .unwrap();
                let f = Feed::load(dir.path()).unwrap();
                (dir, f)
            })
            .1
    }

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn hm(d: &Departure) -> String {
        d.at.format("%H:%M").to_string()
    }

    #[test]
    fn loads_the_whole_feed() {
        let f = feed();
        assert_eq!(f.routes.len(), 1);
        assert_eq!(f.trips.len(), 39); // rendered PDF: 18 + 21 trips
        assert!(f.stop("kravany-n-dunajom-jednota").is_some());
        assert_eq!(f.version, "2025-12-01");
    }

    #[test]
    fn next_departure_on_a_working_day_morning() {
        // Monday 20 Apr 2026, 6:00 at Kravany n. Dunajom, Jednota (rendered PDF, page 2):
        // trip 10 at 6:10 is next; trip 42, also 6:10, runs only 1.7.–31.8. (note 12);
        // then trip 12 at 6:25.
        let f = feed();
        let d = f.departures(
            f.stop("kravany-n-dunajom-jednota").unwrap(),
            at("2026-04-20 06:00"),
            3,
        );
        assert_eq!(hm(&d[0]), "06:10");
        assert_eq!(f.trips[d[0].trip].number, "10");
        assert_eq!(
            (hm(&d[1]).as_str(), f.trips[d[1].trip].number.as_str()),
            ("06:25", "12")
        );
        assert!(d.windows(2).all(|w| w[0].at <= w[1].at));
    }

    #[test]
    fn saturday_only_shows_saturday_trips() {
        // Saturday 25 Apr 2026: first bus at Kravany n. Dunajom, Jednota after 6:00 is
        // trip 18 ("⑥ †") at 9:15 (rendered PDF, page 2).
        let f = feed();
        let d = f.departures(
            f.stop("kravany-n-dunajom-jednota").unwrap(),
            at("2026-04-25 06:00"),
            1,
        );
        assert_eq!(
            (hm(&d[0]).as_str(), f.trips[d[0].trip].number.as_str()),
            ("09:15", "18")
        );
    }

    #[test]
    fn a_trip_does_not_depart_from_its_last_stop() {
        let f = feed();
        // Trips 1, 7, 11 … end at Kravany n. Dunajom, Jednota; trips 2, 10, 12 … start there.
        let terminus = f.stop("kravany-n-dunajom-jednota").unwrap();
        let d = f.departures(terminus, at("2026-04-20 04:00"), 30);
        assert!(!d.is_empty());
        assert!(
            d.iter()
                .all(|x| f.trips[x.trip].headsign != "Kravany n. Dunajom, Jednota")
        );
    }

    #[test]
    fn trips_after_midnight_belong_to_the_previous_service_day() {
        // Trip 39 (Monday service) reaches Kravany n. Dunajom, Tulipánová at 0:10 on
        // Tuesday (rendered PDF, page 1).
        let f = feed();
        let d = f.departures(
            f.stop("kravany-n-dunajom-tulipanova").unwrap(),
            at("2026-04-20 23:50"),
            1,
        );
        assert_eq!(d[0].at, at("2026-04-21 00:10"));
        assert_eq!(d[0].service_date, "2026-04-20".parse().unwrap());
        assert_eq!(f.trips[d[0].trip].number, "39");
    }

    #[test]
    fn plans_a_journey_on_the_real_line() {
        // Monday 20 Apr 2026, 6:00: from Kravany n. Dunajom, Jednota to Bátorove Kosihy,
        // hl.nám. the 6:10 bus (trip 10, there 6:35) is first, then 6:25 (12) and 7:40 (16)
        // (rendered PDF, page 2).
        let f = feed();
        let from = f.stop("kravany-n-dunajom-jednota").unwrap();
        let to = f.stop("batorove-kosihy-hl-nam").unwrap();
        let o = f.network.options(&[from], &[to], at("2026-04-20 06:00"), 3);
        assert_eq!(o.len(), 3);
        assert_eq!(o[0].depart(), at("2026-04-20 06:10"));
        let planner::Leg::Ride { trip, .. } = o[0].legs[0] else {
            panic!()
        };
        assert_eq!(f.trips[trip].number, "10");
        assert!(o.windows(2).all(|w| w[0].depart() < w[1].depart()));
    }

    #[test]
    fn walks_measured_along_the_streets_are_loaded_with_their_path() {
        let src = fixture_dir();
        let dir = tempfile::tempdir().unwrap();
        for e in std::fs::read_dir(src).unwrap() {
            let e = e.unwrap();
            std::fs::copy(e.path(), dir.path().join(e.file_name())).unwrap();
        }
        std::fs::write(
            dir.path().join("walks.txt"),
            "from_stop_id,to_stop_id,length_m,points\nkravany-n-dunajom-jednota,batorove-kosihy-hl-nam,300,47.87 18.19;47.871 18.191;47.872 18.19\n",
        )
        .unwrap();
        let f = Feed::load(dir.path()).unwrap();
        let (a, b) = (
            f.stop("kravany-n-dunajom-jednota").unwrap(),
            f.stop("batorove-kosihy-hl-nam").unwrap(),
        );
        let way = vec![(47.87, 18.19), (47.871, 18.191), (47.872, 18.19)];
        assert_eq!(f.walk_path(a, b), Some(way.clone()));
        let mut back = way;
        back.reverse();
        assert_eq!(f.walk_path(b, a), Some(back));
        assert_eq!(f.walk_path(a, a), None);
    }

    #[test]
    fn line_numbers_sort_like_numbers() {
        let mut v = vec!["10", "4", "401412", "1", "9", "40"];
        v.sort_by(|a, b| line_order(a, b));
        assert_eq!(v, ["1", "4", "9", "10", "40", "401412"]);
    }

    #[test]
    fn platforms_of_a_bus_station_are_one_place() {
        let k = station_key;
        assert_eq!(k("Nové Zámky, AS ŽST MHD"), k("Nové Zámky, AS"));
        assert_eq!(k("Nové Zámky, AS ŽST"), k("Nové Zámky, AS"));
        assert_eq!(k("Levice, AS-ŽST, nást.č.28"), k("Levice, AS"));
        assert_eq!(k("Levice, AS-ŽST, nást.č.30"), k("Levice, AS MHD"));
        assert_eq!(k("Šaľa, Autobusová stanica"), k("Šaľa, AS"));
        assert_eq!(k("Nové Zámky, Autobusové stanovište"), k("Nové Zámky, AS"));
        assert_eq!(k("Štúrovo, ŽST MHD"), k("Štúrovo, ŽST"));
    }

    #[test]
    fn different_places_stay_apart() {
        let k = station_key;
        assert_ne!(k("Štúrovo, ŽST"), k("Štúrovo, AS"));
        assert_ne!(k("Nové Zámky, Železničná stanica"), k("Nové Zámky, AS"));
        assert_ne!(
            k("Nové Zámky, rázc.k ŽST"),
            k("Nové Zámky, Železničná stanica")
        );
        assert_ne!(k("Komárno, AS"), k("Kolárovo, AS"));
        assert_ne!(k("Levice, Tabaková"), k("Levice, Tabaková 2"));
    }

    /// Metres north of 47.87 as latitude.
    fn north(m: f64) -> f64 {
        47.87 + m / 111_195.0
    }

    #[test]
    fn a_leg_follows_the_trip_s_road_shape() {
        let f = feed();
        let path = f.path(0, 1, 3);
        assert!((path[0].0 - north(100.0)).abs() < 1e-6, "{path:?}");
        assert!(
            (path.last().unwrap().0 - north(300.0)).abs() < 1e-6,
            "{path:?}"
        );
        assert!(path.iter().all(|p| (p.1 - 18.19).abs() < 1e-9));
    }

    #[test]
    fn a_guessed_stop_sits_on_the_drawn_line_not_at_the_village_centre() {
        let f = feed();
        let (lat, lon, approximate) = f.call_position(0, 2);
        assert!(approximate);
        assert!((lat - north(200.0)).abs() < 1e-6 && (lon - 18.19).abs() < 1e-9);
    }

    #[test]
    fn search_ignores_diacritics_and_case() {
        let f = feed();
        let hits: Vec<&str> = f
            .search("tulip", 5)
            .iter()
            .map(|&i| f.stops[i].name.as_str())
            .collect();
        assert_eq!(hits, ["Kravany n. Dunajom, Tulipánová"]);
        assert!(!f.search("srobarova", 5).is_empty());
    }

    #[test]
    fn timetable_for_a_day_splits_directions_in_time_order() {
        let f = feed();
        let t = f.timetable(f.route("401402").unwrap(), "2026-04-20".parse().unwrap());
        let first = |dir: u8| {
            t[&dir]
                .iter()
                .map(|&i| f.trips[i].stop_times[0].departure)
                .collect::<Vec<_>>()
        };
        assert!(first(0).windows(2).all(|w| w[0] <= w[1]));
        assert!(t[&1].iter().all(|&i| f.trips[i].direction == 1));
        assert!(
            t[&0]
                .iter()
                .all(|&i| f.runs_on(i, "2026-04-20".parse().unwrap()))
        );
    }
}
