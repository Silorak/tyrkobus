//! Server-rendered pages (maud). No JavaScript is needed to use them; app.js
//! only refreshes departure lists and runs the board clock.

use crate::feed::{Departure, Feed};
use crate::i18n::{self, Lang, Text::*, day, t};
use crate::icons::{Icon, icon};
use chrono::{Days, NaiveDate, NaiveDateTime};
use maud::{DOCTYPE, Markup, html};
use planner::{Journey, Leg};

pub struct Ctx<'a> {
    pub feed: &'a Feed,
    pub lang: Lang,
    /// Current path and query, for the language switch to come back to.
    pub here: String,
    pub now: NaiveDateTime,
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// "6:20", tabular figures; the label keeps screen readers from spelling it.
pub fn time(minutes: u32) -> Markup {
    let (h, m) = ((minutes / 60) % 24, minutes % 60);
    html! { span.t aria-label=(format!("{h}:{m:02}")) { (format!("{h}:{m:02}")) } }
}

fn time_at(at: NaiveDateTime) -> Markup {
    use chrono::Timelike;
    time(at.hour() * 60 + at.minute())
}

/// A line number in the colour of the n-th bus of a journey (same as on the map).
fn leg_box(name: &str, n: usize) -> Markup {
    html! { span class=(format!("line leg-{}", n % 4)) { (icon(Icon::Bus)) (name) } }
}

fn line_box(name: &str) -> Markup {
    html! { span.line { (name) } }
}

fn countdown(lang: Lang, now: NaiveDateTime, at: NaiveDateTime) -> Option<String> {
    let m = (at - now).num_minutes();
    match m {
        ..=0 => Some(t(lang, Now).to_string()),
        1..=59 => Some(i18n::in_minutes(lang, m)),
        _ => None,
    }
}

fn lang_switch(ctx: &Ctx) -> Markup {
    html! {
        nav.langs aria-label=(format!("{} · Jazyk · Nyelv · Language", t(ctx.lang, Language))) {
            @for l in Lang::ALL {
                a href=(format!("/lang/{}?back={}", l.code(), urlencode(&ctx.here)))
                    aria-current=[(l == ctx.lang).then_some("true")]
                    lang=(l.code()) { (l.code().to_uppercase()) }
            }
        }
    }
}

fn head(title: &str) -> Markup {
    html! {
        meta charset="utf-8";
        meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
        meta name="theme-color" content="#00a1ae";
        title { (title) }
        link rel="preload" href="/static/fonts/atkinson-hyperlegible-next-latin.woff2" as="font" type="font/woff2" crossorigin;
        link rel="stylesheet" href="/static/app.css";
    }
}

fn footer(lang: Lang) -> Markup {
    html! {
        footer.foot {
            p { (t(lang, Unofficial)) " " a href="https://arriva.sk/nove-zamky/" { "arriva.sk" } }
            p { (t(lang, Scheduled)) }
            p {
                (t(lang, PhotoBy)) ": Globetrotter19, "
                a href="https://commons.wikimedia.org/wiki/File:Irisbus_Crossway_nach_Sturovo,_2023_Salka.jpg" { "CC BY-SA 3.0" }
                " · " a href="https://www.openstreetmap.org/copyright" { "© OpenStreetMap" }
            }
        }
    }
}

/// Inner pages: a white app bar with the way back, the page's name, languages.
pub fn page(ctx: &Ctx, title: &str, back: Option<&str>, body: Markup) -> Markup {
    let lang = ctx.lang;
    html! {
        (DOCTYPE)
        html lang=(lang.code()) {
            head { (head(title)) }
            body.app {
                header.bar {
                    a.bar-back href=(back.unwrap_or("/")) aria-label=(t(lang, Back)) { (icon(Icon::Back)) }
                    a.brand href="/" { span.brand-mark { (icon(Icon::Bus)) } span.brand-name { (t(lang, Title)) } }
                    (lang_switch(ctx))
                }
                main.inner { (body) }
                (footer(lang))
                script src="/static/app.js" defer {}
            }
        }
    }
}

fn stop_list(ctx: &Ctx, stops: &[usize]) -> Markup {
    let f = ctx.feed;
    html! {
        ul.stops {
            @for &s in stops {
                li {
                    a href=(format!("/stop/{}", f.stops[s].id)) {
                        span.stop-ic { (icon(Icon::Pin)) }
                        span.name { (f.stops[s].name) }
                        span.lines {
                            @for &r in f.routes_at(s).iter().take(4) { (line_box(&f.routes[r].short_name)) }
                        }
                        span.go-ic { (icon(Icon::Chevron)) }
                    }
                }
            }
        }
    }
}

/// What the rider typed into From / To, what it resolved to, and the result.
#[derive(Default)]
pub struct PlanForm {
    pub from_text: String,
    pub to_text: String,
    pub from: Option<usize>,
    pub to: Option<usize>,
    pub when: Option<NaiveDateTime>,
    pub journeys: Option<Vec<Journey>>,
}

fn stop_field(ctx: &Ctx, id: &str, label: &str, text: &str, found: Option<usize>) -> Markup {
    html! {
        div.end class=(format!("end-{id}")) {
            span.end-dot aria-hidden="true" {}
            label for=(id) { (label) }
            input id=(id) name=(id) list="stop-names" value=(text) required
                autocomplete="off" autocapitalize="off" spellcheck="false"
                placeholder=(t(ctx.lang, SearchHint));
            @if !text.is_empty() && found.is_none() {
                p.err { (t(ctx.lang, UnknownStop)) ": „" (text) "“" }
            }
        }
    }
}

