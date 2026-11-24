//! Journey planning on a bus timetable with RAPTOR (Delling, Pajor, Werneck
//! 2012): round k finds the earliest arrival using k buses, so the result is
//! every journey that is either faster or has fewer changes than the others.
//!
//! No I/O and no clock: the same code runs on the server, in the Android app
//! and compiled to WebAssembly in the iPhone web app.

use chrono::{Days, NaiveDate, NaiveDateTime};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct Stop {
    pub lat: f64,
    pub lon: f64,
    /// Position known (not just the village centre): only then is walking offered.
    pub exact: bool,
}

#[derive(Debug, Clone)]
pub struct Call {
    pub stop: usize,
    /// Minutes after midnight of the service day (can exceed 1440).
    pub arrival: u32,
    pub departure: u32,
    pub board: bool,
    pub alight: bool,
}

#[derive(Debug, Clone)]
pub struct Trip {
    pub calls: Vec<Call>,
    /// Index into the service calendars given to [`Network::new`].
    pub service: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Leg {
    Ride {
        trip: usize,
        service_date: NaiveDate,
        /// Call indexes within the trip.
        from_call: usize,
        to_call: usize,
        from: usize,
        to: usize,
        depart: NaiveDateTime,
        arrive: NaiveDateTime,
    },
    Walk {
        from: usize,
        to: usize,
        minutes: u32,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Journey {
    pub legs: Vec<Leg>,
}

impl Journey {
    pub fn rides(&self) -> impl Iterator<Item = &Leg> {
        self.legs.iter().filter(|l| matches!(l, Leg::Ride { .. }))
    }

    pub fn transfers(&self) -> usize {
        self.rides().count().saturating_sub(1)
    }

    /// When to leave the origin (a walk before the first bus counts).
    pub fn depart(&self) -> NaiveDateTime {
        let mut walk = 0;
        for l in &self.legs {
            match l {
                Leg::Walk { minutes, .. } => walk += minutes,
                Leg::Ride { depart, .. } => {
                    return *depart - chrono::Duration::minutes(i64::from(walk));
                }
            }
        }
        unreachable!("a journey has a ride")
    }

    /// Longest time spent waiting for the next bus, in minutes.
    pub fn longest_wait(&self) -> i64 {
        let mut longest = 0;
        let mut last: Option<NaiveDateTime> = None;
        for l in &self.legs {
            match l {
                Leg::Walk { minutes, .. } => {
                    last = last.map(|t| t + chrono::Duration::minutes(i64::from(*minutes)));
                }
                Leg::Ride { depart, arrive, .. } => {
                    if let Some(t) = last {
                        longest = longest.max((*depart - t).num_minutes());
                    }
                    last = Some(*arrive);
                }
            }
        }
        longest
    }

    pub fn arrive(&self) -> NaiveDateTime {
        let mut walk = 0;
        for l in self.legs.iter().rev() {
            match l {
                Leg::Walk { minutes, .. } => walk += minutes,
                Leg::Ride { arrive, .. } => {
                    return *arrive + chrono::Duration::minutes(i64::from(walk));
                }
            }
        }
        unreachable!("a journey has a ride")
    }
}

/// Walking between stops up to this far (straight line).
pub const WALK_MAX_KM: f64 = 0.5;
/// Minutes to change from one bus to another at the same stop.
pub const MIN_CHANGE: u32 = 1;
/// At most this many buses in one journey.
pub const MAX_RIDES: usize = 4;
/// Longer waits between buses (overnight at a bus station) are not offered.
pub const MAX_WAIT: i64 = 180;

pub fn km(a: &Stop, b: &Stop) -> f64 {
    let dy = (a.lat - b.lat) * 111.0;
    let dx = (a.lon - b.lon) * 111.0 * a.lat.to_radians().cos();
    (dx * dx + dy * dy).sqrt()
}

/// Walking time: streets are ~25 % longer than the straight line, 4.5 km/h.
pub fn walk_minutes(km: f64) -> u32 {
    (km * 1.25 / 4.5 * 60.0).ceil().max(1.0) as u32
}

#[derive(Debug, Default)]
pub struct Network {
    pub stops: Vec<Stop>,
    pub trips: Vec<Trip>,
    services: Vec<BTreeSet<NaiveDate>>,
    /// stop → (nearby stop, walking minutes)
    walks: Vec<Vec<(usize, u32)>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Label {
    Unreached,
    Origin,
    /// Bus instance, call boarded, call alighted.
    Ride(usize, usize, usize),
    Walk {
        from: usize,
        minutes: u32,
    },
}

impl Network {
    pub fn new(stops: Vec<Stop>, trips: Vec<Trip>, services: Vec<BTreeSet<NaiveDate>>) -> Self {
        let mut walks = vec![vec![]; stops.len()];
        for a in 0..stops.len() {
            for b in a + 1..stops.len() {
                if !(stops[a].exact && stops[b].exact) {
                    continue;
                }
                let d = km(&stops[a], &stops[b]);
                if d <= WALK_MAX_KM {
                    let m = walk_minutes(d);
                    walks[a].push((b, m));
                    walks[b].push((a, m));
                }
            }
        }
        Self {
            stops,
            trips,
            services,
            walks,
        }
    }

    /// Treat two stops as one place, `minutes` apart (platforms of one
    /// station), whether or not their positions are known. Replaces any
    /// walking time computed from positions: platforms mapped on one spot
    /// still take the station's change time.
    pub fn link(&mut self, a: usize, b: usize, minutes: u32) {
        if a == b {
            return;
        }
        for (x, y) in [(a, b), (b, a)] {
            match self.walks[x].iter_mut().find(|(s, _)| *s == y) {
                Some(w) => w.1 = minutes,
                None => self.walks[x].push((y, minutes)),
            }
        }
    }

    /// Replace the walks guessed from positions with walks measured along
    /// the streets: (stop, stop, minutes), either direction.
    pub fn set_walks(&mut self, walks: impl IntoIterator<Item = (usize, usize, u32)>) {
        self.walks = vec![vec![]; self.stops.len()];
        for (a, b, minutes) in walks {
            self.link(a, b, minutes);
        }
    }

    pub fn runs_on(&self, trip: usize, date: NaiveDate) -> bool {
        self.services[self.trips[trip].service].contains(&date)
    }

    /// Journeys from `from` to `to` leaving at or after `at`: the fastest, and
    /// any with fewer changes that arrive later.
    pub fn plan(&self, from: &[usize], to: &[usize], at: NaiveDateTime) -> Vec<Journey> {
        if from.iter().any(|s| to.contains(s)) {
            return vec![];
        }
        let n = self.stops.len();
        let mut target = vec![false; n];
        for &s in to {
            target[s] = true;
        }
        let midnight = at.date().and_hms_opt(0, 0, 0).expect("midnight exists");
        let t0 = (at - midnight).num_minutes();
        // Buses running around `at`: yesterday's late trips, today's, tomorrow's,
        // with times shifted to minutes after today's midnight.
        let mut buses: Vec<(usize, NaiveDate, i64)> = vec![];
        for (date, shift) in [
            (at.date() - Days::new(1), -1440),
            (at.date(), 0),
            (at.date() + Days::new(1), 1440),
        ] {
            for t in 0..self.trips.len() {
                let last = self.trips[t].calls.last().map_or(0, |c| c.arrival);
                if self.runs_on(t, date) && shift + i64::from(last) >= t0 {
                    buses.push((t, date, shift));
                }
            }
        }

        let mut best = vec![i64::MAX; n];
        let mut best_target = i64::MAX;
        let mut arrival = vec![vec![i64::MAX; n]];
        let mut label = vec![vec![Label::Unreached; n]];
        let mut marked = vec![];
        for &o in from {
            arrival[0][o] = t0;
            label[0][o] = Label::Origin;
            best[o] = t0;
            marked.push(o);
        }
        for &o in from {
            for &(s, m) in &self.walks[o] {
                let a = t0 + i64::from(m);
                if !target[s] && a < best[s] {
                    arrival[0][s] = a;
                    label[0][s] = Label::Walk {
                        from: o,
                        minutes: m,
                    };
                    best[s] = a;
                    marked.push(s);
                }
            }
        }

        let mut journeys = vec![];
        for k in 1..=MAX_RIDES {
            if marked.is_empty() {
                break;
            }
            let mut can_board = vec![false; n];
            for &s in &marked {
                can_board[s] = true;
            }
            let mut arr = vec![i64::MAX; n];
            let mut lab = vec![Label::Unreached; n];
            let mut rode: Vec<usize> = vec![];
            for (b, &(t, _, shift)) in buses.iter().enumerate() {
                let calls = &self.trips[t].calls;
                let mut boarded: Option<usize> = None;
                for (j, c) in calls.iter().enumerate() {
                    if let Some(on) = boarded
                        && c.alight
                    {
                        let a = shift + i64::from(c.arrival);
                        if a < best[c.stop] && a < best_target {
                            best[c.stop] = a;
                            if target[c.stop] {
                                best_target = a;
                            }
                            arr[c.stop] = a;
                            lab[c.stop] = Label::Ride(b, on, j);
                            rode.push(c.stop);
                        }
                    }
                    if boarded.is_none() && c.board && can_board[c.stop] && j + 1 < calls.len() {
                        let change = match label[k - 1][c.stop] {
                            Label::Ride(..) => MIN_CHANGE,
                            _ => 0,
                        };
                        if shift + i64::from(c.departure)
                            >= arrival[k - 1][c.stop] + i64::from(change)
                        {
                            boarded = Some(j);
                        }
                    }
                }
            }
            rode.sort_unstable();
            rode.dedup();
            let mut next = rode.clone();
            for &s in &rode {
                for &(s2, m) in &self.walks[s] {
                    let a = arr[s] + i64::from(m);
                    if a < best[s2] && a < best_target {
                        best[s2] = a;
                        if target[s2] {
                            best_target = a;
                        }
                        arr[s2] = a;
                        lab[s2] = Label::Walk {
                            from: s,
                            minutes: m,
                        };
                        next.push(s2);
                    }
                }
            }
            next.sort_unstable();
            next.dedup();
            arrival.push(arr);
            label.push(lab);
            marked = next;
            let reached = to
                .iter()
                .copied()
                .filter(|&s| label[k][s] != Label::Unreached)
                .min_by_key(|&s| arrival[k][s]);
            if let Some(s) = reached {
                journeys.push(self.journey(&label, k, s, &buses, midnight));
            }
        }
        journeys
    }

    fn journey(
        &self,
        label: &[Vec<Label>],
        mut k: usize,
        mut s: usize,
        buses: &[(usize, NaiveDate, i64)],
        midnight: NaiveDateTime,
    ) -> Journey {
        let time = |m: i64| midnight + chrono::Duration::minutes(m);
        let mut legs = vec![];
        loop {
            match label[k][s] {
                Label::Origin | Label::Unreached => break,
                Label::Walk { from, minutes } => {
                    legs.push(Leg::Walk {
                        from,
                        to: s,
                        minutes,
                    });
                    s = from;
                }
                Label::Ride(b, on, off) => {
                    let (trip, service_date, shift) = buses[b];
                    let calls = &self.trips[trip].calls;
                    legs.push(Leg::Ride {
                        trip,
                        service_date,
                        from_call: on,
                        to_call: off,
                        from: calls[on].stop,
                        to: s,
                        depart: time(shift + i64::from(calls[on].departure)),
                        arrive: time(shift + i64::from(calls[off].arrival)),
                    });
                    s = calls[on].stop;
                    k -= 1;
                }
            }
        }
        legs.reverse();
        Journey { legs }
    }

    /// Up to `n` sensible journeys leaving at or after `at`, by departure:
    /// none leaves earlier and arrives later than another with no more changes.
    pub fn options(
        &self,
        from: &[usize],
        to: &[usize],
        at: NaiveDateTime,
        n: usize,
    ) -> Vec<Journey> {
        let mut all: Vec<Journey> = vec![];
        let mut t = at;
        for _ in 0..n * 4 {
            let found = self.plan(from, to, t);
            let Some(first) = found.iter().map(Journey::depart).min() else {
                break;
            };
            for j in found {
                if !all.contains(&j) {
                    all.push(j);
                }
            }
            t = first + chrono::Duration::minutes(1);
        }
        let worse = |j: &Journey, o: &Journey| {
            o.depart() >= j.depart()
                && o.arrive() <= j.arrive()
                && o.transfers() <= j.transfers()
                && (o.depart(), o.arrive(), o.transfers())
                    != (j.depart(), j.arrive(), j.transfers())
        };
        all.retain(|j| j.longest_wait() <= MAX_WAIT);
        let mut keep: Vec<Journey> = all
            .iter()
            .filter(|j| !all.iter().any(|o| worse(j, o)))
            .cloned()
            .collect();
        keep.sort_by_key(|j| (j.depart(), j.arrive(), j.transfers()));
        keep.dedup_by_key(|j| (j.depart(), j.arrive(), j.transfers()));
        keep.truncate(n);
        keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn at(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    const DAY: &str = "2026-11-24";

    fn stop(lat: f64, lon: f64) -> Stop {
        Stop {
            lat,
            lon,
            exact: true,
        }
    }

    fn trip(calls: &[(usize, &str)]) -> Trip {
        let min = |t: &str| {
            let (h, m) = t.split_once(':').unwrap();
            h.parse::<u32>().unwrap() * 60 + m.parse::<u32>().unwrap()
        };
        Trip {
            calls: calls
                .iter()
                .map(|(s, t)| Call {
                    stop: *s,
                    arrival: min(t),
                    departure: min(t),
                    board: true,
                    alight: true,
                })
                .collect(),
            service: 0,
        }
    }

    /// 0 Bešeňov → 1 Šurany → 2 Komárno; 3 Nové Zámky; 4 is 300 m from 1.
    fn stops() -> Vec<Stop> {
        vec![
            stop(47.845, 18.160),
            stop(47.870, 18.195),
            stop(47.763, 18.128),
            stop(47.985, 18.161),
            stop(47.8727, 18.195),
        ]
    }

    fn net(trips: Vec<Trip>) -> Network {
        Network::new(stops(), trips, vec![BTreeSet::from([d(DAY)])])
    }

    fn hm(t: NaiveDateTime) -> String {
        t.format("%H:%M").to_string()
    }

    fn summary(j: &Journey) -> (String, String, usize) {
        (hm(j.depart()), hm(j.arrive()), j.transfers())
    }

    #[test]
    fn the_earliest_direct_bus() {
        let n = net(vec![
            trip(&[(0, "06:20"), (1, "06:30"), (2, "07:00")]),
            trip(&[(0, "07:20"), (1, "07:30"), (2, "08:00")]),
        ]);
        let j = n.plan(&[0], &[2], at("2026-11-24 06:25"));
        assert_eq!(j.len(), 1);
        assert_eq!(summary(&j[0]), ("07:20".into(), "08:00".into(), 0));
    }

    #[test]
    fn a_change_at_a_shared_stop() {
        let n = net(vec![
            trip(&[(0, "06:20"), (1, "06:30")]),
            trip(&[(1, "06:40"), (3, "07:10")]),
        ]);
        let j = n.plan(&[0], &[3], at("2026-11-24 06:00"));
        assert_eq!(summary(&j[0]), ("06:20".into(), "07:10".into(), 1));
        assert!(matches!(j[0].legs[1], Leg::Ride { from: 1, to: 3, .. }));
    }

    #[test]
    fn a_change_needs_a_minute() {
        let n = net(vec![
            trip(&[(0, "06:20"), (1, "06:30")]),
            trip(&[(1, "06:30"), (3, "07:00")]), // leaves as we arrive: missed
            trip(&[(1, "06:50"), (3, "07:20")]),
        ]);
        let j = n.plan(&[0], &[3], at("2026-11-24 06:00"));
        assert_eq!(summary(&j[0]).1, "07:20");
    }

    #[test]
    fn walking_to_a_nearby_stop_to_change() {
        let n = net(vec![
            trip(&[(0, "06:20"), (1, "06:30")]),
            trip(&[(4, "06:40"), (3, "07:10")]),
        ]);
        let j = n.plan(&[0], &[3], at("2026-11-24 06:00"));
        assert_eq!(j.len(), 1);
        assert!(
            matches!(
                j[0].legs[1],
                Leg::Walk {
                    from: 1,
                    to: 4,
                    minutes: 5
                }
            ),
            "{:?}",
            j[0].legs
        );
    }

    #[test]
    fn walks_measured_along_the_streets_replace_the_straight_line_guess() {
        let mut n = net(vec![
            trip(&[(0, "06:20"), (1, "06:30")]),
            trip(&[(4, "06:40"), (3, "07:10")]),
        ]);
        // The footpath from 1 to 4 takes 8 minutes, and 1 to 2 is no walk at all.
        n.set_walks([(1, 4, 8)]);
        let j = n.plan(&[0], &[3], at("2026-11-24 06:00"));
        assert!(
            matches!(
                j[0].legs[1],
                Leg::Walk {
                    from: 1,
                    to: 4,
                    minutes: 8
                }
            ),
            "{:?}",
            j[0].legs
        );
        // Too far by the streets (a river between): no walk, so no journey.
        n.set_walks([]);
        assert!(n.plan(&[0], &[3], at("2026-11-24 06:00")).is_empty());
    }

    #[test]
    fn no_walking_from_a_stop_whose_position_is_a_guess() {
        let mut s = stops();
        s[4].exact = false;
        let n = Network::new(
            s,
            vec![
                trip(&[(0, "06:20"), (1, "06:30")]),
                trip(&[(4, "06:40"), (3, "07:10")]),
            ],
            vec![BTreeSet::from([d(DAY)])],
        );
        assert!(n.plan(&[0], &[3], at("2026-11-24 06:00")).is_empty());
    }

    #[test]
    fn fewer_changes_is_offered_when_it_arrives_later() {
        let n = net(vec![
            trip(&[(0, "06:20"), (1, "06:30")]),
            trip(&[(1, "06:35"), (3, "07:00")]),
            trip(&[(0, "06:40"), (1, "06:50"), (3, "07:30")]),
        ]);
        let j: Vec<_> = n
            .plan(&[0], &[3], at("2026-11-24 06:00"))
            .iter()
            .map(summary)
            .collect();
        assert_eq!(
            j,
            [
                ("06:40".into(), "07:30".into(), 0),
                ("06:20".into(), "07:00".into(), 1)
            ]
        );
    }

    #[test]
    fn no_boarding_where_the_bus_only_lets_people_off() {
        let mut t = trip(&[(0, "06:20"), (1, "06:30"), (2, "07:00")]);
        t.calls[1].board = false;
        let n = net(vec![t]);
        assert!(n.plan(&[1], &[2], at("2026-11-24 06:00")).is_empty());
        assert_eq!(n.plan(&[0], &[2], at("2026-11-24 06:00")).len(), 1);
    }

    #[test]
    fn a_trip_after_midnight_belongs_to_the_day_before() {
        // Runs on 24 Nov, reaches Komárno at 0:20 on the 25th.
        let n = net(vec![trip(&[(0, "23:50"), (2, "24:20")])]);
        let j = n.plan(&[0], &[2], at("2026-11-24 23:30"));
        assert_eq!(j[0].arrive(), at("2026-11-25 00:20"));
        let late = n.plan(&[2], &[2], at("2026-11-25 00:00"));
        assert!(late.is_empty());
    }

    #[test]
    fn nothing_on_a_day_without_service() {
        let n = net(vec![trip(&[(0, "06:20"), (2, "07:00")])]);
        assert!(n.plan(&[0], &[2], at("2026-11-25 05:00")).is_empty());
    }

    #[test]
    fn options_list_later_buses_and_drop_pointless_ones() {
        let n = net(vec![
            trip(&[(0, "06:20"), (2, "07:00")]),
            trip(&[(0, "06:00"), (1, "06:10")]), // waits 50 min at 1 for…
            trip(&[(1, "07:00"), (2, "07:30")]), // …a slower way: pointless
            trip(&[(0, "08:20"), (2, "09:00")]),
            trip(&[(0, "10:20"), (2, "11:00")]),
        ]);
        let o: Vec<_> = n
            .options(&[0], &[2], at("2026-11-24 05:30"), 2)
            .iter()
            .map(summary)
            .collect();
        assert_eq!(
            o,
            [
                ("06:20".into(), "07:00".into(), 0),
                ("08:20".into(), "09:00".into(), 0)
            ]
        );
    }
    #[test]
    fn any_platform_of_the_destination_station_will_do() {
        // 2 and 5 are platforms of one bus station; only 5 is served.
        let mut s = stops();
        s.push(stop(47.763, 18.128));
        let n = Network::new(
            s,
            vec![trip(&[(0, "06:20"), (5, "07:00")])],
            vec![BTreeSet::from([d(DAY)])],
        );
        let j = n.plan(&[0], &[2, 5], at("2026-11-24 06:00"));
        assert_eq!(summary(&j[0]), ("06:20".into(), "07:00".into(), 0));
    }

    #[test]
    fn platforms_of_one_station_are_linked_even_when_their_position_is_a_guess() {
        let mut s = stops();
        s.push(Stop {
            lat: 47.0,
            lon: 18.0,
            exact: false,
        });
        s[1].exact = false;
        let mut n = Network::new(
            s,
            vec![
                trip(&[(0, "06:20"), (1, "06:30")]),
                trip(&[(5, "06:40"), (3, "07:10")]),
            ],
            vec![BTreeSet::from([d(DAY)])],
        );
        n.link(1, 5, 3);
        let j = n.plan(&[0], &[3], at("2026-11-24 06:00"));
        assert!(
            matches!(
                j[0].legs[1],
                Leg::Walk {
                    from: 1,
                    to: 5,
                    minutes: 3
                }
            ),
            "{:?}",
            j[0].legs
        );
    }

    #[test]
    fn overnight_waits_are_not_offered() {
        let n = net(vec![
            trip(&[(0, "15:00"), (1, "15:10")]),
            trip(&[(1, "23:00"), (3, "23:30")]),
        ]);
        assert!(n.options(&[0], &[3], at("2026-11-24 14:00"), 3).is_empty());
    }

    #[test]
    fn a_platform_change_takes_the_station_s_time_even_if_the_platforms_coincide() {
        // Stop 5 is mapped on top of stop 1 (both at the station building).
        let mut s = stops();
        s.push(s[1].clone());
        let mut n = Network::new(
            s,
            vec![
                trip(&[(0, "06:20"), (1, "06:30")]),
                trip(&[(5, "06:32"), (3, "07:00")]), // only 2 min later: too tight
                trip(&[(5, "06:40"), (3, "07:10")]),
            ],
            vec![BTreeSet::from([d(DAY)])],
        );
        n.link(1, 5, 3);
        let j = n.plan(&[0], &[3], at("2026-11-24 06:00"));
        assert_eq!(summary(&j[0]).1, "07:10");
        assert!(matches!(j[0].legs[1], Leg::Walk { minutes: 3, .. }));
    }
}
