//! Stage C2: exact running dates of a trip from its notes, the legend and the
//! Slovak days-off calendar.

use crate::grid::{NoteKey, SymbolKey};
use crate::notes::{Clause, DateItem, DayMonth, Kind, Meaning, What};
use chrono::{Datelike, NaiveDate, Weekday};
use std::collections::{BTreeMap, BTreeSet};

/// Days off work ("dni pracovného pokoja") under Act 241/1993 as amended.
/// Source: Národná banka Slovenska, https://nbs.sk/en/about-the-bank/holidays-in-slovakia/
/// (checked 23 Nov 2026). 2026 is special: 8 May and 15 Sep are working days.
/// 1 Sep, 28 Oct and 17 Nov are state holidays but not days off.
const DAYS_OFF: &[(i32, &[(u32, u32)])] = &[
    (
        2025,
        &[
            (1, 1),
            (1, 6),
            (4, 18),
            (4, 21),
            (5, 1),
            (5, 8),
            (7, 5),
            (8, 29),
            (9, 15),
            (11, 1),
            (12, 24),
            (12, 25),
            (12, 26),
        ],
    ),
    (
        2026,
        &[
            (1, 1),
            (1, 6),
            (4, 3),
            (4, 6),
            (5, 1),
            (7, 5),
            (8, 29),
            (11, 1),
            (12, 24),
            (12, 25),
            (12, 26),
        ],
    ),
    (
        2027,
        &[
            (1, 1),
            (1, 6),
            (3, 26),
            (3, 29),
            (5, 1),
            (5, 8),
            (7, 5),
            (8, 29),
            (9, 15),
            (11, 1),
            (12, 24),
            (12, 25),
            (12, 26),
        ],
    ),
];

/// `None` when the year is not in the table: we must not guess holidays.
pub fn is_day_off(d: NaiveDate) -> Option<bool> {
    let (_, days) = DAYS_OFF.iter().find(|(y, _)| *y == d.year())?;
    Some(days.contains(&(d.month(), d.day())))
}

fn matches_day_type(key: SymbolKey, d: NaiveDate, day_off: bool) -> bool {
    let wd = d.weekday();
    match key {
        SymbolKey::WorkingDays => wd.number_from_monday() <= 5 && !day_off,
        SymbolKey::Saturday => wd == Weekday::Sat,
        SymbolKey::Sunday => wd == Weekday::Sun,
        SymbolKey::SundayAndHolidays => wd == Weekday::Sun || day_off,
        SymbolKey::Weekday(n) => wd.number_from_monday() == u32::from(n),
        _ => false,
    }
}

fn is_day_type(key: SymbolKey) -> bool {
    matches!(
        key,
        SymbolKey::WorkingDays
            | SymbolKey::Saturday
            | SymbolKey::Sunday
            | SymbolKey::SundayAndHolidays
            | SymbolKey::Weekday(_)
    )
}

fn in_item(item: &DateItem, d: NaiveDate) -> bool {
    let x = DayMonth {
        day: d.day(),
        month: d.month(),
    };
    let key = |m: &DayMonth| (m.month, m.day);
    match item {
        DateItem::Day(m) => *m == x,
        DateItem::Range(a, b) if key(a) <= key(b) => key(a) <= key(&x) && key(&x) <= key(b),
        // Wraps over New Year: "od 22.12. do 7.1."
        DateItem::Range(a, b) => key(&x) >= key(a) || key(&x) <= key(b),
    }
}

