//! Stage B: rebuild timetable tables (stops × trips) from word positions.
//!
//! A table starts at a header line holding `km`, `Tč` and the trip numbers.
//! Trip numbers are column anchors; stop names (prefixed by the ordinary-stop
//! or request-stop glyph) are row anchors. Hours are big glyphs and minutes
//! small raised glyphs right next to them. Note numbers sit on a black box.

use crate::glyph::{Glyph, Symbol, decode, letters};
use crate::words::Word;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NoteKey {
    Symbol(SymbolKey),
    Number(u32),
}

/// `Symbol` without the parts that make no sense as a note key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolKey {
    WorkingDays,
    Saturday,
    Sunday,
    SundayAndHolidays,
    /// 1 = Monday … 5 = Friday
    Weekday(u8),
    RequestStop,
    DropOffOnly,
    PickUpOnly,
    Marker(char),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopRow {
    /// Stop position (Tč) printed left of the name.
    pub tc: Option<u32>,
    pub name: String,
    pub request: bool,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    /// Index into `Table::stops`.
    pub row: usize,
    /// Minutes after midnight as printed (0:05 is 5, not 1445).
    pub minutes: u32,
    pub drop_off_only: bool,
    pub pick_up_only: bool,
    /// Page coordinates of the hour+minute glyphs: provenance.
    pub bbox: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct TripColumn {
    pub number: u32,
    pub notes: Vec<NoteKey>,
    /// In row order (= travel order).
    pub calls: Vec<Call>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub page: u32,
    pub stops: Vec<StopRow>,
    pub trips: Vec<TripColumn>,
    /// Text found inside trip columns that is neither time, note nor dot.
    pub stray: Vec<String>,
    /// y range used by the table (header to last row); legends live outside it.
    pub top: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GridError {
    UnknownGlyph { page: u32, word: String },
    NoTables,
}

pub fn tables(words: &[Word]) -> Result<Vec<Table>, GridError> {
    let mut decoded = Vec::with_capacity(words.len());
    for w in words {
        let g = decode(&w.raw).map_err(|_| GridError::UnknownGlyph {
            page: w.page,
            word: w.raw.clone(),
        })?;
        let text = letters(&g);
        decoded.push(D { w, g, text });
    }
    let mut pages: Vec<u32> = words.iter().map(|w| w.page).collect();
    pages.dedup();
    let mut out = vec![];
    for page in pages {
        let pw: Vec<&D> = decoded.iter().filter(|d| d.w.page == page).collect();
        // A header line holds only `Tč`, optionally `km`, and trip numbers.
        // (Legend text like "čaká v TČ 17 na autobus" also contains TČ.)
        let mut headers: Vec<&D> = pw
            .iter()
            .copied()
            .filter(|d| {
                matches!(d.text.as_str(), "Tč" | "TČ") && {
                    let line: Vec<&&D> = pw
                        .iter()
                        .filter(|k| (k.w.yc() - d.w.yc()).abs() < 3.0)
                        .collect();
                    line.iter().any(|k| k.is_number())
                        && line.iter().all(|k| {
                            k.is_number()
                                || k.text.is_empty()
                                || matches!(k.text.as_str(), "km" | "Tč" | "TČ")
                        })
                }
            })
            .collect();
        headers.sort_by(|a, b| a.w.y0.total_cmp(&b.w.y0));
        for (i, h) in headers.iter().enumerate() {
            let y_end = headers.get(i + 1).map_or(f32::INFINITY, |n| n.w.y0);
            if let Some(t) = table(&pw, h, y_end) {
                out.push(t);
            }
        }
    }
    if out.is_empty() {
        return Err(GridError::NoTables);
    }
    Ok(out)
}

struct D<'a> {
    w: &'a Word,
    g: Vec<Glyph>,
    text: String,
}

impl D<'_> {
    fn is_number(&self) -> bool {
        !self.text.is_empty()
            && self
                .g
                .iter()
                .all(|g| matches!(g, Glyph::Char(c) if c.is_ascii_digit()))
    }
}

