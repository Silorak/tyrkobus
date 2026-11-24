//! Stage E1: where is each stop? Timetable names are matched against
//! OpenStreetMap stop names; failing that, the stop is placed at the most
//! specific village or town in its name and flagged approximate.

use crate::gtfs::{Location, slug};
use crate::osm::Extract;
use std::collections::BTreeMap;

pub struct Locator {
    /// normalised name → stop nodes with that name
    stops: BTreeMap<String, Vec<(f64, f64)>>,
    /// normalised town (first name segment) → normalised stop names in that town
    by_town: BTreeMap<String, Vec<String>>,
    /// normalised place name → places with that name
    places: BTreeMap<String, Vec<(f64, f64)>>,
    /// railway stations: normalised name, position
    stations: Vec<(String, (f64, f64))>,
    /// bus stations: normalised name (may be empty), position
    bus_stations: Vec<(String, (f64, f64))>,
}

/// Lowercase ASCII tokens with common abbreviations unified:
/// "Šurany, rázc.k ŽST" and "Šurany, rácz. k žel. st." → "surany razc k zst".
pub fn normalize(name: &str) -> String {
    let slugged = slug(name);
    let tokens: Vec<&str> = slugged.split('-').filter(|t| !t.is_empty()).collect();
    let mut out: Vec<&str> = vec![];
    let mut i = 0;
    while i < tokens.len() {
        let digit = |j: usize| {
            tokens
                .get(j)
                .is_some_and(|t| t.starts_with(|c: char| c.is_ascii_digit()))
        };
        // "č.41", "č. d. 41" and "41" are the same house number: drop "č" / "č. d.".
        if tokens[i] == "c" && digit(i + 1) {
            i += 1;
            continue;
        }
        if tokens[i] == "c" && tokens.get(i + 1) == Some(&"d") && digit(i + 2) {
            i += 2;
            continue;
        }
        let pair = (tokens[i], tokens.get(i + 1).copied());
        let (word, used) = match pair {
            ("zel" | "zelez" | "zeleznicna", Some("st" | "stanica")) => ("zst", 2),
            ("aut" | "autobusova", Some("st" | "stanica")) => ("as", 2),
            // "Autobusové stanovište": the town-bus name for the bus station.
            ("autobusove", Some("stanoviste" | "stan")) => ("as", 2),
            ("racz", _) => ("razc", 1),
            (w, _) => (w, 1),
        };
        out.push(word);
        i += used;
    }
    out.join(" ")
}

fn town_of(name: &str) -> String {
    normalize(name.split(',').next().unwrap_or(name))
}

/// How well a printed stop name fits an OSM one, 0..=1: the share of printed
/// words found in the OSM name times the share of OSM words accounted for.
/// A printed word matches an equal word or, when ≥ 3 letters, abbreviates a
/// longer one ("nám" → "námestie", "kost" → "kostolný").
fn fit(printed: &str, osm: &str) -> (f64, f64) {
    // "ul." (street) and "MHD" (town bus) are written or left out at will.
    let meaningful = |s: &str| -> Vec<String> {
        s.split(' ')
            .filter(|w| !matches!(*w, "" | "ul" | "ulica" | "mhd"))
            .map(str::to_string)
            .collect()
    };
    let (want, have) = (meaningful(printed), meaningful(osm));
    if want.is_empty() || have.is_empty() {
        return (0.0, 0.0);
    }
    // Equal, an abbreviation (≥ 3 letters), or a one-letter slip in a long word.
    let same = |w: &str, h: &str| {
        w == h || (w.len() >= 3 && h.starts_with(w)) || (w.len() >= 5 && edits(w, h) <= 1)
    };
    let found = want
        .iter()
        .filter(|w| have.iter().any(|h| same(w, h)))
        .count();
    let covered = have
        .iter()
        .filter(|h| want.iter().any(|w| same(w, h)))
        .count();
    (
        found as f64 / want.len() as f64,
        covered as f64 / have.len() as f64,
    )
}

/// Below this fit a name is a different stop (one word in four may differ on each side).
const MIN_FIT: f64 = 0.56;
/// A stop named without its village belongs to the nearest village within this distance.
const BARE_NAME_KM: f64 = 3.0;

