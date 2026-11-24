//! Stage E3: the road network buses drive on, from OpenStreetMap, and the
//! shortest drivable path between two points. Used to draw trips along roads
//! (GTFS shapes) and to place stops whose position OSM does not know.
//!
//! Stored as a small binary file (`data/osm/roads.bin`) so the 350 MB OSM
//! extract is read once: magic, nodes (lat, lon as f32), edges.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::io::{self, Read, Write};
use std::path::Path;

/// Road kinds a bus uses, cheapest first. Buses keep to main roads, so a
/// detour through residential streets has to save real distance to win.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Main,
    Secondary,
    Tertiary,
    Minor,
    Residential,
    Service,
    /// Footpaths, steps, pedestrian streets: walking only.
    Foot,
}

impl Class {
    pub fn from_highway(v: &str) -> Option<Class> {
        Some(match v {
            "motorway" | "motorway_link" | "trunk" | "trunk_link" | "primary" | "primary_link"
            | "busway" => Class::Main,
            "secondary" | "secondary_link" => Class::Secondary,
            "tertiary" | "tertiary_link" => Class::Tertiary,
            "unclassified" | "road" => Class::Minor,
            "residential" | "living_street" => Class::Residential,
            "service" => Class::Service,
            _ => return None,
        })
    }

    fn factor(self) -> f32 {
        match self {
            Class::Main => 1.0,
            Class::Secondary => 1.05,
            Class::Tertiary => 1.15,
            Class::Minor => 1.3,
            Class::Residential => 1.5,
            Class::Service => 2.5,
            Class::Foot => 1.0,
        }
    }

    fn code(self) -> u8 {
        self as u8
    }

    fn from_code(c: u8) -> Option<Class> {
        [
            Class::Main,
            Class::Secondary,
            Class::Tertiary,
            Class::Minor,
            Class::Residential,
            Class::Service,
            Class::Foot,
        ]
        .get(usize::from(c))
        .copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
    pub class: Class,
    /// Drivable only from `a` to `b`.
    pub oneway: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Roads {
    pub nodes: Vec<(f32, f32)>,
    pub edges: Vec<Edge>,
}

pub fn metres(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (a.0 - b.0) * 111_195.0;
    let dx = (a.1 - b.1) * 111_195.0 * a.0.to_radians().cos();
    (dx * dx + dy * dy).sqrt()
}

const MAGIC: &[u8; 4] = b"RDS1";

impl Roads {
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut out = Vec::with_capacity(12 + self.nodes.len() * 8 + self.edges.len() * 10);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());
        for (lat, lon) in &self.nodes {
            out.extend_from_slice(&lat.to_le_bytes());
            out.extend_from_slice(&lon.to_le_bytes());
        }
        out.extend_from_slice(&(self.edges.len() as u32).to_le_bytes());
        for e in &self.edges {
            out.extend_from_slice(&e.a.to_le_bytes());
            out.extend_from_slice(&e.b.to_le_bytes());
            out.push(e.class.code());
            out.push(u8::from(e.oneway));
        }
        std::fs::File::create(path)?.write_all(&out)
    }

    pub fn load(path: &Path) -> io::Result<Roads> {
        let mut bytes = vec![];
        std::fs::File::open(path)?.read_to_end(&mut bytes)?;
        let bad =
            |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("roads file: {why}"));
        let mut at = 0usize;
        let mut take = |n: usize| -> io::Result<&[u8]> {
            let slice = bytes.get(at..at + n).ok_or_else(|| bad("truncated"))?;
            at += n;
            Ok(slice)
        };
        if take(4)? != MAGIC {
            return Err(bad("not a roads file"));
        }
        let u32_at = |b: &[u8]| u32::from_le_bytes(b.try_into().expect("4 bytes"));
        let f32_at = |b: &[u8]| f32::from_le_bytes(b.try_into().expect("4 bytes"));
        let n = u32_at(take(4)?) as usize;
        let mut nodes = Vec::with_capacity(n.min(bytes.len() / 8));
        for _ in 0..n {
            let b = take(8)?;
            nodes.push((f32_at(&b[..4]), f32_at(&b[4..])));
        }
        let m = u32_at(take(4)?) as usize;
        let mut edges = Vec::with_capacity(m.min(bytes.len() / 10));
        for _ in 0..m {
            let b = take(10)?;
            let (a, bb) = (u32_at(&b[..4]), u32_at(&b[4..8]));
            if a as usize >= n || bb as usize >= n {
                return Err(bad("edge to a missing node"));
            }
            let class = Class::from_code(b[8]).ok_or_else(|| bad("unknown road class"))?;
            edges.push(Edge {
                a,
                b: bb,
                class,
                oneway: b[9] != 0,
            });
        }
        Ok(Roads { nodes, edges })
    }
}

