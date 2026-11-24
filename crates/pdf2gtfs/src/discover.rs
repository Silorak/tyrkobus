//! Find timetable PDF links on arriva.sk listing pages.

use std::collections::BTreeSet;

/// Listing pages that link every timetable PDF for Arriva Nové Zámky.
pub const LISTING_PAGES: &[&str] = &[
    "https://arriva.sk/nove-zamky/regionalna-doprava/cestovne-poriadky/",
    "https://arriva.sk/nove-zamky/mestska-doprava/cestovne-poriadky/",
];

/// Timetable PDF URLs in a listing page, deduplicated and sorted.
pub fn timetable_links(html: &str) -> BTreeSet<String> {
    static LINK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r#"https://arriva\.sk/files/nz/cp/[^"'\s<>]+\.pdf"#).unwrap()
    });
    LINK.find_iter(html)
        .map(|m| m.as_str().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_regional_and_city_timetables() {
        let html = r#"
            <a href="https://arriva.sk/files/nz/cp/primesto/komarno/2026/401401.pdf">CP</a>
            <a href="https://arriva.sk/files/nz/cp/mhd/nove-zamky/2026/404001.pdf">CP</a>
        "#;
        let links: Vec<_> = timetable_links(html).into_iter().collect();
        assert_eq!(
            links,
            [
                "https://arriva.sk/files/nz/cp/mhd/nove-zamky/2026/404001.pdf",
                "https://arriva.sk/files/nz/cp/primesto/komarno/2026/401401.pdf",
            ]
        );
    }

    #[test]
    fn ignores_pdfs_that_are_not_timetables() {
        let html = r#"
            <a href="https://arriva.sk//files/dokumenty/gdpr/ARRIVA_Nove%20Zamky_Osobne_udaje.pdf">GDPR</a>
            <a href="https://arriva.sk/pdf/kodex_spravania_pre_obchodnych_partnerov.pdf">Kódex</a>
        "#;
        assert!(timetable_links(html).is_empty());
    }

    #[test]
    fn a_link_listed_twice_counts_once() {
        let html = r#"
            <a href="https://arriva.sk/files/nz/cp/primesto/komarno/2026/401401.pdf">CP</a>
            <a href="https://arriva.sk/files/nz/cp/primesto/komarno/2026/401401.pdf">CP</a>
        "#;
        assert_eq!(timetable_links(html).len(), 1);
    }

    #[test]
    fn next_years_folder_is_found_too() {
        let html = r#"<a href="https://arriva.sk/files/nz/cp/primesto/komarno/2027/401401.pdf">"#;
        assert_eq!(timetable_links(html).len(), 1);
    }
}
