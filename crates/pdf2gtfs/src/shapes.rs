//! Stage E4: a trip's path along the roads (GTFS shape). Between two stops
//! whose position is known, the bus drives the road path; a stop whose
//! position is only a guess is placed on that path where the timetable
//! minutes say the bus is, instead of at the village centre (which drew
//! zig-zags to the centre and back). Such stops stay approximate.

use crate::gtfs::Location;
use crate::roads::{Snap, metres};

pub type Point = (f64, f64);

/// A stop on the trip: where it is and when the bus arrives and leaves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Call {
    pub at: Location,
    pub arrival: u32,
    pub departure: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Traced {
    pub points: Vec<Point>,
    /// Per call: metres along `points` (GTFS shape_dist_traveled).
    pub dist: Vec<f64>,
    /// Per call: new position for a stop whose position was approximate.
    pub placed: Vec<Option<Point>>,
}

/// A road path longer than this × the straight line (+ 1 km) is a routing
/// mistake (wrong road snapped), not the bus's route.
const MAX_DETOUR: f64 = 2.5;

fn length(points: &[Point]) -> f64 {
    points.windows(2).map(|w| metres(w[0], w[1])).sum()
}

/// The point `along` metres into `points`.
fn at(points: &[Point], along: f64) -> Point {
    let mut left = along;
    for w in points.windows(2) {
        let d = metres(w[0], w[1]);
        if left <= d && d > 0.0 {
            let f = left / d;
            return (
                w[0].0 + (w[1].0 - w[0].0) * f,
                w[0].1 + (w[1].1 - w[0].1) * f,
            );
        }
        left -= d;
    }
    *points.last().expect("non-empty path")
}

/// What tracing needs from a road network.
pub trait RoadNet {
    /// Where a bus could stop near `p`: candidates with a penalty in metres
    /// (distance from the platform, lesser roads counted farther). Empty if no road is near.
    fn candidates(&mut self, p: Point) -> Vec<(Snap, f64)>;
    /// Driving from one candidate to another: cost (metres, weighted by road)
    /// and points, first `a.point`, last `b.point`.
    fn between(&mut self, a: &Snap, b: &Snap) -> Option<(f64, Vec<Point>)>;
}

impl RoadNet for crate::roads::Router {
    fn candidates(&mut self, p: Point) -> Vec<(Snap, f64)> {
        crate::roads::Router::candidates(self, p, 60.0)
            .into_iter()
            .map(|s| (s, self.snap_score(&s)))
            .collect()
    }

    fn between(&mut self, a: &Snap, b: &Snap) -> Option<(f64, Vec<Point>)> {
        crate::roads::Router::between(self, a, b)
    }
}

/// A drive between two snaps: cost and points.
type Drive = Option<(f64, Vec<Point>)>;

/// A road network that remembers: the same stops are routed by many trips.
pub struct Cached<R> {
    net: R,
    candidates: std::collections::HashMap<[i64; 2], Vec<(Snap, f64)>>,
    /// keyed by (edge, t × 1e9) of both ends
    between: std::collections::HashMap<(u32, i64, u32, i64), Drive>,
}

impl<R: RoadNet> Cached<R> {
    pub fn new(net: R) -> Self {
        Cached {
            net,
            candidates: Default::default(),
            between: Default::default(),
        }
    }
}

impl<R: RoadNet> RoadNet for Cached<R> {
    fn candidates(&mut self, p: Point) -> Vec<(Snap, f64)> {
        let key = [p.0, p.1].map(|x| (x * 1e7).round() as i64);
        if let Some(c) = self.candidates.get(&key) {
            return c.clone();
        }
        let c = self.net.candidates(p);
        self.candidates.insert(key, c.clone());
        c
    }

    fn between(&mut self, a: &Snap, b: &Snap) -> Option<(f64, Vec<Point>)> {
        let key = (
            a.edge,
            (a.t * 1e9).round() as i64,
            b.edge,
            (b.t * 1e9).round() as i64,
        );
        if let Some(r) = self.between.get(&key) {
            return r.clone();
        }
        let r = self.net.between(a, b);
        self.between.insert(key, r.clone());
        r
    }
}

/// Points with their running length; repeated points are skipped.
#[derive(Default)]
struct Polyline {
    points: Vec<Point>,
    total: f64,
}