/// Levenshtein distance.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != b[j]))
                .min(row[j] + 1)
                .min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Kilometres between two points (equirectangular; fine at this scale).
fn km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dy = (a.0 - b.0) * 111.0;
    let dx = (a.1 - b.1) * 111.0 * a.0.to_radians().cos();
    (dx * dx + dy * dy).sqrt()
}

/// Average of the points within 1 km of the first one (both sides of a road).
fn centre(points: &[(f64, f64)]) -> (f64, f64) {
    let near: Vec<&(f64, f64)> = points.iter().filter(|p| km(**p, points[0]) < 1.0).collect();
    let n = near.len() as f64;
    (
        near.iter().map(|p| p.0).sum::<f64>() / n,
        near.iter().map(|p| p.1).sum::<f64>() / n,
    )
}

/// Town as a place name known to OSM: exact, abbreviated ("Kalná n. Hron." →
/// "kalna nad hronom": every printed word a prefix of the real one), or a small
/// typo (≤ 2 edits). Used for timetable names and OSM stop names alike, so
/// "Trnovec n. Váhom, …" and "Trnovec nad Váhom, …" end up in the same town.
fn canonical_town(places: &BTreeMap<String, Vec<(f64, f64)>>, printed: &str) -> Option<String> {
    if places.contains_key(printed) {
        return Some(printed.to_string());
    }
    let want: Vec<&str> = printed.split(' ').collect();
    let unique = |found: Vec<&String>| (found.len() == 1).then(|| found[0].to_string());
    let abbreviated = places
        .keys()
        .filter(|k| {
            let have: Vec<&str> = k.split(' ').collect();
            have.len() == want.len() && want.iter().zip(&have).all(|(w, h)| h.starts_with(w))
        })
        .collect();
    unique(abbreviated).or_else(|| {
        let close = places
            .keys()
            .filter(|k| printed.len() >= 6 && edits(k, printed) <= 2)
            .collect();
        unique(close)
    })
}

