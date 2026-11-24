//! Checks against the real 2026 PDFs in data/raw/2026 (not in git; see
//! data/sources.txt). Skipped when the files are missing. Needs `pdftotext`.

use std::path::PathBuf;
use std::process::Command;

fn corpus() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/raw/2026");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipping: {} not found", dir.display());
        return vec![];
    };
    let mut files: Vec<_> = entries
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "pdf"))
        .collect();
    files.sort();
    files
}

fn words(pdf: &PathBuf) -> Vec<String> {
    let out = Command::new("pdftotext")
        .arg("-bbox")
        .arg(pdf)
        .arg("-")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "pdftotext failed on {}",
        pdf.display()
    );
    let xml = String::from_utf8(out.stdout).unwrap();
    xml.split("<word ")
        .skip(1)
        .map(|w| {
            let text = &w[w.find('>').unwrap() + 1..w.find("</word>").unwrap()];
            text.replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
        })
        .collect()
}

#[test]
fn every_word_in_the_corpus_decodes() {
    let mut failures = vec![];
    let files = corpus();
    for pdf in &files {
        for w in words(pdf) {
            if let Err(e) = pdf2gtfs::glyph::decode(&w) {
                failures.push(format!(
                    "{}: {:?} in {:?}",
                    pdf.file_name().unwrap().display(),
                    e.ch,
                    w
                ));
            }
        }
    }
    eprintln!("checked {} PDFs", files.len());
    assert!(
        failures.is_empty(),
        "{} undecodable words:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