impl Polyline {
    fn push(&mut self, p: Point) {
        if let Some(&last) = self.points.last() {
            if last == p {
                return;
            }
            self.total += metres(last, p);
        }
        self.points.push(p);
    }
}

/// A straight line where no road path was believable costs this × its length,
/// so a real road path is preferred whenever one exists.
const STRAIGHT_COST: f64 = 3.0;

/// The road each stop with a known position is on, chosen for the whole trip
/// at once (shortest drive plus snap penalties), and the path between each
/// consecutive pair. Platforms beside a divided road are near both
/// carriageways; only the trip's direction tells which one the bus uses.
fn match_roads(stops: &[Point], net: &mut dyn RoadNet) -> Vec<Vec<Point>> {
    let off_road = |p: Point| Snap {
        edge: u32::MAX,
        t: 0.0,
        point: p,
        metres: 0.0,
    };
    let cands: Vec<Vec<(Snap, f64)>> = stops
        .iter()
        .map(|&p| {
            let c = net.candidates(p);
            if c.is_empty() {
                vec![(off_road(p), 0.0)]
            } else {
                c
            }
        })
        .collect();
    let n = stops.len();
    // best[k][c]: cheapest way to reach candidate c of stop k; back[k][c]: from which candidate.
    let mut best: Vec<Vec<f64>> = vec![cands[0].iter().map(|(_, pen)| *pen).collect()];
    let mut back: Vec<Vec<usize>> = vec![vec![0; cands[0].len()]];
    let mut paths: Vec<Vec<Vec<Vec<Point>>>> = vec![];
    for k in 1..n {
        let limit = MAX_DETOUR * metres(stops[k - 1], stops[k]) + 1000.0;
        let mut step_paths = vec![vec![vec![]; cands[k].len()]; cands[k - 1].len()];
        let mut row = vec![f64::INFINITY; cands[k].len()];
        let mut from = vec![0; cands[k].len()];
        for (a, (sa, _)) in cands[k - 1].iter().enumerate() {
            for (b, (sb, pen)) in cands[k].iter().enumerate() {
                let routed = if sa.edge == u32::MAX || sb.edge == u32::MAX {
                    None
                } else {
                    net.between(sa, sb)
                        .filter(|(_, p)| p.len() >= 2 && length(p) <= limit)
                };
                let (cost, path) = routed.unwrap_or_else(|| {
                    (
                        STRAIGHT_COST * metres(sa.point, sb.point),
                        vec![sa.point, sb.point],
                    )
                });
                let total = best[k - 1][a] + cost + pen;
                if total < row[b] {
                    row[b] = total;
                    from[b] = a;
                }
                step_paths[a][b] = path;
            }
        }
        best.push(row);
        back.push(from);
        paths.push(step_paths);
    }
    // Walk back from the cheapest last candidate.
    let mut c = (0..cands[n - 1].len())
        .min_by(|&x, &y| best[n - 1][x].total_cmp(&best[n - 1][y]))
        .unwrap_or(0);
    let mut out = vec![vec![]; n - 1];
    for k in (1..n).rev() {
        let a = back[k][c];
        out[k - 1] = std::mem::take(&mut paths[k - 1][a][c]);
        c = a;
    }
    out
}

