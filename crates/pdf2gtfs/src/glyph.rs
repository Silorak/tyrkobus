//! Stage A0: turn text that poppler extracted from the timetable font into
//! letters and timetable symbols.
//!
//! The PDFs use one custom font. Letters come out as cp1250 bytes read as
//! Latin-1, and timetable symbols share that byte space.

use chrono::Weekday;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    /// ⚒ runs on working days (`ñ`)
    WorkingDays,
    /// ⑥ runs on Saturday (`Ã`)
    Saturday,
    /// ⑦ runs on Sunday (`ã`)
    Sunday,
    /// † runs on Sunday and public holidays
    SundayAndHolidays,
    /// ① to ⑤ runs on one named weekday (`º Æ æ ð ·`)
    OnWeekday(Weekday),
    /// × request stop
    RequestStop,
    /// ´ trip passes the stop without halting
    PassThrough,
    /// drop-off only stop (`þ`)
    DropOffOnly,
    /// pick-up only stop (`Þ`)
    PickUpOnly,
    /// Black box drawn behind a numbered note (`\x8f`)
    NoteBox,
    /// Marks an ordinary stop name, in the column where × marks a request stop (`\x9c`)
    OrdinaryStop,
    /// Footnote marker whose meaning is defined per PDF in its legend (`« » ¬ ¯`)
    Marker(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Char(char),
    Symbol(Symbol),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGlyph {
    pub ch: char,
    pub position: usize,
}

pub fn decode(raw: &str) -> Result<Vec<Glyph>, UnknownGlyph> {
    raw.chars()
        .enumerate()
        .map(|(position, ch)| {
            if let Some(symbol) = symbol(ch) {
                return Ok(Glyph::Symbol(symbol));
            }
            letter(ch)
                .map(Glyph::Char)
                .ok_or(UnknownGlyph { ch, position })
        })
        .collect()
}

/// Letters only, symbols dropped. For tests and debugging output.
pub fn letters(glyphs: &[Glyph]) -> String {
    glyphs
        .iter()
        .filter_map(|g| match g {
            Glyph::Char(c) => Some(*c),
            Glyph::Symbol(_) => None,
        })
        .collect()
}

/// Symbol glyphs, checked before letters: their codes collide with cp1250
/// letters that Slovak and Hungarian names never use (ń, Ă, ă, Ź, ś, ş, Ć, ć, đ, ţ, Ţ).
fn symbol(ch: char) -> Option<Symbol> {
    use Symbol::*;
    Some(match ch {
        'ñ' => WorkingDays,
        'Ã' => Saturday,
        'ã' => Sunday,
        '†' => SundayAndHolidays,
        'º' => OnWeekday(Weekday::Mon),
        'Æ' => OnWeekday(Weekday::Tue),
        'æ' => OnWeekday(Weekday::Wed),
        'ð' => OnWeekday(Weekday::Thu),
        '·' => OnWeekday(Weekday::Fri),
        '×' => RequestStop,
        '´' => PassThrough,
        'þ' => DropOffOnly,
        'Þ' => PickUpOnly,
        '\u{8f}' => NoteBox,
        '\u{9c}' => OrdinaryStop,
        '«' | '»' | '¬' | '¯' => Marker(ch),
        _ => return None,
    })
}

fn letter(ch: char) -> Option<char> {
    match ch {
        '\u{101}' => Some('ž'),
        'Š' | 'š' | 'Ľ' => Some(ch),
        c if c.is_ascii() => Some(c),
        c if (c as u32) <= 0xFF => {
            let byte = [c as u8];
            let (text, _, had_errors) = encoding_rs::WINDOWS_1250.decode(&byte);
            if had_errors {
                None
            } else {
                text.chars().next()
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Glyph::Symbol as S;

    #[test]
    fn slovak_letters_are_recovered_from_cp1250_mojibake() {
        let g = decode("Turèek majer, Ïulov dvor, nede¾u, dòoch, Na vàšku").unwrap();
        assert_eq!(
            letters(&g),
            "Turček majer, Ďulov dvor, nedeľu, dňoch, Na vŕšku"
        );
    }

    #[test]
    fn letters_poppler_maps_outside_latin1_are_recovered() {
        // poppler emits U+0101 for ž; C1 bytes 0x8E/0x8D/0x9D/0x9E are Ž/Ť/ť/ž
        let g = decode("poāiadanie \u{8e}eliezovce FAJÈI\u{8d} Kme\u{9d}ovce tie\u{9e}").unwrap();
        assert_eq!(letters(&g), "požiadanie Želiezovce FAJČIŤ Kmeťovce tiež");
    }

    #[test]
    fn letters_poppler_already_got_right_pass_through() {
        let g = decode("Štúrovo, Ľudovítov, štátom").unwrap();
        assert_eq!(letters(&g), "Štúrovo, Ľudovítov, štátom");
    }

    #[test]
    fn fixed_meaning_symbols_are_not_letters() {
        let g = decode("ñÃã†ºÆæð·×´þÞ\u{8f}\u{9c}").unwrap();
        assert_eq!(
            g,
            vec![
                S(Symbol::WorkingDays),
                S(Symbol::Saturday),
                S(Symbol::Sunday),
                S(Symbol::SundayAndHolidays),
                S(Symbol::OnWeekday(Weekday::Mon)),
                S(Symbol::OnWeekday(Weekday::Tue)),
                S(Symbol::OnWeekday(Weekday::Wed)),
                S(Symbol::OnWeekday(Weekday::Thu)),
                S(Symbol::OnWeekday(Weekday::Fri)),
                S(Symbol::RequestStop),
                S(Symbol::PassThrough),
                S(Symbol::DropOffOnly),
                S(Symbol::PickUpOnly),
                S(Symbol::NoteBox),
                S(Symbol::OrdinaryStop),
            ]
        );
    }

    #[test]
    fn per_pdf_footnote_markers_keep_their_character() {
        let g = decode("«»¬¯").unwrap();
        assert_eq!(
            g,
            vec![
                S(Symbol::Marker('«')),
                S(Symbol::Marker('»')),
                S(Symbol::Marker('¬')),
                S(Symbol::Marker('¯')),
            ]
        );
    }

    #[test]
    fn note_number_in_box_keeps_its_digits() {
        let g = decode("1\u{8f}0").unwrap();
        assert_eq!(
            g,
            vec![Glyph::Char('1'), S(Symbol::NoteBox), Glyph::Char('0')]
        );
    }

    #[test]
    fn unknown_character_is_an_error_not_a_guess() {
        let err = decode("Komárno ☃").unwrap_err();
        assert_eq!(
            err,
            UnknownGlyph {
                ch: '☃',
                position: 8
            }
        );
    }
}
