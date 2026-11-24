//! Our own line icons: 24×24, 2px round strokes in currentColor, so they take
//! the colour of the text around them. Decorative: the text beside each icon
//! says the same thing, so they are hidden from screen readers.

use maud::{Markup, PreEscaped};

pub enum Icon {
    Bus,
    Walk,
    Swap,
    Clock,
    Pin,
    Back,
    Chevron,
    Search,
    Screen,
    Source,
}

pub fn icon(i: Icon) -> Markup {
    let body = match i {
        Icon::Bus => {
            r#"<rect x="4" y="3" width="16" height="15" rx="3"/><path d="M4 11h16M8 21v-3M16 21v-3"/><circle cx="8" cy="14.5" r=".6" fill="currentColor"/><circle cx="16" cy="14.5" r=".6" fill="currentColor"/>"#
        }
        Icon::Walk => {
            r#"<circle cx="13.5" cy="3.8" r="2" fill="currentColor" stroke="none"/><path d="M11.5 8.2 8.2 10.4 7.4 14M11.5 8.2l2.6.6 2.2 3.4 2.6 1M11.5 8.2 10.4 14l3.4 3.2 1.4 5.2M10.4 14l-2.2 4.2-2.6 3.6"/>"#
        }
        Icon::Swap => {
            r#"<path d="M8 4v16M8 4 4.5 7.5M8 4l3.5 3.5M16 20V4M16 20l-3.5-3.5M16 20l3.5-3.5"/>"#
        }
        Icon::Clock => r#"<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>"#,
        Icon::Pin => {
            r#"<path d="M12 21s-6.5-6.1-6.5-11a6.5 6.5 0 0 1 13 0c0 4.9-6.5 11-6.5 11Z"/><circle cx="12" cy="10" r="2.3"/>"#
        }
        Icon::Back => r#"<path d="M15 5l-7 7 7 7"/>"#,
        Icon::Chevron => r#"<path d="M9 5l7 7-7 7"/>"#,
        Icon::Search => r#"<circle cx="10.5" cy="10.5" r="6"/><path d="M15 15l5 5"/>"#,
        Icon::Screen => {
            r#"<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>"#
        }
        Icon::Source => r#"<path d="M7 3h7l4 4v14H7z"/><path d="M14 3v4h4M10 12h5M10 16h5"/>"#,
    };
    PreEscaped(format!(
        r#"<svg class="i" viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">{body}</svg>"#
    ))
}
