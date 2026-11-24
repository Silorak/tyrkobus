//! Stage D: one line's timetable from one PDF: trips with stop times, running
//! dates and the PDF spot each time came from. Trips that cannot be trusted
//! are excluded with a reason, never guessed.

use crate::calendar::running_dates;
use crate::grid::{self, NoteKey, Table};
use crate::legend::{self, Meta};
use crate::notes::Meaning;
use crate::words::Word;
use chrono::NaiveDate;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct StopTime {
    pub stop: String,
    /// Minutes after midnight of the service day; can exceed 1440 after midnight.
    pub arrival: u32,
    pub departure: u32,
    pub request: bool,
    pub drop_off_only: bool,
    pub pick_up_only: bool,
    pub page: u32,
    pub bbox: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trip {
    pub number: u32,
    pub stop_times: Vec<StopTime>,
    pub dates: BTreeSet<NaiveDate>,
    /// Legend texts of notes that do not change dates ("čaká na zmeškaný vlak …").
    pub info: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub meta: Meta,
    pub trips: Vec<Trip>,
    pub excluded: Vec<(u32, String)>,
}

pub fn build(words: &[Word]) -> Result<Line, String> {
    let tables = grid::tables(words).map_err(|e| format!("{e:?}"))?;
    let meta = legend::meta(words).ok_or("no line number or validity in the page header")?;
    let legend = legend::legend(words, &tables);

    let mut by_number: BTreeMap<u32, Vec<(&Table, &grid::TripColumn)>> = BTreeMap::new();
    for t in &tables {
        for c in &t.trips {
            by_number.entry(c.number).or_default().push((t, c));
        }
    }
    let mut trips = vec![];
    let mut excluded = vec![];
    for (number, cols) in by_number {
        let timed: Vec<_> = cols.iter().filter(|(_, c)| !c.calls.is_empty()).collect();
        let (table, col) = match timed.as_slice() {
            [one] => **one,
            [] => {
                excluded.push((number, "no times printed".to_string()));
                continue;
            }
            _ => {
                excluded.push((number, "times printed in more than one table".to_string()));
                continue;
            }
        };
        let mut notes: Vec<NoteKey> = vec![];
        for (_, c) in &cols {
            for n in &c.notes {
                if !notes.contains(n) {
                    notes.push(*n);
                }
            }
        }
        let stop_times = match stop_times(table, col) {
            Ok(st) if st.len() >= 2 => st,
            Ok(_) => {
                excluded.push((number, "fewer than two stops".to_string()));
                continue;
            }
            Err(e) => {
                excluded.push((number, e));
                continue;
            }
        };
        let legend = legend.for_page(table.page);
        let dates = match running_dates(meta.valid_from, meta.valid_to, number, &notes, &legend) {
            Ok(d) if !d.is_empty() => d,
            Ok(_) => {
                excluded.push((number, "runs on no date in the validity period".to_string()));
                continue;
            }
            Err(e) => {
                excluded.push((number, e));
                continue;
            }
        };
        let info = notes
            .iter()
            .filter_map(|n| legend.get(n))
            .filter(|text| crate::notes::parse(text) == Ok(Meaning::Info))
            .cloned()
            .collect();
        trips.push(Trip {
            number,
            stop_times,
            dates,
            info,
        });
    }
    Ok(Line {
        meta,
        trips,
        excluded,
    })
}

fn stop_times(table: &Table, col: &grid::TripColumn) -> Result<Vec<StopTime>, String> {
    let mut out: Vec<StopTime> = vec![];
    let mut offset = 0;
    let mut prev: Option<(u32, usize)> = None;
    for call in &col.calls {
        let mut m = call.minutes + offset;
        let stop = &table.stops[call.row];
        if let Some((p, _)) = prev
            && m < p
        {
            if p - m > 720 {
                offset += 1440; // past midnight
                m += 1440;
            } else {
                return Err(format!(
                    "time goes backwards at {} ({}:{:02} after {}:{:02}) in the source PDF",
                    stop.name,
                    m / 60,
                    m % 60,
                    p / 60,
                    p % 60
                ));
            }
        }
        // Arrival and departure printed for the same stop: one stop time.
        if let (Some((_, row)), Some(last)) = (prev, out.last_mut())
            && row == call.row
        {
            last.departure = m;
            prev = Some((m, call.row));
            continue;
        }
        prev = Some((m, call.row));
        out.push(StopTime {
            stop: stop.name.clone(),
            arrival: m,
            departure: m,
            request: stop.request,
            drop_off_only: call.drop_off_only,
            pick_up_only: call.pick_up_only,
            page: table.page,
            bbox: call.bbox,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load(name: &str) -> Line {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        build(&crate::words::extract(&pdf).unwrap()).unwrap()
    }

    fn trip(l: &Line, n: u32) -> &Trip {
        l.trips.iter().find(|t| t.number == n).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn whole_line_builds_without_exclusions() {
        let l = load("401402.pdf");
        assert_eq!(l.meta.line, "401402");
        // 18 trips on page 1 (header "1 3 25 7 … 37 39"), 21 on page 2 ("4 2 8 34 … 40 44").
        assert_eq!(l.trips.len(), 18 + 21);
        assert!(l.excluded.is_empty(), "{:?}", l.excluded);
    }

    #[test]
    fn times_after_midnight_continue_past_24h() {
        let l = load("401402.pdf");
        // Rendered PDF, page 1, trip 39: leaves at 23 18, reaches Kravany n. Dunajom, Jednota at 0 11.
        let last = trip(&l, 39).stop_times.last().unwrap();
        assert_eq!(
            (last.stop.as_str(), last.arrival),
            ("Kravany n. Dunajom, Jednota", 24 * 60 + 11)
        );
    }

    #[test]
    fn trips_get_their_running_dates() {
        let l = load("401402.pdf");
        // Rendered PDF, page 1, trip 25: "⚒ 10" (working days; note 10: not 2.4.–7.4.).
        let t25 = &trip(&l, 25).dates;
        assert!(t25.contains(&d("2026-04-20"))); // Monday
        assert!(!t25.contains(&d("2026-04-07"))); // note 10
        assert!(!t25.contains(&d("2026-04-25"))); // Saturday
    }

    #[test]
    fn info_notes_are_kept_as_text() {
        let l = load("401402.pdf");
        // Rendered PDF, page 2: trip 26 is marked "△", "spoj 26 čaká v zastávke Kravany nad Dunajom,Jednota …".
        assert!(
            trip(&l, 26)
                .info
                .iter()
                .any(|i| i.contains("spoj 26 čaká v zastávke Kravany nad Dunajom,Jednota")),
            "{:?}",
            trip(&l, 26).info
        );
    }

    #[test]
    fn separate_arrival_and_departure_become_one_stop_time() {
        let l = load("402424.pdf");
        for t in &l.trips {
            let names: Vec<&str> = t.stop_times.iter().map(|s| s.stop.as_str()).collect();
            let mut dedup = names.clone();
            dedup.dedup();
            assert_eq!(
                names, dedup,
                "trip {} visits a stop twice in a row",
                t.number
            );
            assert!(t.stop_times.iter().all(|s| s.arrival <= s.departure));
        }
    }

    #[test]
    fn every_trip_has_two_stops_and_times_never_go_back() {
        for f in ["401402.pdf", "402424.pdf", "404449.pdf", "405405.pdf"] {
            for t in &load(f).trips {
                assert!(t.stop_times.len() >= 2, "{f} trip {}", t.number);
                assert!(
                    t.stop_times
                        .windows(2)
                        .all(|w| w[0].departure <= w[1].arrival),
                    "{f} trip {}",
                    t.number
                );
            }
        }
    }
}