/// Every date in `from..=to` on which trip `number` runs.
///
/// running = ((day types, or every day) ∩ "premáva"/"premáva len" dates)
///           ∪ "premáva tiež" dates − "nepremáva" dates
pub fn running_dates(
    from: NaiveDate,
    to: NaiveDate,
    number: u32,
    notes: &[NoteKey],
    legend: &BTreeMap<NoteKey, String>,
) -> Result<BTreeSet<NaiveDate>, String> {
    let mut day_types: Vec<SymbolKey> = vec![];
    let mut clauses: Vec<Clause> = vec![];
    for key in notes {
        if let NoteKey::Symbol(s) = key
            && is_day_type(*s)
        {
            day_types.push(*s);
            continue;
        }
        let Some(text) = legend.get(key) else {
            if matches!(key, NoteKey::Number(_)) {
                return Err(format!("note {key:?} is not explained in the legend"));
            }
            continue; // request stop etc. without legend line: no effect on dates
        };
        match crate::notes::parse(text).map_err(|e| format!("note {key:?}: {e}"))? {
            Meaning::Info => {}
            Meaning::Calendar(cs) => clauses.extend(
                cs.into_iter()
                    .filter(|c| c.trips.as_ref().is_none_or(|t| t.contains(&number))),
            ),
        }
    }
    for c in &clauses {
        if let What::DayTypes(k) = &c.what {
            match c.kind {
                Kind::Runs | Kind::RunsOnly => day_types.extend(k),
                _ => return Err(format!("unsupported day-type clause {c:?}")),
            }
        }
    }
    let dated = |kind: &[Kind]| -> Vec<&DateItem> {
        clauses
            .iter()
            .filter(|c| kind.contains(&c.kind))
            .flat_map(|c| match &c.what {
                What::Dates(items) => items.iter().collect(),
                What::DayTypes(_) => vec![],
            })
            .collect()
    };
    let restrict = dated(&[Kind::Runs, Kind::RunsOnly]);
    let also = dated(&[Kind::AlsoRuns]);
    let except = dated(&[Kind::DoesNotRun]);

    let mut out = BTreeSet::new();
    for d in from.iter_days().take_while(|d| *d <= to) {
        let day_off = is_day_off(d).ok_or_else(|| format!("no days-off data for {}", d.year()))?;
        let base =
            day_types.is_empty() || day_types.iter().any(|k| matches_day_type(*k, d, day_off));
        let within = restrict.is_empty() || restrict.iter().any(|i| in_item(i, d));
        let runs = (base && within) || also.iter().any(|i| in_item(i, d));
        if runs && !except.iter().any(|i| in_item(i, d)) {
            out.insert(d);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use NoteKey::{Number, Symbol};
    use SymbolKey::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn legend() -> BTreeMap<NoteKey, String> {
        // Note texts from 401402.pdf (10 and 41 on page 1; 12 and the
        // first line of the "△" marker on page 2); the period is the whole
        // 2025/26 timetable year.
        [
            (Number(10), "nepremáva od 22.12. do 7.1.,2.2., od 16.2. do 20.2., od 2.4. do 7.4., od 1.7. do 31.8.,29.10. a 30.10."),
            (Number(12), "premáva od 1.7. do 31.8."),
            (Number(41), "nepremáva 29.12.,30.12.,31.12. a 2.1."),
            (Symbol(Marker('«')), "- spoj 26 čaká v zastávke Kravany nad Dunajom,Jednota na príchod spoja č.16 linky 401403"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_string()))
        .collect()
    }

    fn runs(notes: &[NoteKey]) -> BTreeSet<NaiveDate> {
        running_dates(d("2025-12-14"), d("2026-12-12"), 3, notes, &legend()).unwrap()
    }

    #[test]
    fn working_days_skip_weekends_and_days_off() {
        let r = runs(&[Symbol(WorkingDays)]);
        assert!(r.contains(&d("2025-12-15"))); // Monday
        assert!(!r.contains(&d("2025-12-20"))); // Saturday
        assert!(!r.contains(&d("2025-12-24"))); // Christmas Eve, Wednesday
        assert!(!r.contains(&d("2026-04-06"))); // Easter Monday
        assert!(r.contains(&d("2026-05-08"))); // not a day off in 2026
        assert!(r.contains(&d("2026-09-15"))); // not a day off in 2026
        assert!(r.contains(&d("2026-11-17"))); // state holiday, but a working day
    }

    #[test]
    fn sunday_and_holidays_include_weekday_holidays() {
        let r = runs(&[Symbol(Saturday), Symbol(SundayAndHolidays)]);
        assert!(r.contains(&d("2025-12-20")) && r.contains(&d("2025-12-21")));
        assert!(r.contains(&d("2025-12-24")));
        assert!(!r.contains(&d("2025-12-22")));
    }

    #[test]
    fn does_not_run_removes_dates_and_ranges_across_new_year() {
        let r = runs(&[Symbol(WorkingDays), Number(10)]);
        assert!(r.contains(&d("2025-12-15")));
        assert!(!r.contains(&d("2025-12-22")));
        assert!(!r.contains(&d("2026-01-07")));
        assert!(r.contains(&d("2026-01-08")));
        assert!(!r.contains(&d("2026-07-15")));
    }

    #[test]
    fn runs_restricts_to_the_listed_period() {
        let r = runs(&[Symbol(WorkingDays), Number(12)]);
        assert_eq!(r.first(), Some(&d("2026-07-01")));
        assert_eq!(r.last(), Some(&d("2026-08-31")));
        assert!(!r.contains(&d("2026-07-04"))); // Saturday
    }

    #[test]
    fn no_day_type_means_every_day() {
        let r = runs(&[]);
        assert_eq!(r.len(), 364);
        assert_eq!(r.first(), Some(&d("2025-12-14")));
        assert_eq!(r.last(), Some(&d("2026-12-12")));
    }

    #[test]
    fn info_notes_do_not_change_dates() {
        assert_eq!(
            runs(&[Symbol(WorkingDays), Symbol(Marker('«'))]),
            runs(&[Symbol(WorkingDays)])
        );
    }

    #[test]
    fn unexplained_note_is_an_error() {
        let err = running_dates(
            d("2025-12-14"),
            d("2026-12-12"),
            3,
            &[Number(52)],
            &legend(),
        );
        assert!(err.is_err());
    }

    #[test]
    fn clause_for_other_trips_is_ignored() {
        let mut l = legend();
        l.insert(Number(60), "- spoj 25 premáva od 1.7. do 31.8.".into());
        let mine = running_dates(
            d("2025-12-14"),
            d("2026-12-12"),
            3,
            &[Symbol(WorkingDays), Number(60)],
            &l,
        )
        .unwrap();
        assert_eq!(mine, runs(&[Symbol(WorkingDays)]));
        let t25 = running_dates(
            d("2025-12-14"),
            d("2026-12-12"),
            25,
            &[Symbol(WorkingDays), Number(60)],
            &l,
        )
        .unwrap();
        assert_eq!(t25.first(), Some(&d("2026-07-01")));
    }

    #[test]
    fn years_without_holiday_data_are_an_error() {
        assert!(
            running_dates(
                d("2029-01-01"),
                d("2029-02-01"),
                1,
                &[Symbol(WorkingDays)],
                &legend()
            )
            .is_err()
        );
    }
}
