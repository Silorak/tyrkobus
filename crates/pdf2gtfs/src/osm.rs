//! Bus stops and places from an OpenStreetMap extract (Geofabrik .osm.pbf),
//! reduced to a small JSON file so the 350 MB extract is read once.
//! Data © OpenStreetMap contributors, ODbL 1.0.

use osmpbf::{Element, ElementReader};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Extract {
    pub stops: Vec<Point>,
    pub places: Vec<Point>,
    /// Railway stations and halts ("Nové Zámky", "Komárno-Hadovce").
    #[serde(default)]
    pub stations: Vec<Point>,
    /// Bus stations (`amenity=bus_station`), areas as their centre; name may be empty.
    #[serde(default)]
    pub bus_stations: Vec<Point>,
}

/// South-west Slovakia: Komárno, Nové Zámky, Levice, Šaľa districts and surroundings.
pub const BBOX: [f64; 4] = [47.70, 17.40, 48.60, 19.20]; // min lat, min lon, max lat, max lon

fn is_stop(tags: &[(&str, &str)]) -> bool {
    let has = |k: &str, v: &str| tags.iter().any(|(a, b)| *a == k && *b == v);
    has("highway", "bus_stop")
        || (has("public_transport", "platform") && has("bus", "yes"))
        || (has("public_transport", "stop_position") && has("bus", "yes"))
}

pub fn is_bus_station(tags: &[(&str, &str)]) -> bool {
    let has = |k: &str, v: &str| tags.iter().any(|(a, b)| *a == k && *b == v);
    has("amenity", "bus_station") || (has("public_transport", "station") && has("bus", "yes"))
}

/// Centre of a closed or open way: the average of its distinct points.
pub fn centre(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let mut distinct: Vec<(f64, f64)> = points.to_vec();
    if distinct.len() > 1 && distinct.first() == distinct.last() {
        distinct.pop();
    }
    if distinct.is_empty() {
        return None;
    }
    let n = distinct.len() as f64;
    Some((
        distinct.iter().map(|p| p.0).sum::<f64>() / n,
        distinct.iter().map(|p| p.1).sum::<f64>() / n,
    ))
}

/// Bus stations mapped as areas (ways), placed at their centre.
pub fn bus_station_areas(pbf: &Path, bbox: [f64; 4]) -> io::Result<Vec<Point>> {
    let ways: Vec<(String, Vec<i64>)> = ElementReader::from_path(pbf)
        .map_err(io::Error::other)?
        .par_map_reduce(
            |element| match element {
                Element::Way(w) => {
                    let tags: Vec<(&str, &str)> = w.tags().collect();
                    if is_bus_station(&tags) {
                        let name = tags
                            .iter()
                            .find(|(k, _)| *k == "name")
                            .map_or("", |(_, v)| *v);
                        vec![(name.to_string(), w.refs().collect())]
                    } else {
                        vec![]
                    }
                }
                _ => vec![],
            },
            Vec::new,
            |mut a, b| {
                a.extend(b);
                a
            },
        )
        .map_err(io::Error::other)?;
    let mut wanted: Vec<i64> = ways.iter().flat_map(|w| w.1.iter().copied()).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut nodes: Vec<(i64, f64, f64)> = ElementReader::from_path(pbf)
        .map_err(io::Error::other)?
        .par_map_reduce(
            |element| {
                let (id, lat, lon) = match element {
                    Element::Node(n) => (n.id(), n.lat(), n.lon()),
                    Element::DenseNode(n) => (n.id(), n.lat(), n.lon()),
                    _ => return vec![],
                };
                if wanted.binary_search(&id).is_ok() {
                    vec![(id, lat, lon)]
                } else {
                    vec![]
                }
            },
            Vec::new,
            |mut a, b| {
                a.extend(b);
                a
            },
        )
        .map_err(io::Error::other)?;
    nodes.sort_unstable_by_key(|n| n.0);
    let inside =
        |lat: f64, lon: f64| lat >= bbox[0] && lat <= bbox[2] && lon >= bbox[1] && lon <= bbox[3];
    let mut out = vec![];
    for (name, refs) in ways {
        let pts: Vec<(f64, f64)> = refs
            .iter()
            .filter_map(|id| {
                nodes
                    .binary_search_by_key(id, |n| n.0)
                    .ok()
                    .map(|i| (nodes[i].1, nodes[i].2))
            })
            .collect();
        if let Some((lat, lon)) = centre(&pts)
            && inside(lat, lon)
        {
            out.push(Point { name, lat, lon });
        }
    }
    Ok(out)
}

