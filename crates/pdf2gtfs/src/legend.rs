//! Stage B2: what a timetable PDF says about itself: line, name, validity,
//! and the legend that explains each note key.

use crate::glyph::{Glyph, Symbol, decode, letters};
use crate::grid::{NoteKey, SymbolKey, Table};
use crate::words::Word;
use chrono::NaiveDate;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// Six-digit line number, e.g. "401401".
    pub line: String,
    /// Printed line name, e.g. "Komárno-Štúrovo".
    pub name: String,
    pub valid_from: NaiveDate,
    pub valid_to: NaiveDate,
}

pub fn meta(words: &[Word]) -> Option<Meta> {
    let page1: Vec<Tok> = toks(words.iter().filter(|w| w.page == 1));
    // The line number is the large 6-digit word at the top ("linky 401403" in notes is smaller).
    let number = page1
        .iter()
        .filter(|t| t.text.len() == 6 && t.is_number())
        .max_by(|a, b| {
            a.w.height()
                .total_cmp(&b.w.height())
                .then(b.w.y0.total_cmp(&a.w.y0))
        })?;
    let mut name_words: Vec<&Tok> = page1
        .iter()
        .filter(|t| (t.w.yc() - number.w.yc()).abs() < 3.0 && t.w.x0 > number.w.x1)
        .collect();
    name_words.sort_by(|a, b| a.w.x0.total_cmp(&b.w.x0));
    let name = name_words
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    let top: Vec<Vec<&Tok>> = lines(
        page1
            .iter()
            .filter(|t| t.w.yc() < number.w.yc() - 2.0)
            .collect(),
    );
    let text = top
        .iter()
        .map(|l| {
            l.iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ");
    static VALID: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"od\s+(?:\(\S+\)\s+)?(\d{1,2})\.\s*(\p{L}+)\s+(\d{4}).*?do\s+(?:\(\S+\)\s+)?(\d{1,2})\.\s*(\p{L}+)\s+(\d{4})",
        )
        .unwrap()
    });
    let c = VALID.captures(&text)?;
    let date = |d: usize, m: usize, y: usize| -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(c[y].parse().ok()?, month(&c[m])?, c[d].parse().ok()?)
    };
    Some(Meta {
        line: number.text.clone(),
        name,
        valid_from: date(1, 2, 3)?,
        valid_to: date(4, 5, 6)?,
    })
}

/// Slovak month name in the genitive ("14. decembra").
fn month(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "januára",
        "februára",
        "marca",
        "apríla",
        "mája",
        "júna",
        "júla",
        "augusta",
        "septembra",
        "októbra",
        "novembra",
        "decembra",
    ];
    MONTHS.iter().position(|m| *m == name).map(|i| i as u32 + 1)
}

/// Legend entries per page: note key → explanation text (letters decoded, words
/// joined by spaces). Markers like `«` can mean different things on each page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Legend {
    pub pages: BTreeMap<u32, BTreeMap<NoteKey, String>>,
}

impl Legend {
    /// Entries that apply to trips on `page`: that page's own legend first,
    /// then entries printed on other pages (numbered notes are often listed once).
    pub fn for_page(&self, page: u32) -> BTreeMap<NoteKey, String> {
        let mut out = self.pages.get(&page).cloned().unwrap_or_default();
        for entries in self.pages.values() {
            for (k, v) in entries {
                out.entry(*k).or_insert_with(|| v.clone());
            }
        }
        out
    }
}

