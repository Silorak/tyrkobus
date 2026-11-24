mod feed;
mod i18n;
mod icons;
mod views;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use feed::Feed;
use i18n::Lang;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use views::Ctx;

const TZ: chrono_tz::Tz = chrono_tz::Europe::Bratislava;

struct App {
    dir: PathBuf,
    feed: RwLock<Arc<Feed>>,
    stamp: RwLock<Option<SystemTime>>,
}

type Shared = Arc<App>;

#[derive(Deserialize, Default)]
struct Params {
    q: Option<String>,
    date: Option<String>,
    time: Option<String>,
    lang: Option<String>,
    /// Override "now" (YYYY-MM-DDTHH:MM) to see any day; for checking and demos.
    now: Option<String>,
    back: Option<String>,
    from: Option<String>,
    to: Option<String>,
    swap: Option<String>,
}

fn lang(p: &Params, headers: &HeaderMap) -> Lang {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|c| {
            c.split(';')
                .find_map(|kv| kv.trim().strip_prefix("lang="))
                .map(str::to_string)
        });
    let accept = headers
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    p.lang
        .as_deref()
        .and_then(Lang::from_code)
        .or_else(|| cookie.as_deref().and_then(Lang::from_code))
        .or_else(|| accept.get(..2).and_then(Lang::from_code))
        .unwrap_or(Lang::Sk)
}

fn now(p: &Params) -> NaiveDateTime {
    p.now
        .as_deref()
        .and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M").ok())
        .unwrap_or_else(|| chrono::Utc::now().with_timezone(&TZ).naive_local())
}

fn ctx<'a>(feed: &'a Feed, p: &Params, headers: &HeaderMap, here: String) -> Ctx<'a> {
    Ctx {
        feed,
        lang: lang(p, headers),
        here,
        now: now(p),
    }
}

fn here(path: &str, p: &Params) -> String {
    let mut q = vec![];
    for (k, v) in [
        ("q", &p.q),
        ("from", &p.from),
        ("to", &p.to),
        ("date", &p.date),
        ("time", &p.time),
        ("now", &p.now),
    ] {
        if let Some(v) = v {
            q.push(format!("{k}={}", views::urlencode(v)));
        }
    }
    if q.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", q.join("&"))
    }
}

fn current(app: &App) -> Arc<Feed> {
    app.feed.read().unwrap().clone()
}

fn html(m: maud::Markup) -> Response {
    Html(m.into_string()).into_response()
}

fn not_found(feed: &Feed, p: &Params, headers: &HeaderMap) -> Response {
    let c = ctx(feed, p, headers, "/".into());
    (
        StatusCode::NOT_FOUND,
        Html(views::not_found(&c).into_string()),
    )
        .into_response()
}

async fn home(State(app): State<Shared>, Query(p): Query<Params>, headers: HeaderMap) -> Response {
    let feed = current(&app);
    let c = ctx(&feed, &p, &headers, here("/", &p));
    let q = p.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let mut texts = (
        p.from.as_deref().unwrap_or("").trim().to_string(),
        p.to.as_deref().unwrap_or("").trim().to_string(),
    );
    if p.swap.is_some() {
        texts = (texts.1, texts.0);
    }
    let (from, to) = (resolve(&feed, &texts.0), resolve(&feed, &texts.1));
    let chosen = chosen(&p);
    let journeys = match (from, to) {
        (Some(a), Some(b)) => {
            Some(
                feed.network
                    .options(feed.station(a), feed.station(b), chosen.unwrap_or(c.now), 5),
            )
        }
        _ => None,
    };
    let form = views::PlanForm {
        // Show the stop names we understood, so the rider can see what was searched.
        from_text: from.map_or(texts.0, |s| feed.stops[s].name.clone()),
        to_text: to.map_or(texts.1, |s| feed.stops[s].name.clone()),
        from,
        to,
        when: chosen.filter(|w| (*w - c.now).num_minutes().abs() > 1),
        journeys,
    };
    html(views::home(&c, q, &form))
}