fn is_railway_station(tags: &[(&str, &str)]) -> bool {
    tags.iter()
        .any(|(k, v)| *k == "railway" && matches!(*v, "station" | "halt"))
}

fn is_place(tags: &[(&str, &str)]) -> bool {
    tags.iter().any(|(k, v)| {
        *k == "place"
            && matches!(
                *v,
                "city" | "town" | "village" | "hamlet" | "suburb" | "neighbourhood" | "quarter"
            )
    })
}

pub fn extract(pbf: &Path, bbox: [f64; 4]) -> io::Result<Extract> {
    let reader = ElementReader::from_path(pbf).map_err(io::Error::other)?;
    let inside =
        |lat: f64, lon: f64| lat >= bbox[0] && lat <= bbox[2] && lon >= bbox[1] && lon <= bbox[3];
    let result = reader
        .par_map_reduce(
            |element| {
                let (tags, lat, lon): (Vec<(&str, &str)>, f64, f64) = match &element {
                    Element::Node(n) => (n.tags().collect(), n.lat(), n.lon()),
                    Element::DenseNode(n) => (n.tags().collect(), n.lat(), n.lon()),
                    _ => return Extract::default(),
                };
                let mut out = Extract::default();
                if !inside(lat, lon) {
                    return out;
                }
                let name = tags
                    .iter()
                    .find(|(k, _)| *k == "name")
                    .map(|(_, v)| v.to_string());
                // Bus stations are often unnamed; everything else needs a name to match.
                if is_bus_station(&tags) {
                    out.bus_stations.push(Point {
                        name: name.unwrap_or_default(),
                        lat,
                        lon,
                    });
                    return out;
                }
                let Some(name) = name else {
                    return out;
                };
                let p = Point { name, lat, lon };
                if is_stop(&tags) {
                    out.stops.push(p);
                } else if is_railway_station(&tags) {
                    out.stations.push(p);
                } else if is_place(&tags) {
                    out.places.push(p);
                }
                out
            },
            Extract::default,
            |mut a, b| {
                a.stops.extend(b.stops);
                a.places.extend(b.places);
                a.stations.extend(b.stations);
                a.bus_stations.extend(b.bus_stations);
                a
            },
        )
        .map_err(io::Error::other)?;
    let mut result = result;
    let key = |p: &Point| (p.name.clone(), (p.lat * 1e7) as i64, (p.lon * 1e7) as i64);
    result.stops.sort_by_key(key);
    result.places.sort_by_key(key);
    result.stations.sort_by_key(key);
    result.bus_stations.sort_by_key(key);
    Ok(result)
}

/// Direction a bus may drive a way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Both,
    Forward,
    Backward,
}

/// Whether a bus can drive this way, on what kind of road, which way.
pub fn drivable(tags: &[(&str, &str)]) -> Option<(crate::roads::Class, Direction)> {
    let get = |k: &str| tags.iter().find(|(a, _)| *a == k).map(|(_, v)| *v);
    let class = crate::roads::Class::from_highway(get("highway")?)?;
    if get("area") == Some("yes")
        || matches!(
            get("service"),
            Some("parking_aisle" | "driveway" | "drive-through")
        )
    {
        return None;
    }
    let buses_allowed = matches!(get("bus"), Some("yes" | "designated"))
        || matches!(get("psv"), Some("yes" | "designated"));
    if matches!(get("access"), Some("no" | "private")) && !buses_allowed {
        return None;
    }
    let contraflow = get("oneway:bus") == Some("no") || get("oneway:psv") == Some("no");
    let direction = match get("oneway") {
        _ if contraflow => Direction::Both,
        Some("yes" | "1" | "true") => Direction::Forward,
        Some("-1") => Direction::Backward,
        Some("no") => Direction::Both,
        _ if matches!(get("junction"), Some("roundabout" | "circular")) => Direction::Forward,
        _ => Direction::Both,
    };
    Some((class, direction))
}