pub fn legend(words: &[Word], tables: &[Table]) -> Legend {
    let mut out = Legend::default();
    let mut pages: Vec<u32> = words.iter().map(|w| w.page).collect();
    pages.dedup();
    for page in pages {
        let bottom = tables
            .iter()
            .filter(|t| t.page == page)
            .map(|t| (t.top, t.bottom))
            .collect::<Vec<_>>();
        // Legends can sit below the last table or between stacked tables.
        let below = toks(words.iter().filter(|w| {
            w.page == page
                && !bottom.is_empty()
                && w.yc() > bottom.iter().map(|b| b.0).fold(f32::INFINITY, f32::min)
                && !bottom
                    .iter()
                    .any(|&(top, bot)| w.yc() >= top - 1.0 && w.yc() <= bot)
        }));
        let mut current: Option<(NoteKey, f32)> = None;
        let mut texts: Vec<(NoteKey, String)> = vec![];
        for line in lines(below.iter().collect()) {
            let mut i = 0;
            let mut first = true;
            while i < line.len() {
                if let Some((key, used)) = key_at(&line, i, first) {
                    current = Some((key, line[i].w.x0));
                    texts.push((key, String::new()));
                    i += used;
                } else {
                    let tok = line[i];
                    // Unkeyed line start: continuation only when indented past the keys.
                    if first && current.is_some_and(|(_, x)| tok.w.x0 <= x + 5.0) {
                        current = None;
                    }
                    if let (Some(_), Some(last)) = (current, texts.last_mut())
                        && !tok.text.is_empty()
                    {
                        if !last.1.is_empty() {
                            last.1.push(' ');
                        }
                        last.1.push_str(&tok.text);
                    }
                    i += 1;
                }
                first = false;
            }
        }
        let page_entries = out.pages.entry(page).or_default();
        for (k, t) in texts {
            page_entries.entry(k).or_insert(t);
        }
    }
    out
}

struct Tok<'a> {
    w: &'a Word,
    g: Vec<Glyph>,
    text: String,
}

impl Tok<'_> {
    fn is_number(&self) -> bool {
        !self.text.is_empty() && self.text.chars().all(|c| c.is_ascii_digit()) && {
            self.g.iter().all(|g| matches!(g, Glyph::Char(_)))
        }
    }
}

fn toks<'a>(words: impl Iterator<Item = &'a Word>) -> Vec<Tok<'a>> {
    words
        .filter_map(|w| {
            let g = decode(&w.raw).ok()?;
            let text = letters(&g);
            Some(Tok { w, g, text })
        })
        .collect()
}

/// Group into text lines (centres within 2.5 pt), each sorted left to right.
fn lines<'a, 'b>(mut toks: Vec<&'b Tok<'a>>) -> Vec<Vec<&'b Tok<'a>>> {
    toks.sort_by(|a, b| a.w.yc().total_cmp(&b.w.yc()));
    let mut out: Vec<Vec<&Tok>> = vec![];
    for t in toks {
        match out.last_mut() {
            Some(line) if (t.w.yc() - line[0].w.yc()).abs() < 2.5 => line.push(t),
            _ => out.push(vec![t]),
        }
    }
    for line in &mut out {
        line.sort_by(|a, b| a.w.x0.total_cmp(&b.w.x0));
    }
    out
}

