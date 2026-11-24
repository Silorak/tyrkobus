//! Stage E2: do the stop positions agree with the timetable? A bus cannot
//! cover 4 km in one minute, so a stop that is far from most of the stops
//! the timetable puts next to it was matched to the wrong OSM stop. Such
//! stops are demoted to approximate instead of being shown confidently in
//! the wrong place.

use crate::gtfs::Location;
use std::collections::{BTreeMap, BTreeSet};

/// Consecutive calls of a trip: from, to, minutes between them.
pub type Hop = (String, String, u32);

/// Faster than this between two stops (with a minute of rounding allowed) is impossible for a bus.
const MAX_KMH: f64 = 90.0;
/// Hops shorter than this are never suspicious: stop matching is not that precise.
const MIN_KM: f64 = 2.0;

pub fn km(a: &Location, b: &Location) -> f64 {
    let dy = (a.lat - b.lat) * 111.0;
    let dx = (a.lon - b.lon) * 111.0 * a.lat.to_radians().cos();
    (dx * dx + dy * dy).sqrt()
}

fn impossible(a: &Location, b: &Location, minutes: u32) -> bool {
    let d = km(a, b);
    d > MIN_KM && d / f64::from(minutes + 1) * 60.0 > MAX_KMH
}

/// Stops whose exact position contradicts most of their timetable neighbours.
pub fn implausible(hops: &[Hop], at: &BTreeMap<String, Location>) -> BTreeSet<String> {
    // Most generous time seen for each pair of stops (trips differ).
    let mut minutes: BTreeMap<(&str, &str), u32> = BTreeMap::new();
    for (a, b, m) in hops {
        if a == b {
            continue;
        }
        let key = if a < b {
            (a.as_str(), b.as_str())
        } else {
            (b.as_str(), a.as_str())
        };
        let e = minutes.entry(key).or_insert(*m);
        *e = (*e).max(*m);
    }
    let exact = |n: &str| at.get(n).filter(|l| !l.approximate);
    let mut doubted: BTreeSet<String> = BTreeSet::new();
    loop {
        // stop → (neighbours with a trusted position, of which contradict it)
        let mut tally: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
        for (&(a, b), &m) in &minutes {
            if doubted.contains(a) || doubted.contains(b) {
                continue;
            }
            let (Some(la), Some(lb)) = (exact(a), exact(b)) else {
                continue;
            };
            let bad = u32::from(impossible(la, lb, m));
            for s in [a, b] {
                let t = tally.entry(s).or_default();
                t.0 += 1;
                t.1 += bad;
            }
        }
        // A wrong stop disagrees with most neighbours; its neighbours with only it.
        let new: Vec<String> = tally
            .iter()
            .filter(|(_, (n, bad))| 2 * bad > *n)
            .map(|(s, _)| s.to_string())
            .collect();
        if new.is_empty() {
            return doubted;
        }
        doubted.extend(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(places: &[(&str, f64, f64)]) -> BTreeMap<String, Location> {
        places
            .iter()
            .map(|(n, lat, lon)| {
                (
                    n.to_string(),
                    Location {
                        lat: *lat,
                        lon: *lon,
                        approximate: false,
                    },
                )
            })
            .collect()
    }

    fn hop(a: &str, b: &str, m: u32) -> Hop {
        (a.into(), b.into(), m)
    }

    // A road going east: ~0.75 km per 0.01° of longitude at 47.9° N.
    fn road() -> BTreeMap<String, Location> {
        at(&[
            ("A", 47.9, 18.00),
            ("B", 47.9, 18.02),
            ("C", 47.9, 18.04),
            ("D", 47.9, 18.06),
            ("E", 47.9, 18.08),
        ])
    }

    #[test]
    fn a_consistent_route_is_fine() {
        let hops = [
            hop("A", "B", 2),
            hop("B", "C", 2),
            hop("C", "D", 2),
            hop("D", "E", 2),
        ];
        assert!(implausible(&hops, &road()).is_empty());
    }

    #[test]
    fn a_stop_matched_to_the_wrong_place_is_found_and_its_neighbours_are_not_blamed() {
        let mut at = road();
        at.get_mut("C").unwrap().lat = 47.96; // 6.7 km north of the road
        let hops = [
            hop("A", "B", 2),
            hop("B", "C", 1),
            hop("C", "D", 1),
            hop("D", "E", 2),
        ];
        assert_eq!(implausible(&hops, &at), BTreeSet::from(["C".to_string()]));
    }

    #[test]
    fn long_hops_with_enough_time_are_fine() {
        let at = at(&[("A", 47.9, 18.0), ("Z", 47.9, 18.2)]); // 15 km
        assert!(implausible(&[hop("A", "Z", 14)], &at).is_empty());
    }

    #[test]
    fn approximate_stops_are_neither_judged_nor_used_as_evidence() {
        let mut at = road();
        at.get_mut("C").unwrap().lat = 47.96;
        at.get_mut("C").unwrap().approximate = true;
        let hops = [hop("B", "C", 1), hop("C", "D", 1)];
        assert!(implausible(&hops, &at).is_empty());
    }

    #[test]
    fn two_stops_contradicting_only_each_other_are_both_doubted() {
        // No other evidence: either could be wrong, so neither is trusted.
        let at = at(&[("P", 47.9, 18.0), ("Q", 47.9, 18.06)]);
        assert_eq!(implausible(&[hop("P", "Q", 1)], &at).len(), 2);
    }
}