/// Whether people can walk this way (always both ways), on what kind of way.
pub fn walkable(tags: &[(&str, &str)]) -> Option<(crate::roads::Class, Direction)> {
    use crate::roads::Class;
    let get = |k: &str| tags.iter().find(|(a, _)| *a == k).map(|(_, v)| *v);
    let foot = get("foot");
    let foot_ok = matches!(foot, Some("yes" | "designated" | "permissive"));
    if foot == Some("no") || (matches!(get("access"), Some("no" | "private")) && !foot_ok) {
        return None;
    }
    let class = match get("highway")? {
        "motorway" | "motorway_link" | "trunk" | "trunk_link" => return None,
        "footway" | "path" | "pedestrian" | "steps" | "track" | "bridleway" | "corridor" => {
            Class::Foot
        }
        "cycleway" if foot_ok => Class::Foot,
        h => Class::from_highway(h)?,
    };
    Some((class, Direction::Both))
}

/// Roads buses can drive within `bbox`. Reads the extract twice: ways (which
/// nodes are road nodes), then those nodes' positions.
pub fn roads(pbf: &Path, bbox: [f64; 4]) -> io::Result<crate::roads::Roads> {
    network(pbf, bbox, drivable)
}

/// Ways people can walk within `bbox`: footpaths and streets.
pub fn walkways(pbf: &Path, bbox: [f64; 4]) -> io::Result<crate::roads::Roads> {
    network(pbf, bbox, walkable)
}

type Select = fn(&[(&str, &str)]) -> Option<(crate::roads::Class, Direction)>;

fn network(pbf: &Path, bbox: [f64; 4], select: Select) -> io::Result<crate::roads::Roads> {
    use crate::roads::{Edge, Roads};
    let ways: Vec<(crate::roads::Class, Direction, Vec<i64>)> = ElementReader::from_path(pbf)
        .map_err(io::Error::other)?
        .par_map_reduce(
            |element| match element {
                Element::Way(w) => {
                    let tags: Vec<(&str, &str)> = w.tags().collect();
                    select(&tags)
                        .map(|(c, d)| vec![(c, d, w.refs().collect())])
                        .unwrap_or_default()
                }
                _ => vec![],
            },
            Vec::new,
            |mut a, b| {
                a.extend(b);
                a
            },
        )
        .map_err(io::Error::other)?;
    let mut wanted: Vec<i64> = ways.iter().flat_map(|w| w.2.iter().copied()).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let inside =
        |lat: f64, lon: f64| lat >= bbox[0] && lat <= bbox[2] && lon >= bbox[1] && lon <= bbox[3];
    let mut nodes: Vec<(i64, f32, f32)> = ElementReader::from_path(pbf)
        .map_err(io::Error::other)?
        .par_map_reduce(
            |element| {
                let (id, lat, lon) = match element {
                    Element::Node(n) => (n.id(), n.lat(), n.lon()),
                    Element::DenseNode(n) => (n.id(), n.lat(), n.lon()),
                    _ => return vec![],
                };
                if inside(lat, lon) && wanted.binary_search(&id).is_ok() {
                    vec![(id, lat as f32, lon as f32)]
                } else {
                    vec![]
                }
            },
            Vec::new,
            |mut a, b| {
                a.extend(b);
                a
            },
        )
        .map_err(io::Error::other)?;
    nodes.sort_unstable_by_key(|n| n.0);
    let index = |id: i64| {
        nodes
            .binary_search_by_key(&id, |n| n.0)
            .ok()
            .map(|i| i as u32)
    };
    let mut edges = vec![];
    for (class, direction, refs) in &ways {
        for pair in refs.windows(2) {
            let (Some(a), Some(b)) = (index(pair[0]), index(pair[1])) else {
                continue;
            };
            edges.push(match direction {
                Direction::Both => Edge {
                    a,
                    b,
                    class: *class,
                    oneway: false,
                },
                Direction::Forward => Edge {
                    a,
                    b,
                    class: *class,
                    oneway: true,
                },
                Direction::Backward => Edge {
                    a: b,
                    b: a,
                    class: *class,
                    oneway: true,
                },
            });
        }
    }
    Ok(Roads {
        nodes: nodes.iter().map(|n| (n.1, n.2)).collect(),
        edges,
    })
}

#[cfg(test)]
mod road_tests {
    use super::*;
    use crate::roads::Class;