/// A typed stop name: the exact name (any case/diacritics), else the best search hit.
fn resolve(feed: &Feed, text: &str) -> Option<usize> {
    if text.is_empty() {
        return None;
    }
    let key = pdf2gtfs::gtfs::slug(text);
    feed.stop(&key)
        .or_else(|| feed.search(text, 1).first().copied())
}

/// Departures from `from` when a date/time was chosen, else from now.
fn chosen(p: &Params) -> Option<NaiveDateTime> {
    let date = NaiveDate::parse_from_str(p.date.as_deref()?, "%Y-%m-%d").ok()?;
    let time = p
        .time
        .as_deref()
        .and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
        .unwrap_or(NaiveTime::MIN);
    Some(date.and_time(time))
}

async fn stop(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let feed = current(&app);
    let Some(s) = feed.stop(&id) else {
        return not_found(&feed, &p, &headers);
    };
    let c = ctx(&feed, &p, &headers, here(&format!("/stop/{id}"), &p));
    let from = chosen(&p);
    let deps = feed.departures(s, from.unwrap_or(c.now), 15);
    html(views::stop(&c, s, &deps, from))
}

async fn stop_rows(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let feed = current(&app);
    let Some(s) = feed.stop(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let c = ctx(&feed, &p, &headers, String::new());
    html(views::departures_fragment(
        &c,
        &feed.departures(s, c.now, 15),
        false,
    ))
}

async fn line(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let feed = current(&app);
    let Some(r) = feed.route(&id) else {
        return not_found(&feed, &p, &headers);
    };
    let c = ctx(&feed, &p, &headers, here(&format!("/line/{id}"), &p));
    let date = p
        .date
        .as_deref()
        .and_then(|d| d.parse().ok())
        .unwrap_or(c.now.date());
    html(views::line(&c, r, date))
}

async fn trip(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let feed = current(&app);
    let Some(t) = feed.trips.iter().position(|t| t.id == id) else {
        return not_found(&feed, &p, &headers);
    };
    let c = ctx(
        &feed,
        &p,
        &headers,
        here(&format!("/trip/{}", views::urlencode(&id)), &p),
    );
    let date = p
        .date
        .as_deref()
        .and_then(|d| d.parse().ok())
        .unwrap_or(c.now.date());
    html(views::trip(&c, t, date))
}

async fn board(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let feed = current(&app);
    let Some(s) = feed.stop(&id) else {
        return not_found(&feed, &p, &headers);
    };
    let c = ctx(&feed, &p, &headers, String::new());
    html(views::board(&c, s, &feed.departures(s, c.now, 8)))
}

async fn board_rows(
    State(app): State<Shared>,
    Path(id): Path<String>,
    Query(p): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let feed = current(&app);
    let Some(s) = feed.stop(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let c = ctx(&feed, &p, &headers, String::new());
    html(views::departures_fragment(
        &c,
        &feed.departures(s, c.now, 8),
        true,
    ))
}

async fn set_lang(Path(code): Path<String>, Query(p): Query<Params>) -> Response {
    let Some(l) = Lang::from_code(&code) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Only same-site paths: never redirect elsewhere.
    let back = p
        .back
        .filter(|b| b.starts_with('/') && !b.starts_with("//"))
        .unwrap_or("/".into());
    let cookie = format!("lang={}; Path=/; Max-Age=31536000; SameSite=Lax", l.code());
    ([(header::SET_COOKIE, cookie)], Redirect::to(&back)).into_response()
}

async fn asset(Path(file): Path<String>) -> Response {
    let (body, mime): (&'static [u8], &str) = match file.as_str() {
        "app.css" => (
            include_bytes!("../assets/app.css"),
            "text/css; charset=utf-8",
        ),
        "map.js" => (
            include_bytes!("../assets/map.js"),
            "text/javascript; charset=utf-8",
        ),
        "app.js" => (
            include_bytes!("../assets/app.js"),
            "text/javascript; charset=utf-8",
        ),
        "fonts/atkinson-hyperlegible-next-latin.woff2" => (
            include_bytes!("../assets/fonts/atkinson-hyperlegible-next-latin.woff2"),
            "font/woff2",
        ),
        "fonts/atkinson-hyperlegible-next-latin-ext.woff2" => (
            include_bytes!("../assets/fonts/atkinson-hyperlegible-next-latin-ext.woff2"),
            "font/woff2",
        ),
        "img/crossway-salka-800.webp" => (
            include_bytes!("../assets/img/crossway-salka-800.webp"),
            "image/webp",
        ),
        "img/crossway-salka-1600.webp" => (
            include_bytes!("../assets/img/crossway-salka-1600.webp"),
            "image/webp",
        ),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    // Fonts and photos never change under one name; CSS/JS must reach kiosks right after an update.
    let cache = if mime == "font/woff2" || mime == "image/webp" {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache)],
        body,
    )
        .into_response()
}

fn stamp(dir: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(dir.join("feed_info.txt"))
        .and_then(|m| m.modified())
        .ok()
}

/// Reload the feed when `pdf2gtfs build` writes a new one.
async fn watch(app: Shared) {
    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let new = stamp(&app.dir);
        if new.is_none() || new == *app.stamp.read().unwrap() {
            continue;
        }
        let dir = app.dir.clone();
        match tokio::task::spawn_blocking(move || Feed::load(&dir)).await {
            Ok(Ok(f)) => {
                eprintln!(
                    "feed reloaded: version {}, {} trips",
                    f.version,
                    f.trips.len()
                );
                *app.feed.write().unwrap() = Arc::new(f);
                *app.stamp.write().unwrap() = new;
            }
            Ok(Err(e)) => eprintln!("feed reload failed, keeping the old one: {e}"),
            Err(e) => eprintln!("feed reload panicked, keeping the old one: {e}"),
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opt = |flag: &str, default: &str| -> String {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or(default.into())
    };
    let dir = PathBuf::from(opt("--feed", "data/gtfs/feed"));
    let addr = opt("--addr", "127.0.0.1:8080");
    // Vetted third-party files (VENDOR.md) and the offline map of the region.
    let vendor = opt("--vendor", "crates/web/assets/vendor");
    let map_file = opt("--map", "data/map/region.pmtiles");
    let feed = Feed::load(&dir).unwrap_or_else(|e| {
        eprintln!(
            "cannot load GTFS feed from {}: {e}\nrun `pdf2gtfs build` first",
            dir.display()
        );
        std::process::exit(1)
    });
    eprintln!(
        "feed {}: {} stops, {} lines, {} trips",
        feed.version,
        feed.stops.len(),
        feed.routes.len(),
        feed.trips.len()
    );
    let app = Arc::new(App {
        stamp: RwLock::new(stamp(&dir)),
        dir,
        feed: RwLock::new(Arc::new(feed)),
    });
    tokio::spawn(watch(app.clone()));

    let router = Router::new()
        .route("/", get(home))
        .route("/stop/{id}", get(stop))
        .route("/stop/{id}/rows", get(stop_rows))
        .route("/line/{id}", get(line))
        .route("/trip/{id}", get(trip))
        .route("/board/{id}", get(board))
        .route("/board/{id}/rows", get(board_rows))
        .route("/lang/{code}", get(set_lang))
        .route("/static/{*file}", get(asset))
        .nest_service("/vendor", tower_http::services::ServeDir::new(&vendor))
        // Range requests: the browser reads only the tiles it shows.
        .route_service(
            "/map/region.pmtiles",
            tower_http::services::ServeFile::new(&map_file),
        )
        .layer(tower_http::compression::CompressionLayer::new())
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("cannot bind address");
    eprintln!("listening on http://{addr}");
    axum::serve(listener, router).await.unwrap();
}
