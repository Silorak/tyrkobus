//! Stage C1: understand legend texts. Notes that say when a trip runs become
//! calendar clauses; anything else is information we keep but do not compute with.

use crate::grid::SymbolKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// "premáva …" with dates: only within these dates (intersect).
    Runs,
    /// "premáva len …": only on these dates.
    RunsOnly,
    /// "premáva tiež …": additionally on these dates.
    AlsoRuns,
    /// "nepremáva …": never on these dates.
    DoesNotRun,
}

/// Day and month, no year: the validity period decides the year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DayMonth {
    pub day: u32,
    pub month: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateItem {
    Day(DayMonth),
    Range(DayMonth, DayMonth),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    Dates(Vec<DateItem>),
    DayTypes(Vec<SymbolKey>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    /// "spoj 6 a 25 …": the clause only applies to these trips.
    pub trips: Option<Vec<u32>>,
    pub kind: Kind,
    pub what: What,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meaning {
    Calendar(Vec<Clause>),
    /// Waits for a train, continues as another trip, request stop, …
    Info,
}

/// Parse a legend text. Errors only for texts that talk about running days
/// ("premáva") but do not fit the grammar: those trips must not be guessed.
pub fn parse(text: &str) -> Result<Meaning, String> {
    if !text.contains("premáva") {
        return Ok(Meaning::Info);
    }
    static VERB: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"nepremáva|premáva\s+len|premáva\s+tiež|premáva").unwrap()
    });
    static TRIPS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^spoj\s+([\d\s,a]+?)\s*$").unwrap());
    let mut clauses = vec![];
    // Bullets ("- spoj 6 …") and Hungarian translations (" - nem közlekedik …") are
    // dash-separated; dates and Slovak running-day phrases never contain a dash.
    for segment in text.split('-').filter(|s| s.contains("premáva")) {
        let verbs: Vec<regex::Match> = VERB.find_iter(segment).collect();
        let prefix = segment[..verbs[0].start()].trim();
        let trips = if prefix.is_empty() {
            None
        } else {
            let c = TRIPS
                .captures(prefix)
                .ok_or_else(|| format!("unexpected text {prefix:?}"))?;
            Some(
                c[1].split(|ch: char| !ch.is_ascii_digit())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.parse().unwrap())
                    .collect(),
            )
        };
        for (i, verb) in verbs.iter().enumerate() {
            let end = verbs.get(i + 1).map_or(segment.len(), |v| v.start());
            let args = segment[verb.end()..end].trim().trim_end_matches(',').trim();
            let kind = match verb.as_str().split_whitespace().last() {
                Some("nepremáva") => Kind::DoesNotRun,
                Some("len") => Kind::RunsOnly,
                Some("tiež") => Kind::AlsoRuns,
                _ => Kind::Runs,
            };
            let what = if args.starts_with("v ") || args.starts_with("vo ") {
                What::DayTypes(vec![
                    day_type(args).ok_or_else(|| format!("unknown day type {args:?}"))?,
                ])
            } else {
                What::Dates(dates(args)?)
            };
            clauses.push(Clause {
                trips: trips.clone(),
                kind,
                what,
            });
        }
    }
    if clauses.is_empty() {
        return Err(format!("no clause in {text:?}"));
    }
    Ok(Meaning::Calendar(clauses))
}

fn day_type(phrase: &str) -> Option<SymbolKey> {
    let p = phrase.trim();
    Some(match p {
        "v pracovných dňoch" => SymbolKey::WorkingDays,
        "v sobotu" => SymbolKey::Saturday,
        "v nedeľu" => SymbolKey::Sunday,
        "v pondelok" => SymbolKey::Weekday(1),
        "v utorok" => SymbolKey::Weekday(2),
        "v stredu" => SymbolKey::Weekday(3),
        "vo štvrtok" => SymbolKey::Weekday(4),
        "v piatok" => SymbolKey::Weekday(5),
        _ if p.starts_with("v nedeľu a v") && p.contains("sviat") => SymbolKey::SundayAndHolidays,
        _ => return None,
    })
}