/// Column x-ranges for trip numbers centred at `centres` (sorted). The outer
/// edges reach at most `outer` past the outermost centres.
fn bounds(centres: &[f32], outer: f32) -> Vec<(f32, f32)> {
    let spacing = if centres.len() > 1 {
        let mut gaps: Vec<f32> = centres.windows(2).map(|w| w[1] - w[0]).collect();
        gaps.sort_by(f32::total_cmp);
        gaps[gaps.len() / 2]
    } else {
        f32::INFINITY
    };
    let edge = (spacing / 2.0).min(outer);
    (0..centres.len())
        .map(|i| {
            let c = centres[i];
            let left = if i == 0 {
                c - edge
            } else {
                (centres[i - 1] + c) / 2.0
            };
            let right = if i + 1 == centres.len() {
                c + edge
            } else {
                (c + centres[i + 1]) / 2.0
            };
            (left, right)
        })
        .collect()
}

fn table(pw: &[&D], h: &D, y_end: f32) -> Option<Table> {
    let hy = h.w.yc();
    let header: Vec<&D> = pw
        .iter()
        .copied()
        .filter(|d| (d.w.yc() - hy).abs() < 3.0)
        .collect();
    // Two-sided tables also have trips left of the km columns (or of Tč when there is no km).
    let left_edge = header
        .iter()
        .find(|d| d.text == "km")
        .map_or(h.w.x0, |k| k.w.x0);
    let numbers = |keep: &dyn Fn(&D) -> bool| -> Vec<(u32, f32)> {
        let mut v: Vec<(u32, f32)> = header
            .iter()
            .filter(|d| d.is_number() && keep(d))
            .map(|d| (d.text.parse().unwrap_or(0), d.w.xc()))
            .collect();
        v.sort_by(|a, b| a.1.total_cmp(&b.1));
        v
    };
    let right = numbers(&|d| d.w.x0 > h.w.x1 + 30.0);
    let left = numbers(&|d| d.w.x1 < left_edge - 5.0);
    if right.is_empty() && left.is_empty() {
        return None;
    }
    // A note group ("⚒ ■41") centred on the trip number spans ~1.3 glyph heights each side.
    let outer = 1.4 * h.w.height();
    let right_bounds = bounds(&right.iter().map(|c| c.1).collect::<Vec<_>>(), outer);
    let left_bounds = bounds(&left.iter().map(|c| c.1).collect::<Vec<_>>(), outer);
    let names_end = right_bounds.first().map_or(f32::INFINITY, |b| b.0);

    // Stop rows, anchored on the glyph that starts every stop name.
    let mut anchors: Vec<&D> = pw
        .iter()
        .copied()
        .filter(|d| {
            d.w.y0 > h.w.y1 - 1.0
                && d.w.yc() < y_end
                && d.w.x0 > h.w.x1
                && d.w.x1 < names_end
                && matches!(
                    d.g.first(),
                    Some(Glyph::Symbol(Symbol::OrdinaryStop | Symbol::RequestStop))
                )
        })
        .collect();
    anchors.sort_by(|a, b| a.w.yc().total_cmp(&b.w.yc()));
    anchors.dedup_by(|b, a| (a.w.yc() - b.w.yc()).abs() < 2.0);
    if anchors.is_empty() {
        return None;
    }
    let stops: Vec<StopRow> = anchors
        .iter()
        .map(|a| {
            let y = a.w.yc();
            let mut line: Vec<&D> = pw
                .iter()
                .copied()
                .filter(|d| {
                    (d.w.yc() - y).abs() < 2.5
                        && d.w.x0 >= a.w.x0 - 0.5
                        && d.w.x1 <= names_end + 1.0
                        && !d.text.is_empty()
                })
                .collect();
            line.sort_by(|p, q| p.w.x0.total_cmp(&q.w.x0));
            // Arrival/departure labels (`pr`, `od`) can follow the name on two-sided tables.
            while line
                .last()
                .is_some_and(|d| matches!(d.text.as_str(), "pr" | "od"))
            {
                line.pop();
            }
            let name = line
                .iter()
                .map(|d| d.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let name = name.replace(" ,", ",");
            let tc = pw
                .iter()
                .find(|d| {
                    (d.w.yc() - y).abs() < 2.5
                        && d.w.x1 <= a.w.x0 + 0.5
                        && d.w.x0 >= h.w.x0 - 12.0
                        && d.is_number()
                })
                .and_then(|d| d.text.parse().ok());
            StopRow {
                tc,
                name: name.trim().to_string(),
                request: matches!(a.g.first(), Some(Glyph::Symbol(Symbol::RequestStop))),
                y,
            }
        })
        .collect();

    // Times may sit on a separate arrival/departure line next to the name: allow
    // a bit over half the usual row spacing.
    let row_tolerance = if stops.len() > 1 {
        let mut gaps: Vec<f32> = stops.windows(2).map(|w| w[1].y - w[0].y).collect();
        gaps.sort_by(f32::total_cmp);
        0.6 * gaps[gaps.len() / 2]
    } else {
        5.0
    };
    let top = hy + 4.0;
    let bottom = (stops[stops.len() - 1].y + row_tolerance).min(y_end);
    let in_rows = |d: &&D| d.w.yc() > top && d.w.yc() < bottom;

    let mut stray = vec![];
    let mut trips = vec![];
    for (cols, bnds) in [(&left, &left_bounds), (&right, &right_bounds)] {
        for (&(number, _), &(l, r)) in cols.iter().zip(bnds.iter()) {
            let in_col: Vec<&D> = pw
                .iter()
                .copied()
                .filter(|d| in_rows(d) && d.w.xc() >= l && d.w.xc() < r)
                .collect();
            trips.push(column(number, &in_col, &stops, row_tolerance, &mut stray));
        }
    }

    Some(Table {
        page: h.w.page,
        top: h.w.y0,
        stops,
        trips,
        stray,
        bottom,
    })
}

fn column(
    number: u32,
    words: &[&D],
    stops: &[StopRow],
    row_tolerance: f32,
    stray: &mut Vec<String>,
) -> TripColumn {
    let mut used = vec![false; words.len()];
    let mut notes: Vec<(f32, f32, NoteKey)> = vec![];

    // Note numbers: digits drawn on a black box.
    for (i, b) in words.iter().enumerate() {
        if !b.g.contains(&Glyph::Symbol(Symbol::NoteBox)) {
            continue;
        }
        used[i] = true;
        let mut digits = b.text.clone();
        for (j, d) in words.iter().enumerate() {
            if !used[j]
                && d.is_number()
                && d.w.xc() >= b.w.x0 - 0.5
                && d.w.xc() <= b.w.x1 + 0.5
                && (d.w.yc() - b.w.yc()).abs() < 3.0
            {
                used[j] = true;
                digits.push_str(&d.text);
            }
        }
        match digits.parse() {
            Ok(n) => notes.push((b.w.yc(), b.w.x0, NoteKey::Number(n))),
            Err(_) => stray.push(format!("trip {number}: empty note box")),
        }
    }

    // Times: hour glyphs with smaller (~0.8×) raised minute glyphs directly to the right.
    // Font size differs between PDFs, so sizes are only compared, never absolute.
    let mut calls = vec![];
    for (i, hour) in words.iter().enumerate() {
        if used[i] || !hour.is_number() || hour.text.len() > 2 {
            continue;
        }
        let minute = words.iter().enumerate().find(|(j, m)| {
            !used[*j]
                && m.is_number()
                && m.text.len() == 2
                && m.w.height() < 0.9 * hour.w.height()
                && (m.w.x0 - hour.w.x1).abs() < 0.8
                && (m.w.yc() - hour.w.yc()).abs() < 3.0
        });
        let Some((j, minute)) = minute else { continue };
        let row = stops
            .iter()
            .enumerate()
            .map(|(r, s)| (r, (s.y - hour.w.yc()).abs()))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, dist)| *dist < row_tolerance)
            .map(|(r, _)| r);
        let Some(row) = row else { continue };
        used[i] = true;
        used[j] = true;
        let h: u32 = hour.text.parse().unwrap_or(0);
        let m: u32 = minute.text.parse().unwrap_or(0);
        calls.push(Call {
            row,
            minutes: h * 60 + m,
            drop_off_only: false,
            pick_up_only: false,
            bbox: [hour.w.x0, hour.w.y0, minute.w.x1, minute.w.y1],
        });
    }

    // Everything else: symbols become notes, the rest is stray.
    for (i, d) in words.iter().enumerate() {
        if used[i] {
            continue;
        }
        for g in &d.g {
            match g {
                Glyph::Symbol(Symbol::DropOffOnly | Symbol::PickUpOnly) => {
                    let near = calls
                        .iter_mut()
                        .filter(|c| (stops[c.row].y - d.w.yc()).abs() < row_tolerance)
                        .min_by(|a, b| a.bbox[0].total_cmp(&b.bbox[0]));
                    match (near, g) {
                        (Some(c), Glyph::Symbol(Symbol::DropOffOnly)) => c.drop_off_only = true,
                        (Some(c), _) => c.pick_up_only = true,
                        (None, Glyph::Symbol(s)) => {
                            notes.push((d.w.yc(), d.w.x0, NoteKey::Symbol(symbol_key(*s).unwrap())))
                        }
                        _ => {}
                    }
                }
                Glyph::Symbol(s) => {
                    if let Some(k) = symbol_key(*s) {
                        notes.push((d.w.yc(), d.w.x0, NoteKey::Symbol(k)));
                    } else if !matches!(s, Symbol::PassThrough) {
                        stray.push(format!("trip {number}: {s:?}"));
                    }
                }
                // A lone lowercase letter (`p`) is a footnote marker explained in the legend.
                Glyph::Char(c) if d.g.len() == 1 && c.is_ascii_lowercase() => {
                    notes.push((d.w.yc(), d.w.x0, NoteKey::Symbol(SymbolKey::Marker(*c))));
                }
                Glyph::Char(_) => {
                    stray.push(format!("trip {number}: {:?}", d.text));
                    break;
                }
            }
        }
    }

    // Reading order: group into lines (within 3 pt), then left to right.
    notes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut line_top = f32::NEG_INFINITY;
    for n in &mut notes {
        if n.0 - line_top >= 3.0 {
            line_top = n.0;
        }
        n.0 = line_top;
    }
    notes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut keys: Vec<NoteKey> = vec![];
    for (_, _, k) in notes {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    // Travel order: rows top-down, unless times mostly run backwards (a side read upwards).
    calls.sort_by_key(|c| (c.row, c.minutes));
    let forward = |a: u32, b: u32| (b + 1440 - a) % 1440 < 720;
    let (fwd, back) = calls.windows(2).fold((0, 0), |(f, b), w| {
        if w[0].minutes == w[1].minutes {
            (f, b)
        } else if forward(w[0].minutes, w[1].minutes) {
            (f + 1, b)
        } else {
            (f, b + 1)
        }
    });
    if back > fwd {
        // Read upwards: rows bottom-up, but arrival before departure within a row.
        calls.sort_by_key(|c| (std::cmp::Reverse(c.row), c.minutes));
    }
    TripColumn {
        number,
        notes: keys,
        calls,
    }
}