    #[test]
    fn people_walk_on_paths_and_streets_but_not_motorways() {
        use crate::roads::Class;
        let w = |tags: &[(&str, &str)]| walkable(tags).map(|(c, _)| c);
        assert_eq!(w(&[("highway", "footway")]), Some(Class::Foot));
        assert_eq!(w(&[("highway", "steps")]), Some(Class::Foot));
        assert_eq!(w(&[("highway", "pedestrian")]), Some(Class::Foot));
        assert_eq!(w(&[("highway", "residential")]), Some(Class::Residential));
        // one-way for cars, both ways on foot
        assert!(matches!(
            walkable(&[("highway", "residential"), ("oneway", "yes")]),
            Some((_, Direction::Both))
        ));
        assert_eq!(w(&[("highway", "motorway")]), None);
        assert_eq!(w(&[("highway", "trunk")]), None);
        assert_eq!(w(&[("highway", "secondary"), ("foot", "no")]), None);
        assert_eq!(w(&[("highway", "service"), ("access", "private")]), None);
        assert_eq!(
            w(&[
                ("highway", "service"),
                ("access", "private"),
                ("foot", "yes")
            ]),
            Some(Class::Service)
        );
        assert_eq!(w(&[("highway", "cycleway")]), None);
        assert_eq!(
            w(&[("highway", "cycleway"), ("foot", "designated")]),
            Some(Class::Foot)
        );
    }

    #[test]
    fn main_roads_and_streets_are_drivable_paths_are_not() {
        assert_eq!(
            drivable(&[("highway", "primary")]),
            Some((Class::Main, Direction::Both))
        );
        assert_eq!(
            drivable(&[("highway", "residential")]),
            Some((Class::Residential, Direction::Both))
        );
        assert_eq!(drivable(&[("highway", "footway")]), None);
        assert_eq!(drivable(&[("highway", "cycleway")]), None);
        assert_eq!(drivable(&[("name", "Hlavná")]), None);
    }

    #[test]
    fn one_ways_and_roundabouts() {
        assert_eq!(
            drivable(&[("highway", "secondary"), ("oneway", "yes")])
                .unwrap()
                .1,
            Direction::Forward
        );
        assert_eq!(
            drivable(&[("highway", "secondary"), ("oneway", "-1")])
                .unwrap()
                .1,
            Direction::Backward
        );
        assert_eq!(
            drivable(&[("highway", "primary"), ("junction", "roundabout")])
                .unwrap()
                .1,
            Direction::Forward
        );
        // A contraflow lane for buses makes it two-way for us.
        assert_eq!(
            drivable(&[
                ("highway", "residential"),
                ("oneway", "yes"),
                ("oneway:bus", "no")
            ])
            .unwrap()
            .1,
            Direction::Both
        );
    }

    #[test]
    fn private_roads_only_when_buses_are_allowed() {
        assert_eq!(
            drivable(&[("highway", "service"), ("access", "private")]),
            None
        );
        assert!(drivable(&[("highway", "service"), ("access", "no"), ("bus", "yes")]).is_some());
        assert!(
            drivable(&[
                ("highway", "service"),
                ("access", "no"),
                ("psv", "designated")
            ])
            .is_some()
        );
        assert_eq!(
            drivable(&[("highway", "service"), ("service", "parking_aisle")]),
            None
        );
        assert_eq!(
            drivable(&[("highway", "residential"), ("area", "yes")]),
            None
        );
    }

    #[test]
    fn bus_stations_are_recognised() {
        assert!(is_bus_station(&[
            ("amenity", "bus_station"),
            ("name", "Autobusová stanica")
        ]));
        assert!(is_bus_station(&[
            ("public_transport", "station"),
            ("bus", "yes")
        ]));
        assert!(!is_bus_station(&[
            ("public_transport", "station"),
            ("train", "yes")
        ]));
        assert!(!is_bus_station(&[("highway", "bus_stop")]));
    }

    #[test]
    fn an_area_is_placed_at_its_centre() {
        // A closed square repeats its first point; that must not pull the centre.
        let sq = [
            (48.0, 18.0),
            (48.0, 18.002),
            (48.002, 18.002),
            (48.002, 18.0),
            (48.0, 18.0),
        ];
        let c = centre(&sq).unwrap();
        assert!(
            (c.0 - 48.001).abs() < 1e-12 && (c.1 - 18.001).abs() < 1e-12,
            "{c:?}"
        );
        assert_eq!(centre(&[]), None);
    }
}