/// "od 22.12. do 7.1.,2.2., … 29.10. a 30.10." → items. Anything besides dates,
/// ranges, commas and "a" is an error.
fn dates(args: &str) -> Result<Vec<DateItem>, String> {
    static ITEM: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        let d = r"(\d{1,2})\.\s*(\d{1,2})\.(?:\s*\d{4})?";
        regex::Regex::new(&format!(r"od\s*{d}\s*do\s*{d}|{d}")).unwrap()
    });
    let dm = |c: &regex::Captures, i: usize| -> Result<DayMonth, String> {
        let day: u32 = c[i].parse().map_err(|_| "bad day")?;
        let month: u32 = c[i + 1].parse().map_err(|_| "bad month")?;
        if (1..=31).contains(&day) && (1..=12).contains(&month) {
            Ok(DayMonth { day, month })
        } else {
            Err(format!("impossible date {day}.{month}."))
        }
    };
    let mut items = vec![];
    let mut rest = String::new();
    let mut last = 0;
    for c in ITEM.captures_iter(args) {
        let m = c.get(0).unwrap();
        rest.push_str(&args[last..m.start()]);
        last = m.end();
        items.push(if c.get(1).is_some() {
            DateItem::Range(dm(&c, 1)?, dm(&c, 3)?)
        } else {
            DateItem::Day(dm(&c, 5)?)
        });
    }
    rest.push_str(&args[last..]);
    let leftover: Vec<&str> = rest
        .split([',', ' '])
        .filter(|s| !s.is_empty() && *s != "a")
        .collect();
    if items.is_empty() || !leftover.is_empty() {
        return Err(format!(
            "cannot read dates in {args:?} (left: {leftover:?})"
        ));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dm(day: u32, month: u32) -> DayMonth {
        DayMonth { day, month }
    }

    fn one(text: &str) -> Clause {
        match parse(text).unwrap() {
            Meaning::Calendar(mut c) if c.len() == 1 => c.remove(0),
            other => panic!("{text:?} → {other:?}"),
        }
    }

    #[test]
    fn single_dates_and_ranges_mixed() {
        let c = one(
            "nepremáva od 22.12. do 7.1.,2.2., od 16.2. do 20.2., od 2.4. do 7.4., od 1.7. do 31.8.,29.10. a 30.10.",
        );
        assert_eq!(c.kind, Kind::DoesNotRun);
        assert_eq!(c.trips, None);
        assert_eq!(
            c.what,
            What::Dates(vec![
                DateItem::Range(dm(22, 12), dm(7, 1)),
                DateItem::Day(dm(2, 2)),
                DateItem::Range(dm(16, 2), dm(20, 2)),
                DateItem::Range(dm(2, 4), dm(7, 4)),
                DateItem::Range(dm(1, 7), dm(31, 8)),
                DateItem::Day(dm(29, 10)),
                DateItem::Day(dm(30, 10)),
            ])
        );
    }

    #[test]
    fn verb_variants() {
        assert_eq!(one("premáva od 1.7. do 31.8.").kind, Kind::Runs);
        assert_eq!(one("premáva len 24.12.,31.12. a 5.1.").kind, Kind::RunsOnly);
        assert_eq!(
            one("premáva tiež 29.12.,30.12.,31.12. a 2.1.").kind,
            Kind::AlsoRuns
        );
        assert_eq!(one("nepremáva 29.8.").kind, Kind::DoesNotRun);
    }

    #[test]
    fn day_type_text() {
        assert_eq!(
            one("premáva v pracovných dňoch").what,
            What::DayTypes(vec![SymbolKey::WorkingDays])
        );
        assert_eq!(
            one("premáva v nedeľu a vo štátom uznaných sviatkoch").what,
            What::DayTypes(vec![SymbolKey::SundayAndHolidays])
        );
        assert_eq!(
            one("premáva vo štvrtok").what,
            What::DayTypes(vec![SymbolKey::Weekday(4)])
        );
    }

    #[test]
    fn hungarian_translation_is_ignored() {
        let c = one("nepremáva od 22.12. do 7.1. - nem közlekedik 12.22 és 01.07. között");
        assert_eq!(
            c.what,
            What::Dates(vec![DateItem::Range(dm(22, 12), dm(7, 1))])
        );
        assert_eq!(
            one("premáva v sobotu - közlekedik szombaton").what,
            What::DayTypes(vec![SymbolKey::Saturday])
        );
    }

    #[test]
    fn several_clauses_in_one_note() {
        let Meaning::Calendar(c) = parse("premáva tiež 24.12. a 31.12., nepremáva 1.1.").unwrap()
        else {
            panic!()
        };
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].kind, c[1].kind), (Kind::AlsoRuns, Kind::DoesNotRun));
    }

    #[test]
    fn clauses_for_named_trips() {
        let Meaning::Calendar(c) =
            parse("- spoj 6 a 25 premáva od 1.7. do 31.8.- - spoj 25 premáva od 22.12. do 7.1.")
                .unwrap()
        else {
            panic!()
        };
        assert_eq!(c[0].trips, Some(vec![6, 25]));
        assert_eq!(c[1].trips, Some(vec![25]));
    }

    #[test]
    fn notes_without_running_days_are_info() {
        assert_eq!(
            parse("spoj 21, 23, 37 čaká na zmeškaný vlak najviac 10 minút"),
            Ok(Meaning::Info)
        );
        assert_eq!(
            parse("zastavuje len na znamenie alebo požiadanie"),
            Ok(Meaning::Info)
        );
    }

    #[test]
    fn running_day_text_that_does_not_fit_is_an_error() {
        assert!(parse("premáva v dňoch školského vyučovania").is_err());
        assert!(parse("nepremáva počas letných prázdnin").is_err());
    }
}