/// Trace a trip along the roads of `net`.
// `i` indexes `calls`, `dist` and `placed` together; index loops read clearer here.
#[allow(clippy::needless_range_loop)]
pub fn trace(calls: &[Call], net: &mut dyn RoadNet) -> Traced {
    let n = calls.len();
    let pos = |i: usize| (calls[i].at.lat, calls[i].at.lon);
    let mut line = Polyline::default();
    let mut dist = vec![0.0; n];
    let mut placed = vec![None; n];
    let anchors: Vec<usize> = (0..n).filter(|&i| !calls[i].at.approximate).collect();

    if anchors.len() < 2 {
        // Nothing to route between: straight through the positions we have.
        for i in 0..n {
            line.push(pos(i));
            dist[i] = line.total;
        }
    } else {
        let (first, last) = (anchors[0], anchors[anchors.len() - 1]);
        // Guessed stops before the first known one: straight, left where they are.
        // The first known stop itself is where its road path begins.
        for i in 0..first {
            line.push(pos(i));
            dist[i] = line.total;
        }
        let known: Vec<Point> = anchors.iter().map(|&i| pos(i)).collect();
        let roads = match_roads(&known, net);
        for (w, path) in anchors.windows(2).zip(roads) {
            let (i, j) = (w[0], w[1]);
            let along = length(&path);
            // The bus is where the stop meets the road (path[0]), not on the platform.
            line.push(path[0]);
            let start = line.total;
            dist[i] = start;
            // Guessed stops in between: where the timetable says the bus is,
            // counting only driving time (a bus waiting 10 min at a station is not moving).
            let dwell = |k: usize| f64::from(calls[k].departure.saturating_sub(calls[k].arrival));
            let waits: f64 = (i + 1..j).map(dwell).sum();
            let driving = f64::from(calls[j].arrival.saturating_sub(calls[i].departure)) - waits;
            let mut prev = 0.0_f64;
            let mut waited = 0.0;
            for k in i + 1..j {
                let by_order = (k - i) as f64 / (j - i) as f64;
                let driven =
                    f64::from(calls[k].arrival.saturating_sub(calls[i].departure)) - waited;
                waited += dwell(k);
                let by_time = if driving > 0.0 {
                    driven / driving
                } else {
                    by_order
                };
                // Same minute for several stops: keep them in order, never on top of each other.
                let f = by_time
                    .clamp(0.0, 1.0)
                    .max(prev + 1e-3)
                    .min(1.0 - 1e-3 * (j - k) as f64);
                prev = f;
                placed[k] = Some(at(&path, f * along));
                dist[k] = start + f * along;
            }
            for p in path {
                line.push(p);
            }
            dist[j] = line.total;
        }
        // Guessed stops after the last known one: straight.
        for i in last + 1..n {
            line.push(pos(i));
            dist[i] = line.total;
        }
    }
    // GTFS wants distances to grow from stop to stop, even for two stops at one
    // spot, and never past the end of the shape: nudge forward, then back from the end.
    for i in 1..n {
        if dist[i] <= dist[i - 1] {
            dist[i] = dist[i - 1] + 0.1;
        }
    }
    if n > 0 && dist[n - 1] > line.total {
        dist[n - 1] = line.total;
        for i in (0..n - 1).rev() {
            if dist[i] >= dist[i + 1] {
                dist[i] = (dist[i + 1] - 0.1).max(0.0);
            }
        }
    }
    Traced {
        points: line.points,
        dist,
        placed,
    }
}

