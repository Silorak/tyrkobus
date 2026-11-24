//! Stage A: every word on every page with its position, as poppler sees it.

use std::io;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// 1-based page number.
    pub page: u32,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Text exactly as poppler extracted it (not yet decoded, see `glyph`).
    pub raw: String,
}

impl Word {
    pub fn xc(&self) -> f32 {
        (self.x0 + self.x1) / 2.0
    }
    pub fn yc(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// Run `pdftotext -bbox` on a PDF.
pub fn extract(pdf: &Path) -> io::Result<Vec<Word>> {
    let out = Command::new("pdftotext")
        .arg("-bbox")
        .arg(pdf)
        .arg("-")
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "pdftotext failed on {}: {}",
            pdf.display(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(parse_bbox(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse the XHTML that `pdftotext -bbox` prints.
pub fn parse_bbox(xhtml: &str) -> Vec<Word> {
    let mut words = vec![];
    let mut page = 0;
    for chunk in xhtml.split('<').skip(1) {
        if chunk.starts_with("page ") {
            page += 1;
        } else if let Some(rest) = chunk.strip_prefix("word ") {
            let Some((attrs, text)) = rest.split_once('>') else {
                continue;
            };
            let attr = |name: &str| -> f32 {
                let start = attrs
                    .find(&format!("{name}=\""))
                    .map(|i| i + name.len() + 2);
                start
                    .and_then(|s| attrs[s..].split('"').next())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(f32::NAN)
            };
            words.push(Word {
                page,
                x0: attr("xMin"),
                y0: attr("yMin"),
                x1: attr("xMax"),
                y1: attr("yMax"),
                raw: unescape(text),
            });
        }
    }
    words
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_carry_page_number_position_and_unescaped_text() {
        let xhtml = r#"<doc>
<page width="842.000000" height="600.000000">
<word xMin="185.9" yMin="109.0" xMax="223.9" yMax="124.0">Komárno</word>
<word xMin="1.0" yMin="2.0" xMax="3.0" yMax="4.0">a&amp;b</word>
</page>
<page width="842.000000" height="600.000000">
<word xMin="5.5" yMin="6.5" xMax="7.5" yMax="8.5">x</word>
</page>
</doc>"#;

        let words = parse_bbox(xhtml);

        assert_eq!(
            words,
            vec![
                Word {
                    page: 1,
                    x0: 185.9,
                    y0: 109.0,
                    x1: 223.9,
                    y1: 124.0,
                    raw: "Komárno".into()
                },
                Word {
                    page: 1,
                    x0: 1.0,
                    y0: 2.0,
                    x1: 3.0,
                    y1: 4.0,
                    raw: "a&b".into()
                },
                Word {
                    page: 2,
                    x0: 5.5,
                    y0: 6.5,
                    x1: 7.5,
                    y1: 8.5,
                    raw: "x".into()
                },
            ]
        );
    }
}