impl Locator {
    pub fn new(x: &Extract) -> Self {
        let mut places: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
        for p in &x.places {
            places
                .entry(normalize(&p.name))
                .or_default()
                .push((p.lat, p.lon));
        }
        let mut stops: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
        let mut by_town: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let nearest_place = |at: (f64, f64)| -> Option<String> {
            places
                .iter()
                .flat_map(|(name, pts)| pts.iter().map(move |p| (km(*p, at), name)))
                .filter(|(d, _)| *d < BARE_NAME_KM)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, name)| name.clone())
        };
        for s in &x.stops {
            let at = (s.lat, s.lon);
            let (town, rest) = if s.name.contains(',') {
                let printed = town_of(&s.name);
                let town = canonical_town(&places, &printed).unwrap_or(printed);
                (
                    town,
                    s.name.split(',').skip(1).collect::<Vec<_>>().join(","),
                )
            } else {
                // "Hrkovce": a village's only stop, named like the village.
                let plain = normalize(&s.name);
                if !stops.contains_key(&plain) {
                    by_town
                        .entry(plain.clone())
                        .or_default()
                        .push(plain.clone());
                }
                stops.entry(plain).or_default().push(at);
                // "Strážny dom": the village is wherever the stop stands.
                let Some(town) = nearest_place(at) else {
                    continue;
                };
                (town, s.name.clone())
            };
            let key = normalize(&format!("{town},{rest}"));
            if !stops.contains_key(&key) {
                by_town.entry(town).or_default().push(key.clone());
            }
            stops.entry(key).or_default().push(at);
        }
        Self {
            stops,
            by_town,
            places,
            stations: x
                .stations
                .iter()
                .map(|p| (normalize(&p.name), (p.lat, p.lon)))
                .collect(),
            bus_stations: x
                .bus_stations
                .iter()
                .map(|p| (normalize(&p.name), (p.lat, p.lon)))
                .collect(),
        }
    }

    /// The town a stop name starts with, as known to OSM.
    fn town(&self, name: &str) -> Option<String> {
        let printed_town = town_of(name);
        canonical_town(&self.places, &printed_town).or_else(|| {
            self.by_town
                .contains_key(&printed_town)
                .then_some(printed_town)
        })
    }

    pub fn locate(&self, name: &str) -> Option<Location> {
        let exact = |(lat, lon): (f64, f64)| Location {
            lat,
            lon,
            approximate: false,
        };
        let town = self.town(name)?;
        let rest: Vec<&str> = name.split(',').skip(1).collect();
        let key = normalize(&format!("{town},{}", rest.join(",")));
        if let Some(points) = self.stops.get(&key) {
            return Some(exact(centre(points)));
        }
        let in_town = self.by_town.get(&town);
        if let Some(candidates) = in_town
            && !rest.join("").trim().is_empty()
        {
            // Compare only the stop part: the town words always match and
            // would hide the one word that tells two stops apart.
            // The town's own words on the OSM side ("Vinodol, Horný Vinodol, kostol") say nothing.
            let town_words: Vec<&str> = town.split(' ').collect();
            let part = |k: &str| -> String {
                let rest = k.strip_prefix(town.as_str()).unwrap_or(k);
                let words: Vec<&str> = rest
                    .split(' ')
                    .filter(|w| !w.is_empty() && !town_words.contains(w))
                    .collect();
                words.join(" ")
            };
            let want = part(&key);
            // Good when most words agree both ways, or every printed word is
            // there and OSM adds at most as many words as it shares.
            let good = |(found, covered): (f64, f64)| {
                found * covered >= MIN_FIT || (found == 1.0 && covered >= 0.5)
            };
            let mut scored: Vec<(f64, &String)> = candidates
                .iter()
                .filter_map(|c| {
                    let f = fit(&want, &part(c));
                    good(f).then_some((f.0 * f.1, c))
                })
                .collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0));
            if let Some(&(best, c)) = scored.first() {
                let at = centre(&self.stops[c]);
                // Equally good candidates are fine if they are the same place mapped twice.
                let same_place = scored
                    .iter()
                    .take_while(|(score, _)| *score == best)
                    .all(|(_, other)| km(centre(&self.stops[*other]), at) < 0.3);
                if same_place {
                    return Some(exact(at));
                }
            }
        }

        if let Some(p) = self.bus_station(&town, &key) {
            return Some(exact(p));
        }
        if let Some(p) = self.railway_station(&town, &key) {
            return Some(exact(p));
        }
        self.approximate(name)
    }

    /// "Šaľa, AS", "Nové Zámky, AS ŽST": the town's OSM bus station. One
    /// within 3 km: that one. Several: for "AS ŽST" the one nearest the railway
    /// station (≤ 1 km), else the only one named as a bus station; else none.
    fn bus_station(&self, town: &str, key: &str) -> Option<(f64, f64)> {
        let rest = key.strip_prefix(town).unwrap_or(key);
        let words: Vec<&str> = rest.split(' ').filter(|w| !w.is_empty()).collect();
        if !words.contains(&"as") || words.iter().any(|w| matches!(*w, "razc" | "k")) {
            return None;
        }
        let centres = self.places.get(town)?;
        let found: Vec<&(String, (f64, f64))> = self
            .bus_stations
            .iter()
            .filter(|(_, p)| centres.iter().any(|c| km(*c, *p) < 3.0))
            .collect();
        let first = found.first()?.1;
        if found.iter().all(|(_, p)| km(*p, first) < 0.3) {
            return Some(first);
        }
        if words.contains(&"zst")
            && let Some(rail) = self.railway_station(town, &format!("{town} zst"))
        {
            let nearest = found
                .iter()
                .min_by(|a, b| km(a.1, rail).total_cmp(&km(b.1, rail)))?;
            if km(nearest.1, rail) <= 1.0 {
                return Some(nearest.1);
            }
        }
        let named: Vec<_> = found
            .iter()
            .filter(|(n, _)| n.split(' ').any(|w| w == "as"))
            .collect();
        (named.len() == 1).then(|| named[0].1)
    }

    /// "Nové Zámky, AS ŽST" (bus station at the railway station), "Komárno,
    /// Hadovce, ŽST": the OSM railway station of that town or town part,
    /// within 5 km. Not "rázc. k ŽST", the junction out on the main road.
    fn railway_station(&self, town: &str, key: &str) -> Option<(f64, f64)> {
        let rest = key.strip_prefix(town).unwrap_or(key);
        let words: Vec<&str> = rest.split(' ').filter(|w| !w.is_empty()).collect();
        if !words.contains(&"zst") || words.iter().any(|w| matches!(*w, "razc" | "k")) {
            return None;
        }
        let part: Vec<&str> = words
            .iter()
            .copied()
            .filter(|w| !matches!(*w, "as" | "zst" | "mhd" | "nast"))
            .filter(|w| !w.starts_with(|c: char| c.is_ascii_digit()))
            .collect();
        let want = if part.is_empty() {
            town.to_string()
        } else {
            format!("{town} {}", part.join(" "))
        };
        let centres = self.places.get(town)?;
        let found: Vec<(f64, f64)> = self
            .stations
            .iter()
            .filter(|(n, p)| *n == want && centres.iter().any(|c| km(*c, *p) < 5.0))
            .map(|(_, p)| *p)
            .collect();
        // One station, or the same one mapped twice.
        let first = *found.first()?;
        found.iter().all(|p| km(*p, first) < 0.3).then_some(first)
    }

    /// Approximate position: the town, or a more specific village or part
    /// named in the stop ("Šurany, Bešeňov" → Bešeňov).
    pub fn approximate(&self, name: &str) -> Option<Location> {
        let town = self.town(name)?;
        let in_town = self.by_town.get(&town);
        let town_points = self.places.get(&town)?;
        let reference = in_town
            .and_then(|c| c.first())
            .map(|c| centre(&self.stops[c]))
            .unwrap_or(town_points[0]);
        let town_at = *town_points
            .iter()
            .min_by(|a, b| km(**a, reference).total_cmp(&km(**b, reference)))?;
        let approx = |(lat, lon): (f64, f64)| Location {
            lat,
            lon,
            approximate: true,
        };
        let segments: Vec<&str> = name.split(',').skip(1).collect();
        for segment in segments.into_iter().rev() {
            if let Some(points) = self.places.get(&normalize(segment))
                && let Some(p) = points.iter().find(|p| km(**p, town_at) < 15.0)
            {
                return Some(approx(*p));
            }
        }
        Some(approx(town_at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osm::Point;

    fn p(name: &str, lat: f64, lon: f64) -> Point {
        Point {
            name: name.into(),
            lat,
            lon,
        }
    }

    fn locator() -> Locator {
        Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![
                p("Šurany, Námestie", 47.873, 18.191),
                p("Šurany, Námestie", 47.874, 18.192), // other side of the road
                p("Šurany, rázc. k žel. st.", 47.880, 18.200),
                p("Šurany, Žel. st.", 47.881, 18.205),
                p("Komárno, SLK a.s.", 47.760, 18.130),
                p("Šurany, Kostolný háj, obchod", 47.86, 18.17),
            ],
            places: vec![
                p("Šurany", 47.870, 18.195),
                p("Bešeňov", 47.845, 18.160),
                p("Bešeňov", 48.900, 20.000), // same name far away: ignored
                p("Komárno", 47.763, 18.128),
            ],
        })
    }

    #[test]
    fn normalisation_unifies_spelling_and_abbreviations() {
        assert_eq!(
            normalize("Šurany, rázc.k ŽST"),
            normalize("Šurany, rácz. k žel. st.")
        );
        assert_eq!(normalize("Šurany, ŽST"), normalize("Šurany, Žel. st."));
        assert_eq!(
            normalize("Nové Zámky, AS"),
            normalize("Nové Zámky, aut. st.")
        );
    }

    #[test]
    fn exact_match_averages_both_sides_of_the_road() {
        let l = locator().locate("Šurany, Námestie").unwrap();
        assert!(!l.approximate);
        assert!((l.lat - 47.8735).abs() < 1e-9 && (l.lon - 18.1915).abs() < 1e-9);
    }

    #[test]
    fn abbreviations_still_match_exactly() {
        let l = locator().locate("Šurany, rázc.k ŽST").unwrap();
        assert!(!l.approximate);
        assert_eq!((l.lat, l.lon), (47.880, 18.200));
    }

    #[test]
    fn close_name_in_the_same_town_matches() {
        // PDF capitalises "Háj"; OSM writes "háj".
        let l = locator().locate("Šurany, Kostolný Háj, obchod").unwrap();
        assert!(!l.approximate);
        // But a different stop in another town never matches.
        assert!(locator().locate("Bánov, Kostolný Háj, obchod").is_none());
    }

    #[test]
    fn unknown_stop_falls_back_to_the_most_specific_place_nearby() {
        let l = locator().locate("Šurany, Bešeňov").unwrap();
        assert!(l.approximate);
        assert_eq!((l.lat, l.lon), (47.845, 18.160));
        let town = locator().locate("Šurany, Tesco").unwrap();
        assert!(town.approximate);
        assert_eq!((town.lat, town.lon), (47.870, 18.195));
    }

    #[test]
    fn unknown_town_is_not_located() {
        assert!(locator().locate("Atlantis, AS").is_none());
    }

    fn towns() -> Locator {
        Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![p("Kalná nad Hronom, Jednota", 48.20, 18.53)],
            places: vec![
                p("Kalná nad Hronom", 48.21, 18.52),
                p("Tekovské Lužany", 48.10, 18.53),
                p("Hontianske Moravce", 48.19, 19.02),
                p("Dvory nad Žitavou", 47.98, 18.27),
            ],
        })
    }

    #[test]
    fn abbreviated_town_names_are_expanded() {
        let l = towns().locate("Kalná n. Hron., Jednota").unwrap();
        assert!(!l.approximate);
        assert_eq!((l.lat, l.lon), (48.20, 18.53));
        let t = towns().locate("Tek. Lužany, park").unwrap();
        assert!(t.approximate);
        assert_eq!((t.lat, t.lon), (48.10, 18.53));
        assert!(towns().locate("Dvory n. Žitavou, kino").is_some());
    }

    #[test]
    fn small_typos_in_town_names_are_tolerated() {
        let l = towns().locate("Hontinske Moravce, nákup.str.").unwrap();
        assert_eq!((l.lat, l.lon), (48.19, 19.02));
    }

    fn villages() -> Locator {
        Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![
                // OSM often names a stop without its village.
                p("Strážny dom", 47.751, 18.201),
                p("ref.kostol", 47.744, 18.205),
                p("ref.kostol", 47.790, 18.300), // in the next village: not Iža's
                p("Šoporňa, námestie", 48.252, 17.811),
                p("Šurany, Kostolný Sek, Kostolná č.41", 48.080, 18.150),
                p("Dolný Ohaj, rázcestie", 48.060, 18.240),
                p("Dolný Ohaj, Centrum", 48.064, 18.245),
                p("Branč, ZŠ Hlavná", 48.300, 18.000),
                p("Branč, ZŠ Školská", 48.310, 18.010),
            ],
            places: vec![
                p("Iža", 47.747, 18.203),
                p("Patince", 47.789, 18.299),
                p("Šoporňa", 48.250, 17.810),
                p("Šurany", 48.086, 18.186),
                p("Dolný Ohaj", 48.062, 18.243),
                p("Branč", 48.305, 18.005),
            ],
        })
    }

    #[test]
    fn stops_named_without_their_village_belong_to_the_nearest_one() {
        let l = villages().locate("Iža, strážny dom").unwrap();
        assert!(!l.approximate);
        assert_eq!((l.lat, l.lon), (47.751, 18.201));
        let k = villages().locate("Iža, ref.kostol").unwrap();
        assert_eq!(
            (k.lat, k.lon),
            (47.744, 18.205),
            "the one in Iža, not Patince"
        );
    }

    #[test]
    fn abbreviated_words_in_the_stop_name_match_the_full_ones() {
        let n = villages().locate("Šoporňa, nám.").unwrap();
        assert!(!n.approximate);
        assert_eq!((n.lat, n.lon), (48.252, 17.811));
        let k = villages().locate("Šurany, Kost. Sek, Kostolná 41").unwrap();
        assert_eq!((k.lat, k.lon), (48.080, 18.150));
        let r = villages().locate("Dolný Ohaj, rázc.").unwrap();
        assert_eq!((r.lat, r.lon), (48.060, 18.240));
    }

    #[test]
    fn two_equally_good_candidates_mean_we_do_not_guess() {
        let l = villages().locate("Branč, ZŠ").unwrap();
        assert!(l.approximate, "{l:?}");
    }

    #[test]
    fn house_numbers_match_however_they_are_written() {
        let x = Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![
                p("Chľaba, č.d.200", 47.82, 18.82),
                p("Chľaba, č.d. 9", 47.83, 18.83),
            ],
            places: vec![p("Chľaba", 47.825, 18.825)],
        });
        let l = x.locate("Chľaba, č. 200").unwrap();
        assert_eq!((l.approximate, l.lat), (false, 47.82));
        assert_eq!(x.locate("Chľaba, č. 9").unwrap().lat, 47.83);
    }

    #[test]
    fn the_word_that_tells_stops_apart_must_match() {
        // Town words match anyway; "dolný" ≠ "horný" makes it another stop.
        let x = Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![p("Veľké Kozmálovce, horný koniec", 48.29, 18.52)],
            places: vec![p("Veľké Kozmálovce", 48.28, 18.51)],
        });
        assert!(
            x.locate("Veľké Kozmálovce, dolný koniec")
                .unwrap()
                .approximate
        );
    }
    fn real_names() -> Locator {
        Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![
                p("Hrkovce", 48.10, 18.80),
                p("Bánov, Jarmočná", 47.90, 18.30),
                p("Levice, AS", 48.21, 18.60),
                p("Šurany, Kostolni háj, rázc.", 47.86, 18.16), // OSM typo
                p("Kráľová nad Váhom, Mlynárska ul.", 48.20, 17.78),
            ],
            places: vec![
                p("Hrkovce", 48.101, 18.801),
                p("Bánov", 47.90, 18.31),
                p("Levice", 48.215, 18.605),
                p("Šurany", 47.87, 18.19),
                p("Kráľová nad Váhom", 48.20, 17.79),
            ],
        })
    }

    #[test]
    fn a_village_with_a_single_stop_named_like_it_is_exact() {
        let l = real_names().locate("Hrkovce").unwrap();
        assert_eq!((l.approximate, l.lat), (false, 48.10));
    }

    #[test]
    fn filler_words_and_one_letter_typos_do_not_block_a_match() {
        for (printed, lat) in [
            ("Bánov, Jarmočná ul.", 47.90),
            ("Levice, AS MHD", 48.21),
            ("Šurany, Kostolný Háj, rázc.", 47.86),
            ("Kráľová nad Váhom, Mlynárska", 48.20),
        ] {
            let l = real_names().locate(printed).unwrap();
            assert_eq!((l.approximate, l.lat), (false, lat), "{printed}");
        }
    }
    #[test]
    fn the_same_stop_mapped_twice_is_not_a_tie() {
        let x = Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![
                p("Levice, Tabaková", 48.212, 18.585),
                p("Tabaková ulica", 48.2121, 18.5861),
            ],
            places: vec![p("Levice", 48.215, 18.6)],
        });
        let l = x.locate("Levice, Tabaková ul.").unwrap();
        assert!(!l.approximate);
        assert!((l.lat - 48.212).abs() < 0.001, "{l:?}");
    }

    #[test]
    fn a_village_part_in_the_osm_name_is_allowed() {
        let x = Locator::new(&Extract {
            stations: vec![],
            bus_stations: vec![],
            stops: vec![
                p("Vinodol, Horný Vinodol, kostol", 48.192, 18.201),
                p("Vinodol, Dolný Vinodol, ZŠ", 48.188, 18.205),
            ],
            places: vec![p("Vinodol", 48.19, 18.21)],
        });
        assert_eq!(x.locate("Vinodol, kostol").unwrap().lat, 48.192);
        assert_eq!(x.locate("Vinodol, ZŠ").unwrap().lat, 48.188);
        assert!(x.locate("Vinodol, pomník").unwrap().approximate);
    }

    fn railways() -> Locator {
        Locator::new(&Extract {
            bus_stations: vec![],
            stops: vec![p("Nové Zámky, Andovská, ZŠ", 47.986, 18.158)],
            places: vec![
                p("Nové Zámky", 47.986, 18.163),
                p("Komárno", 47.763, 18.128),
                p("Šurany", 47.870, 18.195),
            ],
            stations: vec![
                p("Nové Zámky", 47.9927, 18.1720),
                p("Komárno", 47.7600, 18.1400),
                p("Komárno-Hadovce", 47.7860, 18.0780),
                p("Šurany", 47.8800, 18.2050),
                p("Nové Zámky", 49.5, 20.0), // same name far away
            ],
        })
    }

    #[test]
    fn a_stop_named_after_the_railway_station_is_at_the_station() {
        let l = railways().locate("Nové Zámky, AS ŽST").unwrap();
        assert_eq!((l.approximate, l.lat, l.lon), (false, 47.9927, 18.1720));
        let m = railways().locate("Nové Zámky, AS ŽST MHD").unwrap();
        assert_eq!((m.lat, m.lon), (47.9927, 18.1720));
    }

    #[test]
    fn the_station_of_a_town_part_is_that_part_s_station() {
        let l = railways().locate("Komárno, Hadovce, ŽST").unwrap();
        assert_eq!((l.approximate, l.lat), (false, 47.7860));
        assert_eq!(railways().locate("Komárno, ŽST").unwrap().lat, 47.7600);
    }

    #[test]
    fn the_junction_towards_the_station_is_not_the_station() {
        // "rázc. k ŽST" stands on the main road, often far from the station.
        assert!(railways().locate("Šurany, rázc.k ŽST").unwrap().approximate);
    }

    fn bus_hubs() -> Locator {
        Locator::new(&Extract {
            stops: vec![],
            places: vec![
                p("Nové Zámky", 47.986, 18.163),
                p("Šaľa", 48.151, 17.877),
                p("Levice", 48.215, 18.605),
            ],
            stations: vec![
                p("Nové Zámky", 47.9927, 18.1720),
                p("Levice", 48.2160, 18.6000),
            ],
            bus_stations: vec![
                p("Autobusová stanica", 47.9940, 18.1705), // by the railway station
                p("", 48.1490, 17.8800),
                p("AS Levice", 48.2113, 18.5926),
                p("", 48.2190, 18.6150), // a second, smaller one in Levice
            ],
        })
    }

    #[test]
    fn a_bus_station_stop_is_at_the_mapped_bus_station() {
        let l = bus_hubs().locate("Nové Zámky, AS ŽST").unwrap();
        assert_eq!((l.approximate, l.lat, l.lon), (false, 47.9940, 18.1705));
        let s = bus_hubs().locate("Šaľa, AS").unwrap();
        assert_eq!((s.approximate, s.lat), (false, 48.1490));
        // "Autobusové stanovište" (the town-bus name) is the same bus station.
        let m = bus_hubs()
            .locate("Nové Zámky, Autobusové stanovište")
            .unwrap();
        assert_eq!((m.approximate, m.lat, m.lon), (false, 47.9940, 18.1705));
    }

    #[test]
    fn of_several_bus_stations_the_one_by_the_railway_or_named_as_is_chosen() {
        // "AS-ŽST": the bus station next to the railway station.
        assert_eq!(
            bus_hubs().locate("Levice, AS-ŽST, nást.č.28").unwrap().lat,
            48.2113
        );
        // Plain "AS" with two unnamed candidates and no hint: do not guess.
        let x = Locator::new(&Extract {
            stops: vec![],
            places: vec![p("Komárno", 47.763, 18.128)],
            stations: vec![],
            bus_stations: vec![p("", 47.768, 18.127), p("", 47.760, 18.140)],
        });
        assert!(x.locate("Komárno, AS").unwrap().approximate);
    }
}
