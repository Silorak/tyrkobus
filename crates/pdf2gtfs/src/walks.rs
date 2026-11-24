//! Walks between nearby stops, measured along footpaths and streets.
//!
//! Written next to the timetable as GTFS `transfers.txt` (walking time,
//! both directions) and `walks.txt` (not GTFS: the path, for drawing).

use crate::roads::Router;

/// Straight-line distance within which two stops may be a walk apart.
pub const NEAR_M: f64 = 500.0;
/// Longest walk offered, measured along the way.
pub const MAX_M: f64 = 800.0;
/// 4.5 km/h.
pub const METRES_PER_MINUTE: f64 = 75.0;

#[derive(Debug, Clone, PartialEq)]
pub struct StopAt {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    /// Position known, not guessed: only then is walking offered.
    pub exact: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Walk {
    pub from: String,
    pub to: String,
    pub metres: f64,
    /// From the `from` stop to the `to` stop.
    pub points: Vec<(f64, f64)>,
}

impl Walk {
    pub fn minutes(&self) -> u32 {
        (self.metres / METRES_PER_MINUTE).ceil().max(1.0) as u32
    }
}

/// Walks between every two stops with known positions that are near each
/// other and not too far apart along the footpaths and streets.
pub fn walks(stops: &[StopAt], router: &Router) -> Vec<Walk> {
    use crate::roads::metres;
    let exact: Vec<&StopAt> = stops.iter().filter(|s| s.exact).collect();
    // Where each stop meets the walkable network (a stop is beside a street).
    let snaps: Vec<_> = exact
        .iter()
        .map(|s| router.snap((s.lat, s.lon), 100.0))
        .collect();
    let mut out = vec![];
    for i in 0..exact.len() {
        for j in i + 1..exact.len() {
            let (a, b) = (exact[i], exact[j]);
            let (pa, pb) = ((a.lat, a.lon), (b.lat, b.lon));
            let straight = metres(pa, pb);
            if straight > NEAR_M {
                continue;
            }
            let points = match (&snaps[i], &snaps[j]) {
                _ if straight < 25.0 => vec![pa, pb],
                (Some(s), Some(t)) => match router.between(s, t) {
                    Some((_, mut way)) => {
                        way.insert(0, pa);
                        way.push(pb);
                        way.dedup();
                        way
                    }
                    None => continue,
                },
                // Off the mapped network: the straight line, as before.
                _ => vec![pa, pb],
            };
            let mut length: f64 = points.windows(2).map(|w| metres(w[0], w[1])).sum();
            if points.len() == 2 && straight >= 25.0 {
                length *= 1.25;
            }
            if length > MAX_M {
                continue;
            }
            out.push(Walk {
                from: a.id.clone(),
                to: b.id.clone(),
                metres: length,
                points,
            });
        }
    }
    out
}

/// `transfers.txt` (walking time, both directions) and `walks.txt` (paths).
pub fn write(dir: &std::path::Path, walks: &[Walk]) -> std::io::Result<()> {
    let err = std::io::Error::other;
    let mut t = csv::Writer::from_path(dir.join("transfers.txt")).map_err(err)?;
    t.write_record([
        "from_stop_id",
        "to_stop_id",
        "transfer_type",
        "min_transfer_time",
    ])
    .map_err(err)?;
    let mut p = csv::Writer::from_path(dir.join("walks.txt")).map_err(err)?;
    p.write_record(["from_stop_id", "to_stop_id", "length_m", "points"])
        .map_err(err)?;
    for w in walks {
        let secs = (w.minutes() * 60).to_string();
        t.write_record([&w.from, &w.to, "2", &secs]).map_err(err)?;
        t.write_record([&w.to, &w.from, "2", &secs]).map_err(err)?;
        let points: Vec<String> = w
            .points
            .iter()
            .map(|(a, b)| format!("{a:.6} {b:.6}"))
            .collect();
        p.write_record([
            &w.from,
            &w.to,
            &format!("{:.0}", w.metres),
            &points.join(";"),
        ])
        .map_err(err)?;
    }
    t.flush()?;
    p.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roads::{Class, Edge, Roads};

    // A street 0-1-2 east-west; a river north of it; stop C across the
    // river is close in a straight line but the bridge is far away.
    fn place() -> Roads {
        Roads {
            nodes: vec![
                (48.0, 18.0),
                (48.0, 18.003),
                (48.0, 18.006),
                (48.003, 18.006),
                (48.003, 18.0),
            ],
            edges: vec![
                Edge {
                    a: 0,
                    b: 1,
                    class: Class::Residential,
                    oneway: true,
                },
                Edge {
                    a: 1,
                    b: 2,
                    class: Class::Residential,
                    oneway: true,
                },
                Edge {
                    a: 2,
                    b: 3,
                    class: Class::Foot,
                    oneway: false,
                }, // the bridge, 330 m
                Edge {
                    a: 3,
                    b: 4,
                    class: Class::Foot,
                    oneway: false,
                },
            ],
        }
    }

    fn at(id: &str, lat: f64, lon: f64) -> StopAt {
        StopAt {
            id: id.into(),
            lat,
            lon,
            exact: true,
        }
    }

    #[test]
    fn a_walk_follows_the_street_both_ways_with_its_length() {
        let r = Router::walking(&place());
        // A at node 0 (10 m beside the street), B at node 1: 223 m apart.
        let w = walks(&[at("A", 48.00009, 18.0), at("B", 48.0, 18.003)], &r);
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!((w[0].from.as_str(), w[0].to.as_str()), ("A", "B"));
        assert!((w[0].metres - 233.0).abs() < 5.0, "{}", w[0].metres);
        assert_eq!(w[0].points.first(), Some(&(48.00009, 18.0)));
        assert_eq!(w[0].points.last(), Some(&(48.0, 18.003)));
        assert_eq!(w[0].minutes(), 4);
    }

    #[test]
    fn stops_close_across_a_river_far_by_the_bridge_are_no_walk() {
        let r = Router::walking(&place());
        // A at node 0, C at node 4: 333 m straight, ~1.5 km via the bridge.
        let w = walks(&[at("A", 48.0, 18.0), at("C", 48.003, 18.0)], &r);
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn far_or_guessed_stops_are_not_paired() {
        let r = Router::walking(&place());
        let mut guess = at("B", 48.0, 18.003);
        guess.exact = false;
        assert!(walks(&[at("A", 48.0, 18.0), guess], &r).is_empty());
        assert!(walks(&[at("A", 48.0, 18.0), at("D", 48.0, 18.01)], &r).is_empty());
    }

    #[test]
    fn platforms_on_one_spot_are_a_one_minute_walk() {
        let r = Router::walking(&place());
        let w = walks(&[at("A", 48.0, 18.0), at("A2", 48.0, 18.0)], &r);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].minutes(), 1);
    }
}