/// A legend key starting at `line[i]`, and how many tokens it uses.
fn key_at(line: &[&Tok], i: usize, line_start: bool) -> Option<(NoteKey, usize)> {
    let t = line[i];
    if t.g.contains(&Glyph::Symbol(Symbol::NoteBox)) {
        let mut digits = t.text.clone();
        let mut used = 1;
        while let Some(d) = line.get(i + used) {
            if d.is_number() && d.w.xc() >= t.w.x0 - 0.5 && d.w.xc() <= t.w.x1 + 0.5 {
                digits.push_str(&d.text);
                used += 1;
            } else {
                break;
            }
        }
        return digits.parse().ok().map(|n| (NoteKey::Number(n), used));
    }
    if let [Glyph::Symbol(s)] = t.g.as_slice() {
        return crate::grid::symbol_key(*s).map(|k| (NoteKey::Symbol(k), 1));
    }
    // Letter keys (`x` request stop, `p` …) only at line start: "a" also means "and".
    if line_start
        && let [Glyph::Char(c)] = t.g.as_slice()
        && c.is_ascii_lowercase()
    {
        let key = if *c == 'x' {
            SymbolKey::RequestStop
        } else {
            SymbolKey::Marker(*c)
        };
        return Some((NoteKey::Symbol(key), 1));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load(name: &str) -> (Vec<Word>, Vec<Table>) {
        let pdf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let w = crate::words::extract(&pdf).unwrap();
        let t = crate::grid::tables(&w).unwrap();
        (w, t)
    }

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn line_name_and_validity_come_from_the_page_header() {
        let (w, _) = load("401402.pdf");
        // Rendered PDF, page 1 header: "Platí od 1. apríla 2026 do 12. decembra 2026".
        assert_eq!(
            meta(&w),
            Some(Meta {
                line: "401402".into(),
                name: "Komárno-Kravany nad Dunajom-Nová Vieska".into(),
                valid_from: date("2026-04-01"),
                valid_to: date("2026-12-12"),
            })
        );
    }

    #[test]
    fn mid_year_validity_is_read_too() {
        let (w, _) = load("404449.pdf");
        let m = meta(&w).unwrap();
        assert_eq!(
            (m.line.as_str(), m.valid_from),
            ("404449", date("2026-06-22"))
        );
    }

    #[test]
    fn numbered_notes_are_read_from_their_boxes() {
        let (w, t) = load("401402.pdf");
        let l = legend(&w, &t).for_page(1);
        // Rendered PDF, page 1 legend.
        assert_eq!(
            l[&NoteKey::Number(10)],
            "nepremáva od 22.12. do 7.1.,2.2., od 16.2. do 20.2., od 2.4. do 7.4., od 1.7. do 31.8.,29.10. a 30.10."
        );
        assert_eq!(
            l[&NoteKey::Number(41)],
            "nepremáva 29.12.,30.12.,31.12. a 2.1."
        );
        assert_eq!(
            l[&NoteKey::Number(11)],
            "premáva od 22.12. do 7.1.,2.2., od 16.2. do 20.2., od 2.4. do 7.4., od 1.7. do 31.8.,29.10. a 30.10."
        );
        assert_eq!(l[&NoteKey::Number(40)], "nepremáva 29.8.");
    }

    #[test]
    fn markers_mean_what_their_own_page_says() {
        // 401402.pdf prints its marker on page 2 only, so this uses another line.
        let (w, t) = load("405405.pdf");
        let l = legend(&w, &t);
        let m = NoteKey::Symbol(SymbolKey::Marker('«'));
        // Rendered PDF, page 1 and page 2 legends ("△" lines).
        assert_eq!(
            l.for_page(1)[&m],
            "spoj 9 je vedený do zastávky Galanta,nemocnica"
        );
        assert!(l.for_page(2)[&m].starts_with("spoj 30 čaká v Tč.13"));
    }

    #[test]
    fn symbol_and_letter_keys_are_read() {
        let (w, t) = load("401402.pdf");
        let l = legend(&w, &t);
        // Rendered PDF, page 1 and page 2 legends.
        let p1 = l.for_page(1);
        assert_eq!(
            p1[&NoteKey::Symbol(SymbolKey::WorkingDays)],
            "premáva v pracovných dňoch"
        );
        assert_eq!(
            p1[&NoteKey::Symbol(SymbolKey::RequestStop)],
            "zastavuje len na znamenie alebo požiadanie"
        );
        assert_eq!(
            p1[&NoteKey::Symbol(SymbolKey::Saturday)],
            "premáva v sobotu"
        );
        assert_eq!(
            l.for_page(2)[&NoteKey::Symbol(SymbolKey::Marker('¬'))],
            "spoj zastavuje na vystupovanie na zastávke Komárno,nemocnica"
        );
    }

    #[test]
    fn every_note_used_by_a_trip_is_explained() {
        for f in ["401402.pdf", "402424.pdf", "404449.pdf", "405405.pdf"] {
            let (w, t) = load(f);
            let legend = legend(&w, &t);
            for table in &t {
                let l = legend.for_page(table.page);
                for trip in &table.trips {
                    for n in &trip.notes {
                        assert!(
                            l.contains_key(n),
                            "{f}: trip {} note {n:?} not in legend",
                            trip.number
                        );
                    }
                }
            }
        }
    }
}