/// Where a point meets the road network: `t` of the way from edge.a to edge.b.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub edge: u32,
    pub t: f64,
    pub point: (f64, f64),
    pub metres: f64,
}

#[derive(Debug, Clone, Copy)]
struct RouterEdge {
    a: u32,
    b: u32,
    oneway: bool,
    class: Class,
    cost: f64,
}

/// Roads ready for routing: adjacency lists and a grid of edges to snap to.
pub struct Router {
    nodes: Vec<(f64, f64)>,
    edges: Vec<RouterEdge>,
    /// node → (next node, cost)
    out: Vec<Vec<(u32, f64)>>,
    grid: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

/// Grid cell ~ 500 m.
const CELL: f64 = 0.005;

fn cell(p: (f64, f64)) -> (i32, i32) {
    ((p.0 / CELL).floor() as i32, (p.1 / CELL).floor() as i32)
}

/// How much closer than a main road a lesser road must be to be the one a
/// bus stop stands on (metres). Platforms are mapped a few metres beside
/// the carriageway, often nearer a side street or a parking spur.
fn snap_penalty(c: Class) -> f64 {
    match c {
        Class::Main | Class::Secondary => 0.0,
        Class::Tertiary => 10.0,
        Class::Minor => 15.0,
        Class::Residential => 30.0,
        Class::Service => 60.0,
        Class::Foot => 0.0,
    }
}

/// Nearest point to `p` on segment a-b, as (t, point), in a local flat projection.
fn project(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> (f64, (f64, f64)) {
    let k = p.0.to_radians().cos();
    let (ax, ay, bx, by, px, py) = (a.1 * k, a.0, b.1 * k, b.0, p.1 * k, p.0);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    (t, (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
}

impl Router {
    /// For people on foot: footpaths too, every street both ways, cost = metres.
    pub fn walking(roads: &Roads) -> Router {
        Router::build(roads, true)
    }

    /// For buses: roads only, one-way streets one way, lesser roads cost more.
    pub fn new(roads: &Roads) -> Router {
        Router::build(roads, false)
    }

    fn build(roads: &Roads, walking: bool) -> Router {
        let nodes: Vec<(f64, f64)> = roads
            .nodes
            .iter()
            .map(|(a, b)| (f64::from(*a), f64::from(*b)))
            .collect();
        let mut out = vec![vec![]; nodes.len()];
        let mut edges = Vec::with_capacity(roads.edges.len());
        let mut grid: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
        for e in &roads.edges {
            if !walking && e.class == Class::Foot {
                continue;
            }
            let (pa, pb) = (nodes[e.a as usize], nodes[e.b as usize]);
            let m = metres(pa, pb);
            let cost = if walking {
                m
            } else {
                m * f64::from(e.class.factor())
            };
            let oneway = e.oneway && !walking;
            out[e.a as usize].push((e.b, cost));
            if !oneway {
                out[e.b as usize].push((e.a, cost));
            }
            let id = edges.len() as u32;
            edges.push(RouterEdge {
                a: e.a,
                b: e.b,
                oneway,
                class: e.class,
                cost,
            });
            // Every cell the segment's bounding box touches.
            let (ca, cb) = (cell(pa), cell(pb));
            for y in ca.0.min(cb.0)..=ca.0.max(cb.0) {
                for x in ca.1.min(cb.1)..=ca.1.max(cb.1) {
                    grid.entry((y, x)).or_default().push(id);
                }
            }
        }
        Router {
            nodes,
            edges,
            out,
            grid,
        }
    }

    /// Where `p` meets the roads: the nearest road within `max_m`, lesser
    /// roads counted as farther by their [`snap_penalty`].
    pub fn snap(&self, p: (f64, f64), max_m: f64) -> Option<Snap> {
        // A degree of longitude is shortest (~74 km here), so cells are ≥ 370 m wide.
        let reach = (max_m / (CELL * 74_000.0)).ceil() as i32;
        let (cy, cx) = cell(p);
        let mut best: Option<(f64, Snap)> = None;
        let mut seen = std::collections::HashSet::new();
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                for &id in self.grid.get(&(cy + dy, cx + dx)).into_iter().flatten() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let e = &self.edges[id as usize];
                    let (t, point) = project(p, self.nodes[e.a as usize], self.nodes[e.b as usize]);
                    let d = metres(p, point);
                    let score = d + snap_penalty(e.class);
                    if d <= max_m && best.is_none_or(|(b, _)| score < b) {
                        best = Some((
                            score,
                            Snap {
                                edge: id,
                                t,
                                point,
                                metres: d,
                            },
                        ));
                    }
                }
            }
        }
        best.map(|(_, s)| s)
    }

    /// The road node nearest to `p` within `max_m` metres.
    pub fn nearest(&self, p: (f64, f64), max_m: f64) -> Option<u32> {
        let s = self.snap(p, max_m)?;
        let e = &self.edges[s.edge as usize];
        Some(if s.t < 0.5 { e.a } else { e.b })
    }

    /// Drivable path from where `from` meets the roads to where `to` does:
    /// points on the road, first and last being those meeting points.
    /// None if either is more than 150 m from a road, or no connection.
    pub fn path(&self, from: (f64, f64), to: (f64, f64)) -> Option<Vec<(f64, f64)>> {
        let (s, t) = (self.snap(from, 150.0)?, self.snap(to, 150.0)?);
        if s.edge == t.edge {
            let e = &self.edges[s.edge as usize];
            if s.t <= t.t || !e.oneway {
                return Some(vec![s.point, t.point]);
            }
        }
        self.between(&s, &t).map(|(_, points)| points)
    }

    /// Up to six places near `p` where a bus could stop, best first: every
    /// road within `max_m` whose score (metres + [`snap_penalty`]) is within
    /// 60 m of the best, at distinct spots. Both carriageways of a divided
    /// road come out here; which one the bus uses is decided by the trip.
    pub fn candidates(&self, p: (f64, f64), max_m: f64) -> Vec<Snap> {
        let reach = (max_m / (CELL * 74_000.0)).ceil() as i32;
        let (cy, cx) = cell(p);
        let mut all: Vec<(f64, Snap)> = vec![];
        let mut seen = std::collections::HashSet::new();
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                for &id in self.grid.get(&(cy + dy, cx + dx)).into_iter().flatten() {
                    if !seen.insert(id) {
                        continue;
                    }
                    let e = &self.edges[id as usize];
                    let (t, point) = project(p, self.nodes[e.a as usize], self.nodes[e.b as usize]);
                    let d = metres(p, point);
                    if d <= max_m {
                        all.push((
                            d + snap_penalty(e.class),
                            Snap {
                                edge: id,
                                t,
                                point,
                                metres: d,
                            },
                        ));
                    }
                }
            }
        }
        all.sort_by(|a, b| a.0.total_cmp(&b.0));
        let Some(&(best, _)) = all.first() else {
            return vec![];
        };
        let mut out: Vec<Snap> = vec![];
        for (score, snap) in all {
            if score > best + 60.0 || out.len() == 6 {
                break;
            }
            if out.iter().all(|o| metres(o.point, snap.point) > 3.0) {
                out.push(snap);
            }
        }
        out
    }

    /// The penalty that came with a snap (metres + road class), for choosing among candidates.
    pub fn snap_score(&self, s: &Snap) -> f64 {
        s.metres + snap_penalty(self.edges[s.edge as usize].class)
    }

    /// Driving from snap `s` to snap `t`: cost (metres × road class) and points.
    pub fn between(&self, s: &Snap, t: &Snap) -> Option<(f64, Vec<(f64, f64)>)> {
        if s.edge == t.edge {
            let e = &self.edges[s.edge as usize];
            if s.t <= t.t || !e.oneway {
                return Some(((t.t - s.t).abs() * e.cost, vec![s.point, t.point]));
            }
        }
        let (cost, nodes) = self.astar(s, t)?;
        let mut points = vec![s.point];
        for n in nodes {
            let p = self.nodes[n as usize];
            if points.last() != Some(&p) {
                points.push(p);
            }
        }
        if points.last() != Some(&t.point) {
            points.push(t.point);
        }
        Some((cost, points))
    }

    /// A* from partway along one edge to partway along another. Cost = metres
    /// × road-class factor; straight-line metres never overestimate it.
    fn astar(&self, s: &Snap, t: &Snap) -> Option<(f64, Vec<u32>)> {
        #[derive(PartialEq)]
        struct Open(f64, u32);
        impl Eq for Open {}
        impl Ord for Open {
            fn cmp(&self, other: &Self) -> Ordering {
                other.0.total_cmp(&self.0) // min-heap
            }
        }
        impl PartialOrd for Open {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
        let goal = t.point;
        let h = |n: u32| metres(self.nodes[n as usize], goal);
        let (se, te) = (&self.edges[s.edge as usize], &self.edges[t.edge as usize]);
        // Leaving the start edge: forward to b always, back to a if two-way.
        let mut starts = vec![(se.b, (1.0 - s.t) * se.cost)];
        if !se.oneway {
            starts.push((se.a, s.t * se.cost));
        }
        // Entering the target edge: from a always, from b if two-way.
        let mut finish: Vec<(u32, f64)> = vec![(te.a, t.t * te.cost)];
        if !te.oneway {
            finish.push((te.b, (1.0 - t.t) * te.cost));
        }
        let mut cost: std::collections::HashMap<u32, f64> = Default::default();
        let mut came: std::collections::HashMap<u32, u32> = Default::default();
        let mut open = BinaryHeap::new();
        for &(n, c) in &starts {
            if cost.get(&n).is_none_or(|&old| c < old) {
                cost.insert(n, c);
                open.push(Open(c + h(n), n));
            }
        }
        let mut best: Option<(f64, u32)> = None;
        while let Some(Open(f, n)) = open.pop() {
            if best.is_some_and(|(b, _)| f >= b) {
                break;
            }
            let here = cost[&n];
            if f > here + h(n) + 1e-9 {
                continue; // stale entry
            }
            for &(end, rest) in &finish {
                if end == n && best.is_none_or(|(b, _)| here + rest < b) {
                    best = Some((here + rest, n));
                }
            }
            for &(m, c) in &self.out[n as usize] {
                let via = here + c;
                if cost.get(&m).is_none_or(|&old| via < old) {
                    cost.insert(m, via);
                    came.insert(m, n);
                    open.push(Open(via + h(m), m));
                }
            }
        }
        let (total, end) = best?;
        let mut path = vec![end];
        let mut at = end;
        while let Some(&p) = came.get(&at) {
            path.push(p);
            at = p;
        }
        path.reverse();
        Some((total, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A small town grid, ~110 m between nodes (0.001° lat):
    //
    //   3 ─── 4 ─── 5        main road along the bottom (0-1-2),
    //   │     │     │        residential above, 1→4 one way north.
    //   0 ─── 1 ─── 2
    fn town() -> Roads {
        let n = |i: usize| {
            (
                (i / 3) as f32 * 0.001 + 48.0,
                (i % 3) as f32 * 0.0015 + 18.0,
            )
        };
        let e = |a, b, class, oneway| Edge {
            a,
            b,
            class,
            oneway,
        };
        Roads {
            nodes: (0..6).map(n).collect(),
            edges: vec![
                e(0, 1, Class::Main, false),
                e(1, 2, Class::Main, false),
                e(3, 4, Class::Residential, false),
                e(4, 5, Class::Residential, false),
                e(0, 3, Class::Residential, false),
                e(1, 4, Class::Residential, true),
                e(2, 5, Class::Residential, false),
            ],
        }
    }

    fn node(r: &Roads, i: usize) -> (f64, f64) {
        (f64::from(r.nodes[i].0), f64::from(r.nodes[i].1))
    }

    #[test]
    fn walkers_ignore_one_way_streets_and_take_footpaths_drivers_do_not() {
        let mut r = town();
        // A footpath straight from 3 to 1 (diagonal through the block).
        r.edges.push(Edge {
            a: 3,
            b: 1,
            class: Class::Foot,
            oneway: false,
        });
        let walk = Router::walking(&r);
        // 4 → 1 against the one-way: walkers go straight down it.
        assert_eq!(walk.path(node(&r, 4), node(&r, 1)).unwrap().len(), 2);
        // 3 → 1: along the footpath, not round by 0.
        assert_eq!(
            walk.path(node(&r, 3), node(&r, 1)).unwrap(),
            [node(&r, 3), node(&r, 1)]
        );
        // Buses never use the footpath.
        let drive = Router::new(&r).path(node(&r, 3), node(&r, 1)).unwrap();
        assert!(drive.len() > 2, "{drive:?}");
        // and the file keeps footpaths
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("walk.bin");
        r.save(&p).unwrap();
        assert_eq!(Roads::load(&p).unwrap(), r);
    }

    #[test]
    fn saves_and_loads_the_same_network() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("roads.bin");
        town().save(&p).unwrap();
        assert_eq!(Roads::load(&p).unwrap(), town());
    }

    #[test]
    fn a_damaged_file_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("roads.bin");
        std::fs::write(&p, b"RDS1\xff\xff\xff\xff").unwrap();
        assert!(Roads::load(&p).is_err());
        std::fs::write(&p, b"nope").unwrap();
        assert!(Roads::load(&p).is_err());
    }

    #[test]
    fn the_path_follows_the_roads_from_point_to_point() {
        let r = town();
        let route = Router::new(&r).path(node(&r, 0), node(&r, 2)).unwrap();
        assert_eq!(route, [node(&r, 0), node(&r, 1), node(&r, 2)]);
    }

    #[test]
    fn one_way_streets_are_driven_one_way_only() {
        let r = town();
        let router = Router::new(&r);
        // 1 → 4 is allowed directly; 4 → 1 must go round via 3-0 or 5-2.
        assert_eq!(router.path(node(&r, 1), node(&r, 4)).unwrap().len(), 2);
        assert!(router.path(node(&r, 4), node(&r, 1)).unwrap().len() > 2);
    }

    #[test]
    fn a_stop_beside_the_road_starts_the_path_on_the_road_without_detours() {
        let r = town();
        let router = Router::new(&r);
        // 20 m north of the middle of main road 0-1.
        let stop = (48.00018, 18.00075);
        let route = router.path(stop, node(&r, 2)).unwrap();
        assert!(
            (route[0].0 - 48.0).abs() < 1e-6 && (route[0].1 - 18.00075).abs() < 1e-6,
            "{route:?}"
        );
        assert_eq!(
            &route[1..],
            [node(&r, 1), node(&r, 2)],
            "straight on, not back to node 0 first"
        );
    }

    #[test]
    fn the_main_road_wins_over_a_slightly_closer_side_street() {
        let r = town();
        let router = Router::new(&r);
        // 61 m from the main road, 50 m from the residential street above it.
        let stop = (48.00055, 18.00075);
        let s = router.snap(stop, 150.0).unwrap();
        assert!((s.point.0 - 48.0).abs() < 1e-6, "{s:?}");
    }

    #[test]
    fn a_dead_end_service_road_next_to_the_stop_is_not_driven_into() {
        let mut r = town();
        // A parking spur from node 1 into the middle of the block, ending 6 m
        // from the stop; the main road is 56 m away, the side streets 52+ m.
        r.nodes.push((48.00054, 18.00075));
        r.edges.push(Edge {
            a: 1,
            b: 6,
            class: Class::Service,
            oneway: false,
        });
        let router = Router::new(&r);
        let stop = (48.0005, 18.0008);
        let route = router.path(node(&r, 0), stop).unwrap();
        assert!(
            route.iter().all(|p| (p.0 - 48.0).abs() < 1e-6),
            "stays on the main road: {route:?}"
        );
    }

    #[test]
    fn points_far_from_any_road_are_not_snapped() {
        let r = town();
        assert!(Router::new(&r).snap((48.1, 18.0), 150.0).is_none()); // 11 km away
        assert!(Router::new(&r).path((48.1, 18.0), node(&r, 2)).is_none());
    }

    #[test]
    fn one_way_streets_also_hold_on_the_first_and_last_stretch() {
        let r = town();
        let router = Router::new(&r);
        // From the middle of one-way 1→4 (northbound) to node 1: must go on north, round, and back.
        let mid = (48.0005, 18.0015);
        let route = router.path(mid, node(&r, 1)).unwrap();
        assert!(route.len() > 3, "{route:?}");
    }

    #[test]
    fn no_connection_is_none() {
        let mut r = town();
        r.nodes.push((49.0, 19.0));
        r.nodes.push((49.0005, 19.0));
        r.edges.push(Edge {
            a: 6,
            b: 7,
            class: Class::Main,
            oneway: false,
        });
        assert!(Router::new(&r).path(node(&r, 0), node(&r, 7)).is_none());
    }

    /// A divided road: eastbound carriageway (one-way, lat 48.0000, nodes 0-2)
    /// and westbound (one-way, lat 48.0002, nodes 3-5), joined at both ends.
    fn divided() -> Roads {
        let mut nodes = vec![];
        for lat in [48.0f32, 48.0002] {
            for lon in [18.0f32, 18.005, 18.01] {
                nodes.push((lat, lon));
            }
        }
        let e = |a, b| Edge {
            a,
            b,
            class: Class::Main,
            oneway: true,
        };
        Roads {
            nodes,
            edges: vec![e(0, 1), e(1, 2), e(5, 4), e(4, 3), e(2, 5), e(3, 0)],
        }
    }

    #[test]
    fn candidates_include_both_carriageways_of_a_divided_road() {
        let router = Router::new(&divided());
        // Platform 13 m north of the eastbound lane, 9 m south of the westbound.
        let c = router.candidates((48.00012, 18.0075), 60.0);
        let lanes: Vec<f64> = c.iter().map(|s| (s.point.0 * 1e4).round() / 1e4).collect();
        assert!(lanes.contains(&48.0) && lanes.contains(&48.0002), "{c:?}");
        assert!(c.len() <= 6);
    }

    #[test]
    fn between_gives_the_cost_of_driving_and_its_points() {
        let router = Router::new(&divided());
        let c = router.candidates((48.0, 18.0025), 60.0);
        let east_a = c.iter().find(|s| s.point.0 == 48.0).unwrap();
        let d = router.candidates((48.00012, 18.0075), 60.0);
        let east_b = d.iter().find(|s| (s.point.0 - 48.0).abs() < 1e-9).unwrap();
        let west_b = d
            .iter()
            .find(|s| (s.point.0 - 48.0002).abs() < 1e-5)
            .unwrap();
        let (straight, pts) = router.between(east_a, east_b).unwrap();
        assert!(pts.iter().all(|p| p.0 == 48.0));
        // Reaching the westbound lane means driving to the end, turning, and back.
        let (round, _) = router.between(east_a, west_b).unwrap();
        assert!(round > straight * 1.5, "{round} vs {straight}");
    }

    #[test]
    fn a_through_street_50_m_away_is_still_a_candidate_beside_a_closer_side_street() {
        let r = town();
        let router = Router::new(&r);
        // 10 m from residential 4-5 (lat 48.001), 101 m from main road 0-1: too far.
        // 45 m from the one-way 1→4 at lon 18.0015: must be offered.
        let c = router.candidates((48.0009, 18.0009), 60.0); // 45 m from one-way 1→4
        assert!(
            c.iter().any(|s| (s.point.1 - 18.0015).abs() < 1e-6),
            "{c:?}"
        );
    }
}