fn plan_form(ctx: &Ctx, form: &PlanForm) -> Markup {
    let lang = ctx.lang;
    let when = form.when.unwrap_or(ctx.now);
    html! {
        form.plan action="/" method="get" {
            div.ends {
                (stop_field(ctx, "from", t(lang, From), &form.from_text, form.from))
                (stop_field(ctx, "to", t(lang, To), &form.to_text, form.to))
                button.swap type="submit" name="swap" value="1" title=(t(lang, Swap)) aria-label=(t(lang, Swap)) { (icon(Icon::Swap)) }
            }
            div.when-row {
                span.chip.on[form.when.is_none()] { (icon(Icon::Clock)) (t(lang, LeaveNow)) }
                details.when open[form.when.is_some()] {
                    summary.chip.on[form.when.is_some()] {
                        @if let Some(w) = form.when { (day(lang, w.date())) " " (w.format("%H:%M")) } @else { (t(lang, OtherTime)) }
                    }
                    div.when-form {
                        input type="date" name="date" value=(when.date().to_string()) aria-label=(t(lang, OtherTime))
                            min=[ctx.feed.first_date.map(|d| d.to_string())] max=[ctx.feed.last_date.map(|d| d.to_string())];
                        input type="time" name="time" value=(when.format("%H:%M").to_string()) aria-label=(t(lang, OtherTime));
                    }
                }
            }
            button.go type="submit" { (t(lang, FindBuses)) }
        }
        datalist #stop-names {
            @for s in &ctx.feed.stops { option value=(s.name) {} }
        }
    }
}

/// Walking figure (also drawn on the map), our own drawing.
fn walker() -> Markup {
    icon(Icon::Walk)
}

/// A piece of the window strip: the n-th bus and its minutes, a walk, a wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seg {
    Ride(usize, i64),
    Walk(i64),
    Wait(i64),
}

impl Seg {
    pub fn minutes(&self) -> i64 {
        match *self {
            Seg::Ride(_, m) | Seg::Walk(m) | Seg::Wait(m) => m,
        }
    }
}

/// The journey as the window strip draws it, in order, in minutes: you walk
/// to the next stop first, then wait there for the bus.
pub fn strip(j: &Journey) -> Vec<Seg> {
    let mut out = vec![];
    let mut at = j.depart();
    let mut bus = 0;
    for leg in &j.legs {
        match leg {
            Leg::Walk { minutes, .. } => {
                out.push(Seg::Walk(i64::from(*minutes)));
                at += chrono::Duration::minutes(i64::from(*minutes));
            }
            Leg::Ride { depart, arrive, .. } => {
                let wait = (*depart - at).num_minutes();
                if wait > 0 {
                    out.push(Seg::Wait(wait));
                }
                out.push(Seg::Ride(bus, (*arrive - *depart).num_minutes()));
                bus += 1;
                at = *arrive;
            }
        }
    }
    out
}

/// The signature: the journey as a bus side of windows, panes sized by minutes.
/// The buses and walks in order: line pills, walks as walker + minutes.
fn route_chain(f: &Feed, j: &Journey) -> Markup {
    html! {
        span.chain {
            @for (i, leg) in j.legs.iter().enumerate() {
                // The arrow stays with what follows it when the chain wraps.
                span.step {
                    @if i > 0 { span.chev aria-hidden="true" { "›" } }
                    @match leg {
                        Leg::Ride { trip, .. } => {
                            @let n = j.legs[..i].iter().filter(|l| matches!(l, Leg::Ride { .. })).count();
                            (leg_box(&f.routes[f.trips[*trip].route].short_name, n))
                        }
                        Leg::Walk { minutes, .. } => span.walkchip { (walker()) span.m { (minutes) } },
                    }
                }
            }
        }
    }
}

/// A thin track under the chain: each ride, walk and wait as long as its minutes.
fn track(j: &Journey) -> Markup {
    html! {
        div.track aria-hidden="true" {
            @for seg in &strip(j) {
                @let grow = format!("flex-grow:{}", seg.minutes().max(1));
                @match seg {
                    Seg::Ride(n, _) => span class=(format!("seg leg-{}", n % 4)) style=(grow) {},
                    Seg::Walk(_) => span.tw style=(grow) {},
                    Seg::Wait(_) => span.tw-wait style=(grow) {},
                }
            }
        }
    }
}

/// "Dnes" / "Zajtra, st 25. 11." / "pi 27. 11."
fn day_heading(ctx: &Ctx, d: chrono::NaiveDate) -> String {
    let today = ctx.now.date();
    let cap = |s: &str| {
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    };
    if d == today {
        cap(t(ctx.lang, Today))
    } else if Some(d) == today.succ_opt() {
        format!("{}, {}", cap(t(ctx.lang, Tomorrow)), i18n::day(ctx.lang, d))
    } else {
        i18n::day(ctx.lang, d)
    }
}