/// Estimates of a stop's position from several trips must agree to be used:
/// their median, unless they are spread more than `max_m` around it.
pub fn consensus(estimates: &[Point], max_m: f64) -> Option<Point> {
    if estimates.is_empty() {
        return None;
    }
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mid = (
        median(estimates.iter().map(|p| p.0).collect()),
        median(estimates.iter().map(|p| p.1).collect()),
    );
    let spread = median(estimates.iter().map(|p| metres(*p, mid)).collect());
    (spread <= max_m).then_some(mid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(lat: f64, lon: f64) -> Location {
        Location {
            lat,
            lon,
            approximate: false,
        }
    }

    fn guess(lat: f64, lon: f64) -> Location {
        Location {
            lat,
            lon,
            approximate: true,
        }
    }

    /// Fake roads: every trip bends through a point north of the straight line.
    /// Roads as a closure from point to point: each stop is on its own road.
    struct Fake<F>(F);

    impl<F: FnMut(Point, Point) -> Option<Vec<Point>>> RoadNet for Fake<F> {
        fn candidates(&mut self, p: Point) -> Vec<(Snap, f64)> {
            vec![(
                Snap {
                    edge: 0,
                    t: 0.0,
                    point: p,
                    metres: 0.0,
                },
                0.0,
            )]
        }
        fn between(&mut self, a: &Snap, b: &Snap) -> Option<(f64, Vec<Point>)> {
            (self.0)(a.point, b.point).map(|p| (length(&p), p))
        }
    }

    fn c(at: Location, minute: u32) -> Call {
        Call {
            at,
            arrival: minute,
            departure: minute,
        }
    }

    fn bendy(a: Point, b: Point) -> Option<Vec<Point>> {
        Some(vec![a, ((a.0 + b.0) / 2.0 + 0.002, (a.1 + b.1) / 2.0), b])
    }

    fn straight_only(_: Point, _: Point) -> Option<Vec<Point>> {
        None
    }

    #[test]
    fn the_path_follows_the_roads_through_every_stop() {
        let calls = [
            c(exact(48.0, 18.0), 0),
            c(exact(48.0, 18.01), 2),
            c(exact(48.0, 18.02), 4),
        ];
        let t = trace(&calls, &mut Fake(bendy));
        assert_eq!(t.points.len(), 5); // 3 stops + 2 bends
        assert_eq!(t.points[0], (48.0, 18.0));
        assert_eq!(t.points[2], (48.0, 18.01));
        assert_eq!(t.dist[0], 0.0);
        assert!((t.dist[2] - length(&t.points)).abs() < 1e-6);
        assert!(t.dist[1] > 0.0 && t.dist[1] < t.dist[2]);
        assert_eq!(t.placed, [None, None, None]);
    }

    #[test]
    fn a_stop_with_a_guessed_position_is_placed_on_the_road_by_its_time() {
        // B is "somewhere in the village" (far off); the bus is there 3 of 10 minutes in.
        let calls = [
            c(exact(48.0, 18.0), 0),
            c(guess(47.9, 18.3), 3),
            c(exact(48.0, 18.02), 10),
        ];
        let t = trace(&calls, &mut Fake(bendy));
        let b = t.placed[1].expect("placed");
        let total = t.dist[2];
        assert!(
            (t.dist[1] - 0.3 * total).abs() < 1.0,
            "{} of {total}",
            t.dist[1]
        );
        assert!(metres(b, at(&t.points, t.dist[1])) < 0.5);
        assert!(
            !t.points.contains(&(47.9, 18.3)),
            "the village centre is not on the path"
        );
        assert_eq!((t.placed[0], t.placed[2]), (None, None));
    }

    #[test]
    fn stops_at_the_same_minute_are_spread_by_their_order() {
        let calls = [
            c(exact(48.0, 18.0), 5),
            c(guess(47.9, 18.3), 5),
            c(guess(47.9, 18.3), 5),
            c(exact(48.0, 18.03), 5),
        ];
        let t = trace(&calls, &mut Fake(bendy));
        assert!(t.dist[0] < t.dist[1] && t.dist[1] < t.dist[2] && t.dist[2] < t.dist[3]);
    }

    #[test]
    fn without_a_road_path_the_line_is_straight() {
        let calls = [c(exact(48.0, 18.0), 0), c(exact(48.0, 18.02), 4)];
        let t = trace(&calls, &mut Fake(straight_only));
        assert_eq!(t.points, [(48.0, 18.0), (48.0, 18.02)]);
    }

    #[test]
    fn an_absurd_detour_is_not_believed() {
        let calls = [c(exact(48.0, 18.0), 0), c(exact(48.0, 18.01), 3)];
        let far = |a: Point, b: Point| Some(vec![a, (48.2, 18.0), b]); // 22 km round for 750 m
        assert_eq!(
            trace(&calls, &mut Fake(far)).points,
            [(48.0, 18.0), (48.0, 18.01)]
        );
    }

    #[test]
    fn guessed_stops_at_the_ends_keep_their_place_and_stay_unplaced() {
        let calls = [
            c(guess(47.9, 18.0), 0),
            c(exact(48.0, 18.0), 5),
            c(exact(48.0, 18.01), 8),
        ];
        let t = trace(&calls, &mut Fake(bendy));
        assert_eq!(t.points[0], (47.9, 18.0));
        assert_eq!(t.placed[0], None);
        assert!(t.dist.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn a_trip_with_one_or_no_known_stop_is_a_straight_line() {
        let calls = [c(guess(47.9, 18.0), 0), c(guess(47.95, 18.0), 5)];
        let t = trace(&calls, &mut Fake(bendy));
        assert_eq!(t.points, [(47.9, 18.0), (47.95, 18.0)]);
        assert_eq!(t.placed, [None, None]);
    }

    #[test]
    fn waiting_at_a_stop_is_not_driving() {
        // A → B (guessed) 3 min driving, waits 10 min at B, → C 3 min: B is halfway.
        let calls = [
            c(exact(48.0, 18.0), 0),
            Call {
                at: guess(47.9, 18.3),
                arrival: 3,
                departure: 13,
            },
            c(exact(48.0, 18.02), 16),
        ];
        let t = trace(&calls, &mut Fake(bendy));
        assert!(
            (t.dist[1] / t.dist[2] - 0.5).abs() < 0.01,
            "{}",
            t.dist[1] / t.dist[2]
        );
    }

    #[test]
    fn estimates_that_agree_give_their_middle() {
        let p = consensus(&[(48.0, 18.0), (48.0005, 18.0), (48.0010, 18.0)], 500.0).unwrap();
        assert_eq!(p, (48.0005, 18.0));
    }

    #[test]
    fn estimates_that_disagree_give_nothing() {
        // Two routes put the stop 2 km apart: neither is believed.
        assert_eq!(consensus(&[(48.0, 18.0), (48.018, 18.0)], 500.0), None);
        assert_eq!(
            consensus(&[(48.0, 18.0), (48.0, 18.0), (48.018, 18.0)], 500.0),
            Some((48.0, 18.0))
        );
        assert_eq!(consensus(&[], 500.0), None);
    }

    #[test]
    fn distances_grow_but_never_pass_the_end_of_the_shape() {
        // The last two stops are at one spot (both sides of a turning loop).
        let calls = [
            c(exact(48.0, 18.0), 0),
            c(exact(48.0, 18.01), 3),
            c(exact(48.0, 18.01), 4),
        ];
        let t = trace(&calls, &mut Fake(straight_only));
        let total = length(&t.points);
        assert!(t.dist.windows(2).all(|w| w[0] < w[1]), "{:?}", t.dist);
        assert!(
            *t.dist.last().unwrap() <= total + 1e-9,
            "{:?} > {total}",
            t.dist
        );
    }

    #[test]
    fn the_line_runs_on_the_road_not_out_to_each_stop_s_platform() {
        // Platforms are mapped 11 m beside the road the bus drives.
        let road = |a: Point, b: Point| {
            Some(vec![
                (a.0 - 0.0001, a.1),
                ((a.0 + b.0) / 2.0 - 0.0001, (a.1 + b.1) / 2.0),
                (b.0 - 0.0001, b.1),
            ])
        };
        let calls = [
            c(exact(48.0, 18.0), 0),
            c(exact(48.0, 18.01), 2),
            c(exact(48.0, 18.02), 4),
        ];
        let t = trace(&calls, &mut Fake(road));
        assert!(
            t.points.iter().all(|p| (p.0 - 47.9999).abs() < 1e-9),
            "{:?}",
            t.points
        );
        assert_eq!(t.dist[0], 0.0);
    }

    #[test]
    fn on_a_divided_road_the_bus_keeps_to_its_own_carriageway() {
        // Eastbound lane at lat 48.0000, westbound at 48.0002 (one-way each,
        // joined only at the ends 18.0 and 18.03). Platforms sit between them,
        // a little nearer the westbound lane, but this bus goes east.
        let roads = crate::roads::Roads {
            nodes: [48.0f32, 48.0002]
                .iter()
                .flat_map(|&lat| [18.0f32, 18.01, 18.02, 18.03].map(|lon| (lat, lon)))
                .collect(),
            edges: {
                use crate::roads::{Class, Edge};
                let e = |a, b| Edge {
                    a,
                    b,
                    class: Class::Main,
                    oneway: true,
                };
                vec![
                    e(0, 1),
                    e(1, 2),
                    e(2, 3),
                    e(7, 6),
                    e(6, 5),
                    e(5, 4),
                    e(3, 7),
                    e(4, 0),
                ]
            },
        };
        let mut net = crate::roads::Router::new(&roads);
        let platform = |lon| exact(48.00011, lon);
        let calls = [
            c(platform(18.005), 0),
            c(platform(18.015), 2),
            c(platform(18.025), 4),
        ];
        let t = trace(&calls, &mut net);
        assert!(
            t.points
                .iter()
                .all(|p| p.0.abs() - 48.0 < 1e-5 && p.0 < 48.0001),
            "east lane only: {:?}",
            t.points
        );
        assert!(t.points.len() <= 5, "{:?}", t.points);
    }
}