pub(crate) fn symbol_key(s: Symbol) -> Option<SymbolKey> {
    Some(match s {
        Symbol::WorkingDays => SymbolKey::WorkingDays,
        Symbol::Saturday => SymbolKey::Saturday,
        Symbol::Sunday => SymbolKey::Sunday,
        Symbol::SundayAndHolidays => SymbolKey::SundayAndHolidays,
        Symbol::OnWeekday(d) => SymbolKey::Weekday(d.number_from_monday() as u8),
        Symbol::RequestStop => SymbolKey::RequestStop,
        Symbol::DropOffOnly => SymbolKey::DropOffOnly,
        Symbol::PickUpOnly => SymbolKey::PickUpOnly,
        Symbol::Marker(c) => SymbolKey::Marker(c),
        Symbol::PassThrough | Symbol::NoteBox | Symbol::OrdinaryStop => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture() -> Vec<Table> {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/401402.pdf");
        tables(&crate::words::extract(&pdf).unwrap()).unwrap()
    }

    fn hm(t: &str) -> u32 {
        let (h, m) = t.split_once(':').unwrap();
        h.parse::<u32>().unwrap() * 60 + m.parse::<u32>().unwrap()
    }

    fn times(table: &Table, trip: u32) -> Vec<(String, u32)> {
        let t = table.trips.iter().find(|t| t.number == trip).unwrap();
        t.calls
            .iter()
            .map(|c| (table.stops[c.row].name.clone(), c.minutes))
            .collect()
    }

    #[test]
    fn one_table_per_direction_page() {
        let t = fixture();
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].page, t[1].page), (1, 2));
    }

    #[test]
    fn trip_numbers_are_read_left_to_right() {
        let numbers: Vec<u32> = fixture()[0].trips.iter().map(|t| t.number).collect();
        // Rendered PDF, page 1 header.
        assert_eq!(
            numbers,
            [
                1, 3, 25, 7, 11, 13, 15, 17, 19, 5, 21, 27, 23, 9, 33, 29, 37, 39
            ]
        );
    }

    #[test]
    fn stops_have_position_name_and_request_flag() {
        let t = &fixture()[0];
        // Rendered PDF, page 1: Tč 1–36, with the Marcelová loop printed as
        // Tč 16, 17, 18, 17, 16 (rázc. → trafostanica → Jednota → back).
        assert_eq!(t.stops.len(), 38);
        let first = &t.stops[0];
        assert_eq!(
            (first.tc, first.name.as_str(), first.request),
            (Some(1), "Komárno, ARRIVA", false)
        );
        let dulov = &t.stops[5];
        assert_eq!(
            (dulov.tc, dulov.name.as_str(), dulov.request),
            (Some(6), "Komárno, Ďulov dvor", true)
        );
        let tc: Vec<Option<u32>> = t.stops[15..20].iter().map(|s| s.tc).collect();
        assert_eq!(tc, [Some(16), Some(17), Some(18), Some(17), Some(16)]);
        assert_eq!(t.stops[17].name, "Marcelová, Jednota");
        assert_eq!(t.stops[37].name, "Nová Vieska, ŽST");
    }

    #[test]
    fn trip_notes_come_from_the_cells_above_the_first_time() {
        let t = &fixture()[0];
        let notes = |n: u32| {
            t.trips
                .iter()
                .find(|t| t.number == n)
                .unwrap()
                .notes
                .clone()
        };
        use NoteKey::*;
        // Rendered PDF, page 1: trip 25 "⚒ 10", trip 9 "⑥ 40", trip 23 "⚒ †".
        assert_eq!(notes(25), vec![Symbol(SymbolKey::WorkingDays), Number(10)]);
        assert_eq!(notes(9), vec![Symbol(SymbolKey::Saturday), Number(40)]);
        assert_eq!(
            notes(23),
            vec![
                Symbol(SymbolKey::WorkingDays),
                Symbol(SymbolKey::SundayAndHolidays)
            ]
        );
    }

    #[test]
    fn hours_and_raised_minutes_are_joined() {
        let t = &fixture()[0];
        // Rendered PDF, page 1, trip 11: SLK 7 50, AS 8 00.
        let trip11 = times(t, 11);
        assert_eq!(trip11[0], ("Komárno, SLK a.s.".to_string(), hm("7:50")));
        assert_eq!(trip11[1], ("Komárno, AS".to_string(), hm("8:00")));
    }

    #[test]
    fn stops_a_trip_skips_have_no_call() {
        let t = &fixture()[0];
        // Rendered PDF, page 1, trip 7 passes Komárno, Rieker obuv (Tč 3) without stopping.
        let trip7: Vec<_> = times(t, 7).into_iter().take(3).collect();
        assert_eq!(
            trip7,
            vec![
                ("Komárno, ARRIVA".to_string(), hm("6:25")),
                ("Komárno, Kaufland".to_string(), hm("6:26")),
                ("Komárno, SLK a.s.".to_string(), hm("6:30")),
            ]
        );
    }

    #[test]
    fn a_trip_can_start_partway_along_the_route() {
        let t = &fixture()[0];
        // Rendered PDF, page 1, trip 3: Bátorove Kosihy, hl.nám. 6 05 → Nová Vieska, ŽST 6 15.
        let trip3 = times(t, 3);
        assert_eq!(
            trip3.first().unwrap(),
            &("Bátorove Kosihy, hl.nám.".to_string(), hm("6:05"))
        );
        assert_eq!(
            trip3.last().unwrap(),
            &("Nová Vieska, ŽST".to_string(), hm("6:15"))
        );
        assert_eq!(trip3.len(), 5);
    }

    #[test]
    fn times_after_midnight_are_kept_as_printed() {
        let t = &fixture()[0];
        // Rendered PDF, page 1, trip 39: Kravany n. Dunajom, Jednota 0 11.
        let last = times(t, 39).pop().unwrap();
        assert_eq!(
            last,
            ("Kravany n. Dunajom, Jednota".to_string(), hm("0:11"))
        );
    }

    #[test]
    fn reverse_direction_page_reads_top_to_bottom() {
        let t = &fixture()[1];
        // Rendered PDF, page 2 ("Opačný smer"): trip 14 leaves Nová Vieska, ŽST at 6 45.
        assert_eq!(t.stops[0].name, "Nová Vieska, ŽST");
        assert_eq!(
            times(t, 14)[0],
            ("Nová Vieska, ŽST".to_string(), hm("6:45"))
        );
        assert!(t.stray.is_empty(), "stray: {:?}", t.stray);
    }

    #[test]
    fn every_call_points_at_a_stop_and_times_run_forward() {
        for table in fixture() {
            for trip in &table.trips {
                assert!(trip.calls.iter().all(|c| c.row < table.stops.len()));
                assert!(
                    trip.calls
                        .windows(2)
                        .all(|w| (w[1].minutes + 1440 - w[0].minutes) % 1440 < 720),
                    "trip {}",
                    trip.number
                );
            }
        }
    }

    fn load(name: &str) -> Vec<Table> {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        tables(&crate::words::extract(&pdf).unwrap()).unwrap()
    }

    #[test]
    fn two_sided_table_reads_each_side_in_travel_order() {
        // 404449: trip 1 left of km/Tč (read downwards), trip 2 right of the names (read upwards).
        let t = &load("404449.pdf")[0];
        let numbers: Vec<u32> = t.trips.iter().map(|t| t.number).collect();
        assert_eq!(numbers, [1, 2]);
        let trip1 = times(t, 1);
        assert_eq!(trip1[0], ("Štúrovo, AS".to_string(), hm("5:40")));
        let trip2 = times(t, 2);
        assert_eq!(
            trip2.last().unwrap(),
            &("Štúrovo, AS".to_string(), hm("17:25"))
        );
        assert!(trip1.len() > 10 && trip2.len() > 10);
        assert!(t.stray.is_empty(), "stray: {:?}", t.stray);
        assert!(
            t.stops
                .iter()
                .all(|s| !s.name.ends_with(" pr") && !s.name.ends_with(" od"))
        );
    }

    #[test]
    fn departure_row_below_the_stop_name_belongs_to_that_stop() {
        // 402424: Levice, AS has separate arrival (pr) and departure (od) lines.
        let t = &load("402424.pdf")[0];
        assert_eq!(times(t, 1)[0], ("Levice, AS".to_string(), hm("5:20")));
        assert!(t.stray.is_empty(), "stray: {:?}", t.stray);
    }
}