fn journey_list(ctx: &Ctx, journeys: &[Journey]) -> Markup {
    let f = ctx.feed;
    let lang = ctx.lang;
    let stop_link =
        |s: usize| html! { a.stop href=(format!("/stop/{}", f.stops[s].id)) { (f.stops[s].name) } };
    html! {
        @if journeys.is_empty() { p.empty { (t(lang, NoJourney)) } }
        // Say the day whenever a journey is not today: "13:09" alone at 18:54 is a puzzle.
        @let other_day = journeys.iter().any(|j| j.depart().date() != ctx.now.date());
        @if other_day && journeys.first().is_some_and(|j| j.depart().date() != ctx.now.date()) {
            p.no-more { (t(lang, NoMoreToday)) }
        }
        ol.journeys {
            @for (i, j) in journeys.iter().enumerate() {
                @let soon = countdown(lang, ctx.now, j.depart());
                @let d = j.depart().date();
                @if other_day && (i == 0 || journeys[i - 1].depart().date() != d) {
                    li.day { (day_heading(ctx, d)) }
                }
                li.journey {
                    details open[i == 0] {
                        summary {
                            span.span { (time_at(j.depart())) span.arrow aria-hidden="true" { "→" } (time_at(j.arrive())) }
                            span.dur { (i18n::duration(lang, (j.arrive() - j.depart()).num_minutes())) }
                            (route_chain(f, j))
                            (track(j))
                            @if let Some(s) = j.legs.iter().find_map(|l| match l { Leg::Ride { from, .. } => Some(*from), _ => None }) {
                                span.board-at { (icon(Icon::Pin)) (f.stops[s].name) }
                            }
                            span.changes {
                                @if let Some(c) = &soon { span.count.soon[(j.depart() - ctx.now).num_minutes() <= 2] { (c) } " · " }
                                (i18n::changes(lang, j.transfers()))
                            }
                        }
                        ol.legs {
                            @for (i, leg) in j.legs.iter().enumerate() {
                                @let n = j.legs[..i].iter().filter(|l| matches!(l, Leg::Ride { .. })).count();
                                @match leg {
                                    Leg::Ride { trip, service_date, from, to, depart, arrive, from_call, to_call } => {
                                        @let tr = &f.trips[*trip];
                                        li class=(format!("leg-on leg-{}", n % 4)) { span.lt { (time_at(*depart)) } span.node {} (stop_link(*from)) }
                                        li class=(format!("leg-ride leg-{}", n % 4)) {
                                            span.lt {}
                                            span.node {}
                                            span.ride {
                                                (leg_box(&f.routes[tr.route].short_name, n)) " " (t(lang, Towards)) " " (tr.headsign)
                                                span.ride-sub {
                                                    (i18n::duration(lang, (*arrive - *depart).num_minutes())) " · "
                                                    (i18n::stops(lang, to_call - from_call)) " · "
                                                    a href=(format!("/trip/{}?date={service_date}", urlencode(&tr.id))) { (t(lang, Details)) }
                                                }
                                            }
                                        }
                                        li class=(format!("leg-off leg-{}", n % 4)) { span.lt { (time_at(*arrive)) } span.node {} (stop_link(*to)) }
                                    }
                                    Leg::Walk { to, minutes, .. } => {
                                        li.leg-walk {
                                            span.lt.walk-icon aria-hidden="true" { (walker()) }
                                            span.node {}
                                            span { (i18n::walk(lang, *minutes)) " → " (stop_link(*to)) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(serde::Serialize)]
struct MapStop<'a> {
    name: &'a str,
    lat: f64,
    lon: f64,
    approx: bool,
}

#[derive(serde::Serialize)]
struct MapLeg<'a> {
    kind: &'static str,
    line: Option<&'a str>,
    stops: Vec<MapStop<'a>>,
    /// What to draw: the bus's road, or a straight walk. [lat, lon] pairs.
    path: Vec<[f64; 2]>,
    /// Walks: how long, in the page's language ("3 min", "3 perc").
    label: Option<String>,
}

#[derive(serde::Serialize)]
struct MapJourney<'a> {
    legs: Vec<MapLeg<'a>>,
}

/// JSON may sit inside <script>: "</" would end it early.
pub fn script_safe(json: &str) -> String {
    json.replace("</", "<\\/")
}

/// What the map draws for each journey: every stop each bus passes, walks.
pub fn journeys_json(f: &Feed, lang: Lang, journeys: &[Journey]) -> String {
    let stop = |s: usize| MapStop {
        name: &f.stops[s].name,
        lat: f.stops[s].lat,
        lon: f.stops[s].lon,
        approx: f.stops[s].approximate,
    };
    let out: Vec<MapJourney> = journeys
        .iter()
        .map(|j| MapJourney {
            legs: j
                .legs
                .iter()
                .map(|leg| match leg {
                    Leg::Ride {
                        trip,
                        from_call,
                        to_call,
                        ..
                    } => {
                        let tr = &f.trips[*trip];
                        let mut path: Vec<[f64; 2]> = f
                            .path(*trip, *from_call, *to_call)
                            .iter()
                            .map(|p| [p.0, p.1])
                            .collect();
                        let stops: Vec<MapStop> = (*from_call..=*to_call)
                            .map(|k| {
                                let (lat, lon, approx) = f.call_position(*trip, k);
                                MapStop {
                                    name: &f.stops[tr.stop_times[k].stop].name,
                                    lat,
                                    lon,
                                    approx,
                                }
                            })
                            .collect();
                        // Start and end the line exactly at the drawn stops.
                        if let (Some(first), Some(last)) = (stops.first(), stops.last()) {
                            path[0] = [first.lat, first.lon];
                            let n = path.len();
                            path[n - 1] = [last.lat, last.lon];
                        }
                        MapLeg {
                            kind: "ride",
                            line: Some(&f.routes[tr.route].short_name),
                            stops,
                            path,
                            label: None,
                        }
                    }
                    Leg::Walk { from, to, minutes } => {
                        let stops = vec![stop(*from), stop(*to)];
                        let path = match f.walk_path(*from, *to) {
                            Some(way) => way.iter().map(|p| [p.0, p.1]).collect(),
                            None => stops.iter().map(|s| [s.lat, s.lon]).collect(),
                        };
                        MapLeg {
                            kind: "walk",
                            line: None,
                            stops,
                            path,
                            label: Some(i18n::duration(lang, i64::from(*minutes))),
                        }
                    }
                })
                .collect(),
        })
        .collect();
    // A walk goes from where the bus let you off to where the next one stops.
    let mut out = out;
    for j in &mut out {
        for i in 0..j.legs.len() {
            if j.legs[i].kind != "walk" {
                continue;
            }
            let before = i
                .checked_sub(1)
                .and_then(|k| j.legs[k].stops.last())
                .map(|s| (s.lat, s.lon));
            let after = j
                .legs
                .get(i + 1)
                .and_then(|l| l.stops.first())
                .map(|s| (s.lat, s.lon));
            let walk = &mut j.legs[i];
            // A measured path keeps its own ends (the stops) and gets the
            // bus's spot added; a straight walk just moves its ends there.
            let routed = walk.path.len() > 2;
            if let Some((lat, lon)) = before {
                if routed {
                    walk.path.insert(0, [lat, lon]);
                } else {
                    walk.path[0] = [lat, lon];
                }
                walk.stops[0].lat = lat;
                walk.stops[0].lon = lon;
            }
            if let Some((lat, lon)) = after {
                if routed {
                    walk.path.push([lat, lon]);
                } else {
                    let n = walk.path.len() - 1;
                    walk.path[n] = [lat, lon];
                }
                walk.stops[1].lat = lat;
                walk.stops[1].lon = lon;
            }
        }
    }
    script_safe(&serde_json::to_string(&out).expect("plain data serialises"))
}

fn stop_search(ctx: &Ctx, query: Option<&str>, busiest: bool) -> Markup {
    let lang = ctx.lang;
    let f = ctx.feed;
    html! {
        section.panel.departures-search {
            h2 { (t(lang, StopDepartures)) }
            form.search action="/" method="get" role="search" {
                div.field {
                    span.field-ic { (icon(Icon::Search)) }
                    input #q type="search" name="q" value=[query] placeholder=(t(lang, SearchHint)) aria-label=(t(lang, SearchLabel))
                        autocomplete="off" autocapitalize="off" spellcheck="false";
                    button type="submit" { (t(lang, SearchButton)) }
                }
            }
            @match query {
                Some(q) => {
                    @let hits = f.search(q, 40);
                    @if hits.is_empty() { p.empty { (t(lang, NoResults)) } }
                    @else { (stop_list(ctx, &hits)) }
                }
                None => {
                    @if busiest {
                        h3 { (t(lang, BusiestStops)) }
                        (stop_list(ctx, &f.busiest(6)))
                    }
                }
            }
        }
    }
}

pub fn home(ctx: &Ctx, query: Option<&str>, form: &PlanForm) -> Markup {
    let lang = ctx.lang;
    let f = ctx.feed;
    html! {
        (DOCTYPE)
        html lang=(lang.code()) {
            head {
                (head(t(lang, Title)))
                @if form.journeys.as_ref().is_some_and(|j| !j.is_empty()) {
                    link rel="stylesheet" href="/vendor/maplibre/maplibre-gl.css";
                }
            }
            body.app.home {
                @match &form.journeys {
                    None => {
                        header.hero {
                            picture {
                                img src="/static/img/crossway-salka-800.webp"
                                    srcset="/static/img/crossway-salka-800.webp 800w, /static/img/crossway-salka-1600.webp 1600w"
                                    sizes="100vw" alt="" fetchpriority="high" width="800" height="458";
                            }
                            div.hero-top {
                                a.brand href="/" { span.brand-mark { (icon(Icon::Bus)) } span.brand-name { (t(lang, Title)) } }
                                (lang_switch(ctx))
                            }
                            div.hero-text {
                                h1 { (t(lang, WhereToday)) }
                                p { (t(lang, Region)) }
                            }
                            p.credit {
                                (t(lang, PhotoBy)) ": Globetrotter19, "
                                a href="https://commons.wikimedia.org/wiki/File:Irisbus_Crossway_nach_Sturovo,_2023_Salka.jpg" { "CC BY-SA 3.0" }
                            }
                        }
                        main.home-main {
                            div.panel.plan-card { (plan_form(ctx, form)) }
                            section.panel #recent hidden {
                                h2 { (t(lang, Recent)) }
                                ul.recent {}
                            }
                            (stop_search(ctx, query, query.is_none()))
                        }
                    }
                    Some(j) => {
                        header.bar {
                            a.bar-back href="/" aria-label=(t(lang, Back)) { (icon(Icon::Back)) }
                            a.brand href="/" { span.brand-mark { (icon(Icon::Bus)) } span.brand-name { (t(lang, Title)) } }
                            (lang_switch(ctx))
                        }
                        main.results-main {
                            details.panel.route-edit open[j.is_empty()] {
                                summary {
                                    span.route-ends {
                                        span.from { (form.from.map_or(form.from_text.as_str(), |s| f.stops[s].name.as_str())) }
                                        span.to { (form.to.map_or(form.to_text.as_str(), |s| f.stops[s].name.as_str())) }
                                    }
                                    span.route-change { (t(lang, Change)) }
                                }
                                (plan_form(ctx, form))
                            }
                            @if !j.is_empty() {
                                // Shown by map.js; without JavaScript the list alone works.
                                div #map .map hidden role="img" aria-label=(t(lang, MapOfJourney)) {}
                                script #journeys-data type="application/json" { (maud::PreEscaped(journeys_json(f, lang, j))) }
                                script src="/vendor/pmtiles.js" {}
                                script src="/vendor/basemaps.js" {}
                                script type="module" src="/static/map.js" {}
                            }
                            section.sheet.results aria-live="polite" {
                                h2 { (t(lang, Journeys)) }
                                (journey_list(ctx, j))
                            }
                        }
                    }
                }
                (footer(lang))
                script src="/static/app.js" defer {}
            }
        }
    }
}

fn departure_rows(ctx: &Ctx, deps: &[Departure], board: bool) -> Markup {
    let f = ctx.feed;
    let lang = ctx.lang;
    html! {
        @for d in deps {
            @let trip = &f.trips[d.trip];
            @let route = &f.routes[trip.route];
            @let soon = (d.at - ctx.now).num_minutes() <= 2;
            // A stable key, so a refresh can tell which bus has left.
            li.dep.soon[soon] data-key=(format!("{}@{}", trip.id, d.service_date)) {
                a href=[(!board).then(|| format!("/trip/{}?date={}", urlencode(&trip.id), d.service_date))] {
                    span.when {
                        (time_at(d.at))
                        @if let Some(c) = countdown(lang, ctx.now, d.at) { span.count { (c) } }
                        @else if d.at.date() > ctx.now.date() { span.count { (t(lang, Tomorrow)) } }
                    }
                    (line_box(&route.short_name))
                    span.to {
                        span.dest { (trip.headsign) }
                        @if d.request_stop { span.note { (t(lang, RequestStop)) } }
                    }
                    @if board {
                        @let m = (d.at - ctx.now).num_minutes();
                        span.eta { @if m <= 0 { (t(lang, Now)) } @else if m < 60 { (m) " min" } }
                    } @else { span.go-ic { (icon(Icon::Chevron)) } }
                }
            }
        }
    }
}

pub fn departures_fragment(ctx: &Ctx, deps: &[Departure], board: bool) -> Markup {
    let lang = ctx.lang;
    html! {
        @if deps.is_empty() { li.empty { (t(lang, NoDepartures)) } }
        @else { (departure_rows(ctx, deps, board)) }
    }
}

pub fn stop(ctx: &Ctx, s: usize, deps: &[Departure], from: Option<NaiveDateTime>) -> Markup {
    let f = ctx.feed;
    let lang = ctx.lang;
    let stop = &f.stops[s];
    let today = ctx.now.date();
    let shown_day = from.map_or(today, |d| d.date());
    let refresh = from.is_none().then(|| format!("/stop/{}/rows", stop.id));
    let plan = |key: &str| format!("/?{key}={}", urlencode(&stop.name).replace("%20", "+"));
    let body = html! {
        section.stop-head {
            h1 { (stop.name) }
            p.where_ {
                a href=(format!("https://www.openstreetmap.org/?mlat={:.5}&mlon={:.5}#map=17/{:.5}/{:.5}", stop.lat, stop.lon, stop.lat, stop.lon)) {
                    (icon(Icon::Pin)) (t(lang, OnMap))
                }
                @if stop.approximate { span.approx { (t(lang, Approximate)) } }
            }
            p.actions {
                a.btn href=(plan("from")) { (t(lang, PlanFromHere)) }
                a.btn.ghost href=(plan("to")) { (t(lang, PlanToHere)) }
            }
        }
        div.deps-head {
            h2 {
                (t(lang, Departures))
                @if let Some(d) = from { " · " (day(lang, d.date())) " " (d.format("%H:%M")) }
            }
            details open[from.is_some()] {
                summary { (t(lang, OtherTime)) }
                form.when-form action=(format!("/stop/{}", stop.id)) method="get" {
                    input type="date" name="date" value=(shown_day.to_string()) aria-label=(t(lang, OtherTime))
                        min=[f.first_date.map(|d| d.to_string())] max=[f.last_date.map(|d| d.to_string())];
                    input type="time" name="time" aria-label=(t(lang, OtherTime))
                        value=(from.map_or(ctx.now.format("%H:%M").to_string(), |d| d.format("%H:%M").to_string()));
                    button type="submit" { (t(lang, Show)) }
                }
            }
        }
        ol.deps #deps data-refresh=[refresh] aria-live="polite" {
            (departures_fragment(ctx, deps, false))
        }
        section.panel.here {
            h2 { (t(lang, LinesHere)) }
            p.chips {
                @for &r in &f.routes_at(s) {
                    a href=(format!("/line/{}?date={}", f.routes[r].id, shown_day)) title=(f.routes[r].long_name) {
                        (line_box(&f.routes[r].short_name))
                        span { (f.routes[r].long_name) }
                    }
                }
            }
            p { a.board-link href=(format!("/board/{}?lang={}", stop.id, lang.code())) { (icon(Icon::Screen)) (t(lang, BoardMode)) } }
        }
    };
    page(ctx, &stop.name, Some("/"), body)
}

pub fn line(ctx: &Ctx, r: usize, date: NaiveDate) -> Markup {
    let f = ctx.feed;
    let lang = ctx.lang;
    let route = &f.routes[r];
    let table = f.timetable(r, date);
    let prev = date - Days::new(1);
    let next = date + Days::new(1);
    let body = html! {
        h1.line-title { (line_box(&route.short_name)) span { (route.long_name) } }
        nav.days {
            a href=(format!("/line/{}?date={prev}", route.id)) aria-label=(day(lang, prev)) { (icon(Icon::Back)) }
            span { (t(lang, TimetableFor)) " " strong { (day(lang, date)) } }
            a href=(format!("/line/{}?date={next}", route.id)) aria-label=(day(lang, next)) { (icon(Icon::Chevron)) }
        }
        @if table.is_empty() { p.empty { (t(lang, NoTrips)) } }
        @for trips in table.values() {
            @let order = stop_order(f, trips);
            // On today's table, the next trip to set off from now is marked and scrolled to.
            @let next = (date == ctx.now.date()).then(|| next_trip(f, trips, ctx.now)).flatten();
            @let towards = &f.trips[*trips.last().unwrap()].headsign;
            section.panel.direction {
                h2 { (t(lang, Direction)) " " (towards) }
                div.grid-wrap tabindex="0" {
                    table.grid {
                        thead { tr {
                            th scope="col" { (t(lang, SearchLabel)) " / " (t(lang, Trip)) }
                            @for (k, &tr) in trips.iter().enumerate() {
                                th scope="col" .now-col[next == Some(k)] { a href=(format!("/trip/{}?date={date}", urlencode(&f.trips[tr].id))) { (f.trips[tr].number) } }
                            }
                        } }
                        tbody {
                            @for &s in &order {
                                tr {
                                    th scope="row" { a href=(format!("/stop/{}", f.stops[s].id)) { (f.stops[s].name) } }
                                    @for (k, &tr) in trips.iter().enumerate() {
                                        @match f.trips[tr].stop_times.iter().find(|st| st.stop == s) {
                                            Some(st) => td.now-col[next == Some(k)] { (time(st.departure)) },
                                            None => td.none.now-col[next == Some(k)] { "·" },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    page(
        ctx,
        &format!("{} {}", route.short_name, route.long_name),
        Some("/"),
        body,
    )
}

/// The first of `trips` (in timetable order) that has not yet set off at `now`.
fn next_trip(f: &Feed, trips: &[usize], now: NaiveDateTime) -> Option<usize> {
    use chrono::Timelike;
    let now = now.hour() * 60 + now.minute();
    trips.iter().position(|&tr| {
        f.trips[tr]
            .stop_times
            .first()
            .is_some_and(|st| st.departure >= now)
    })
}

/// Stops of several trips in travel order.
fn stop_order(f: &Feed, trips: &[usize]) -> Vec<usize> {
    merge_order(
        trips
            .iter()
            .map(|&t| f.trips[t].stop_times.iter().map(|s| s.stop).collect())
            .collect(),
    )
}

/// Merge stop sequences into one order: start from the longest; a stop not
/// yet placed goes right after the previous placed stop of its trip, or, at
/// the start of a trip, right before the next placed one.
fn merge_order(mut seqs: Vec<Vec<usize>>) -> Vec<usize> {
    seqs.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut order: Vec<usize> = vec![];
    while !seqs.is_empty() {
        // Prefer a trip that shares a stop with what is placed, so it can be anchored.
        let next = seqs
            .iter()
            .position(|s| s.iter().any(|x| order.contains(x)))
            .unwrap_or(0);
        let seq = seqs.remove(next);
        let mut prev: Option<usize> = None;
        let mut pending: Vec<usize> = vec![];
        for s in seq {
            match order.iter().position(|&o| o == s) {
                Some(mut i) => {
                    if prev.is_none() && !pending.is_empty() {
                        let n = pending.len();
                        order.splice(i..i, pending.drain(..));
                        i += n;
                    }
                    prev = Some(i);
                }
                None => match prev {
                    Some(p) => {
                        order.insert(p + 1, s);
                        prev = Some(p + 1);
                    }
                    None if !pending.contains(&s) => pending.push(s),
                    None => {}
                },
            }
        }
        order.extend(pending);
    }
    order
}

pub fn trip(ctx: &Ctx, tr: usize, date: NaiveDate) -> Markup {
    let f = ctx.feed;
    let lang = ctx.lang;
    let trip = &f.trips[tr];
    let route = &f.routes[trip.route];
    let page_no = trip.stop_times.first().map_or(1, |s| s.page.max(1));
    let back = format!("/line/{}?date={date}", route.id);
    let body = html! {
        h1.line-title { a href=(back) { (line_box(&route.short_name)) } span { (t(lang, Towards)) " " (trip.headsign) } }
        p.sub { (t(lang, Trip)) " " (trip.number) " · " (day(lang, date)) }
        h2 { (t(lang, AllStops)) }
        ol.calls.panel {
            @for (i, st) in trip.stop_times.iter().enumerate() {
                @let last = i + 1 == trip.stop_times.len();
                li {
                    span.when { (time(if last { st.arrival } else { st.departure })) }
                    span.node {}
                    a href=(format!("/stop/{}", f.stops[st.stop].id)) { (f.stops[st.stop].name) }
                    @if st.pickup == 3 { span.note { (t(lang, RequestStop)) } }
                    @if st.pickup == 1 && !last { span.note { (t(lang, DropOffOnly)) } }
                    @if st.drop_off == 1 && i > 0 { span.note { (t(lang, BoardingOnly)) } }
                    @if last { span.note { (t(lang, Arrival)) } }
                }
            }
        }
        @if !trip.source_url.is_empty() {
            p.source { a href=(format!("{}#page={page_no}", trip.source_url)) { (icon(Icon::Source)) (t(lang, SourcePage)) " " (page_no) } }
        }
    };
    page(
        ctx,
        &format!("{} {}", route.short_name, trip.headsign),
        Some(&back),
        body,
    )
}

pub fn board(ctx: &Ctx, s: usize, deps: &[Departure]) -> Markup {
    let f = ctx.feed;
    let lang = ctx.lang;
    let stop = &f.stops[s];
    html! {
        (DOCTYPE)
        html lang=(lang.code()) {
            head { (head(&stop.name)) }
            body.board {
                header.board-top {
                    h1 { span.brand-mark { (icon(Icon::Bus)) } (stop.name) }
                    p #clock .clock data-now=(ctx.now.format("%Y-%m-%dT%H:%M:%S")) { (ctx.now.format("%H:%M")) }
                }
                ol.deps.board-rows #deps
                    data-refresh=(format!("/board/{}/rows?lang={}", stop.id, lang.code()))
                    data-every="30" {
                    (departures_fragment(ctx, deps, true))
                }
                footer.board-foot {
                    span { (t(lang, Departures)) " · " (t(lang, Scheduled)) }
                    span #offline hidden { (t(lang, Offline)) }
                }
                script src="/static/app.js" defer {}
            }
        }
    }
}

pub fn not_found(ctx: &Ctx) -> Markup {
    page(
        ctx,
        "404",
        None,
        html! { h1 { "404" } p { a.btn href="/" { (t(ctx.lang, Back)) } } },
    )
}

#[cfg(test)]
mod tests {
    use super::merge_order;

    #[test]
    fn a_short_trip_starting_mid_route_is_placed_before_its_first_shared_stop() {
        // Line 401402 trip 39 starts at Komárno, Rieker obuv (7), a stop trip 7 passes
        // without stopping, and joins the route at Komárno, SLK a.s. (8) (rendered PDF, page 1).
        let order = merge_order(vec![vec![1, 2, 3, 8, 9], vec![7, 8, 9]]);
        assert_eq!(order, [1, 2, 3, 7, 8, 9]);
    }

    #[test]
    fn branch_stops_follow_the_stop_they_leave_from() {
        let order = merge_order(vec![vec![1, 2, 3, 4], vec![1, 2, 5, 4], vec![3, 4, 6]]);
        assert_eq!(order, [1, 2, 5, 3, 4, 6]);
    }

    #[test]
    fn no_stop_is_lost_or_repeated() {
        let order = merge_order(vec![vec![4, 5], vec![1, 2, 3], vec![3, 4]]);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn the_next_trip_is_one_that_has_not_left_yet() {
        let f = crate::feed::tests::feed();
        let mut trips: Vec<usize> = (0..f.trips.len()).collect();
        trips.sort_by_key(|&t| f.trips[t].stop_times[0].departure);
        let first = f.trips[trips[0]].stop_times[0].departure;
        // One minute after the first trip set off: it is gone, even if still on the road.
        let at = |m: u32| {
            chrono::NaiveDate::from_ymd_opt(2026, 4, 20)
                .unwrap()
                .and_hms_opt(m / 60, m % 60, 0)
                .unwrap()
        };
        let next = super::next_trip(f, &trips, at(first + 1)).unwrap();
        assert!(f.trips[trips[next]].stop_times[0].departure > first);
        assert_eq!(super::next_trip(f, &trips, at(first)), Some(0));
    }

    #[test]
    fn every_departure_row_has_a_key_even_on_the_board() {
        let f = crate::feed::tests::feed();
        let now =
            chrono::NaiveDateTime::parse_from_str("2026-04-20 06:00", "%Y-%m-%d %H:%M").unwrap();
        let ctx = super::Ctx {
            feed: f,
            lang: crate::i18n::Lang::Sk,
            here: "/".into(),
            now,
        };
        let s = f.stop("kravany-n-dunajom-jednota").unwrap();
        let deps = f.departures(s, now, 3);
        let html = super::departures_fragment(&ctx, &deps, true).into_string();
        let keys: Vec<&str> = html
            .split("data-key=\"")
            .skip(1)
            .map(|k| &k[..k.find('"').unwrap()])
            .collect();
        assert_eq!(keys.len(), deps.len(), "{html}");
        assert!(keys.windows(2).all(|w| w[0] != w[1]), "{keys:?}");
        assert!(keys[0].ends_with("@2026-04-20"), "{keys:?}");
    }

    #[test]
    fn stop_page_shows_the_next_bus_in_timetable_form_with_a_spoken_label() {
        let f = crate::feed::tests::feed();
        let now =
            chrono::NaiveDateTime::parse_from_str("2026-04-20 06:00", "%Y-%m-%d %H:%M").unwrap();
        let ctx = super::Ctx {
            feed: f,
            lang: crate::i18n::Lang::Hu,
            here: "/".into(),
            now,
        };
        // Monday 20 Apr 2026, 6:00: trip 10 leaves Kravany n. Dunajom, Jednota at 6:10
        // (rendered PDF, page 2).
        let s = f.stop("kravany-n-dunajom-jednota").unwrap();
        let html = super::stop(&ctx, s, &f.departures(s, now, 5), None).into_string();
        assert!(html.contains(r#"aria-label="6:10""#), "{html}");
        assert!(html.contains("Következő indulások"));
        assert!(html.contains("10 perc"));
    }
}

#[cfg(test)]
mod map_tests {
    use super::*;

    #[test]
    fn journey_geometry_lists_every_stop_the_bus_passes() {
        let f = crate::feed::tests::feed();
        // Monday 20 Apr 2026, 6:00: trip 10 from Kravany n. Dunajom, Tulipánová (6:11) to
        // Bátorove Kosihy, hl.nám. (6:35) (rendered PDF, page 2). Tulipánová is the trip's
        // second stop, so its spot on the fixture's road lies north of the road's start.
        let now = NaiveDateTime::parse_from_str("2026-04-20 06:00", "%Y-%m-%d %H:%M").unwrap();
        let from = f.stop("kravany-n-dunajom-tulipanova").unwrap();
        let to = f.stop("batorove-kosihy-hl-nam").unwrap();
        let journeys = f.network.options(&[from], &[to], now, 1);
        let json = journeys_json(f, Lang::Sk, &journeys);
        let v: serde_json::Value = serde_json::from_str(&json.replace("<\\/", "</")).unwrap();
        let stops = v[0]["legs"][0]["stops"].as_array().unwrap();
        let Leg::Ride {
            from_call, to_call, ..
        } = journeys[0].legs[0]
        else {
            panic!()
        };
        assert_eq!(stops.len(), to_call - from_call + 1);
        assert_eq!(stops[0]["name"], "Kravany n. Dunajom, Tulipánová");
        assert_eq!(stops.last().unwrap()["name"], "Bátorove Kosihy, hl.nám.");
        // Rendered PDF, page 2, trip 10: Tulipánová, Mária dvor, Búč rázc., Búč nám.,
        // Búč ZŠ, Rohamtér, OD Fényes, hl.nám.
        assert_eq!(stops.len(), 8);
        assert_eq!(v[0]["legs"][0]["kind"], "ride");
        assert_eq!(v[0]["legs"][0]["line"], "401402");
        assert!(stops[0]["lat"].as_f64().unwrap() > 47.0);
        assert_eq!(stops[0]["approx"], true); // the fixture feed places every stop approximately
        // The line follows the trip's road, and guessed stops sit on it.
        let path = v[0]["legs"][0]["path"].as_array().unwrap();
        assert!(path.len() >= 2);
        assert_eq!(
            path[0],
            serde_json::json!([stops[0]["lat"], stops[0]["lon"]])
        );
        assert_eq!(
            path.last().unwrap(),
            &serde_json::json!([stops.last().unwrap()["lat"], stops.last().unwrap()["lon"]])
        );
        assert!(
            stops[0]["lat"].as_f64().unwrap() > 47.87,
            "on the road, not at the village centre"
        );
    }

    #[test]
    fn json_cannot_close_the_script_tag() {
        assert!(!script_safe(r#"{"name":"</script><b>"}"#).contains("</"));
    }
}

#[cfg(test)]
mod strip_tests {
    use super::*;

    fn at(hm: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(&format!("2026-11-25 {hm}"), "%Y-%m-%d %H:%M").unwrap()
    }

    fn ride(dep: &str, arr: &str) -> Leg {
        Leg::Ride {
            trip: 0,
            service_date: at("00:00").date(),
            from_call: 0,
            to_call: 1,
            from: 0,
            to: 1,
            depart: at(dep),
            arrive: at(arr),
        }
    }

    #[test]
    fn a_journey_is_rides_waits_and_walks_in_minutes() {
        // 7:00–7:20 bus, 3 min walk, wait until 7:30, 7:30–7:45 bus.
        let j = Journey {
            legs: vec![
                ride("07:00", "07:20"),
                Leg::Walk {
                    from: 1,
                    to: 2,
                    minutes: 3,
                },
                ride("07:30", "07:45"),
            ],
        };
        assert_eq!(
            strip(&j),
            [
                Seg::Ride(0, 20),
                Seg::Walk(3),
                Seg::Wait(7),
                Seg::Ride(1, 15)
            ]
        );
    }

    #[test]
    fn a_change_at_the_same_stop_is_a_wait_and_a_direct_bus_is_one_pane() {
        let j = Journey {
            legs: vec![ride("07:00", "07:20"), ride("07:25", "07:40")],
        };
        assert_eq!(
            strip(&j),
            [Seg::Ride(0, 20), Seg::Wait(5), Seg::Ride(1, 15)]
        );
        let one = Journey {
            legs: vec![ride("07:00", "07:20")],
        };
        assert_eq!(strip(&one), [Seg::Ride(0, 20)]);
    }

    #[test]
    fn a_card_lists_buses_and_walks_in_order_over_a_thin_track() {
        let f = crate::feed::tests::feed();
        let j = Journey {
            legs: vec![
                ride("07:00", "07:51"),
                Leg::Walk {
                    from: 1,
                    to: 2,
                    minutes: 9,
                },
                ride("08:05", "08:07"),
            ],
        };
        let ctx = Ctx {
            feed: f,
            lang: Lang::En,
            here: "/".into(),
            now: at("06:00"),
        };
        let html = journey_list(&ctx, &[j]).into_string();
        let chain =
            &html[html.find(r#"class="chain""#).unwrap()..html.find(r#"class="track""#).unwrap()];
        // bus › walk 9 › bus, in that order
        let (bus1, walk, bus2) = (
            chain.find("line leg-0").unwrap(),
            chain.find(r#"class="walkchip""#).unwrap(),
            chain.find("line leg-1").unwrap(),
        );
        assert!(bus1 < walk && walk < bus2, "{chain}");
        assert!(chain.contains(">9<"), "{chain}");
        // the track: one segment per ride, walk and wait, sized by minutes
        assert!(
            html.contains(r#"class="seg leg-0" style="flex-grow:51""#),
            "{html}"
        );
        assert!(html.contains(r#"class="tw" style="flex-grow:9""#), "{html}");
        assert!(!html.contains("class=\"win\"") && !html.contains("pane"));
        // where you get on
        assert!(html.contains(r#"class="board-at""#), "{html}");
    }

    #[test]
    fn journeys_on_another_day_say_which_day() {
        let f = crate::feed::tests::feed();
        let j = Journey {
            legs: vec![ride("13:09", "14:16")],
        };
        // Evening before: no more buses today, so the list says it's tomorrow.
        let ctx = Ctx {
            feed: f,
            lang: Lang::Sk,
            here: "/".into(),
            now: at("18:54") - chrono::Duration::days(1),
        };
        let html = journey_list(&ctx, std::slice::from_ref(&j)).into_string();
        assert!(html.contains("Dnes už žiadny spoj"), "{html}");
        assert!(html.contains("Zajtra, st 25. 11."), "{html}");
        // Same day: no day heading at all.
        let today = Ctx {
            now: at("12:00"),
            ..ctx
        };
        let html = journey_list(&today, &[j]).into_string();
        assert!(
            !html.contains("Zajtra") && !html.contains("Dnes už"),
            "{html}"
        );
    }

    #[test]
    fn the_strip_spans_the_whole_journey() {
        let j = Journey {
            legs: vec![
                Leg::Walk {
                    from: 0,
                    to: 1,
                    minutes: 4,
                },
                ride("07:00", "07:20"),
                ride("07:22", "07:40"),
            ],
        };
        let total: i64 = strip(&j).iter().map(Seg::minutes).sum();
        assert_eq!(total, (j.arrive() - j.depart()).num_minutes());
    }
}
