use pdf2gtfs::archive::Archive;
use pdf2gtfs::discover::LISTING_PAGES;
use pdf2gtfs::update::{Http, HttpResponse, update};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage:
  pdf2gtfs update [--archive DIR]          download new timetable PDFs (default DIR: data/archive)
  pdf2gtfs osm-extract PBF [OUT]           bus stops + places from an OSM extract (default OUT: data/osm/stops.json)
  pdf2gtfs build [--archive DIR] [--stops FILE] [--out DIR]   build into OUT/staging, publish to OUT if the
                                           checks pass (default OUT: data/gtfs); exit 2 = refused, live feed kept
  pdf2gtfs rollback [OUT]                  swap the live feed with OUT/previous (again to undo)";

/// Polite HTTP client: identifies itself, one request at a time, pause between requests.
struct Polite {
    agent: ureq::Agent,
    pause: Duration,
}

impl Http for Polite {
    fn get(&self, url: &str, inm: Option<&str>, ims: Option<&str>) -> Result<HttpResponse, String> {
        std::thread::sleep(self.pause);
        let mut req = self.agent.get(url);
        if let Some(v) = inm {
            req = req.header("If-None-Match", v);
        }
        if let Some(v) = ims {
            req = req.header("If-Modified-Since", v);
        }
        let mut resp = req.call().map_err(|e| e.to_string())?;
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let (etag, last_modified) = (header("etag"), header("last-modified"));
        let status = resp.status().as_u16();
        let body = if status == 200 {
            resp.body_mut()
                .with_config()
                .limit(50 * 1024 * 1024)
                .read_to_vec()
                .map_err(|e| e.to_string())?
        } else {
            vec![]
        };
        Ok(HttpResponse {
            status,
            etag,
            last_modified,
            body,
        })
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|c| c == "build") {
        let opt = |flag: &str, default: &str| -> PathBuf {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .map_or(PathBuf::from(default), PathBuf::from)
        };
        let today = chrono::Local::now().date_naive();
        let out = opt("--out", "data/gtfs");
        // Built aside, published only if it passes the checks.
        let built = pdf2gtfs::build::build(
            &opt("--archive", "data/archive"),
            &opt("--stops", "data/osm/stops.json"),
            &out.join("staging"),
            today,
        );
        return match built {
            Ok(r) => {
                println!(
                    "{} PDFs ({} failed) → {} lines, {} trips, {} stops ({} approximate)",
                    r.pdfs,
                    r.pdfs_failed.len(),
                    r.lines,
                    r.trips,
                    r.stops,
                    r.stops_approximate
                );
                println!(
                    "excluded: {} trips (see report.json), {} trips through {} unlocated stops{}",
                    r.trips_excluded.len(),
                    r.trips_without_location,
                    r.stops_unlocated.len(),
                    if r.stops_unlocated.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ", e.g. {:?}",
                            &r.stops_unlocated[..r.stops_unlocated.len().min(5)]
                        )
                    }
                );
                println!("dates {:?} .. {:?}", r.first_date, r.last_date);
                match pdf2gtfs::build::publish(&out, today) {
                    Ok(why) if why.is_empty() => {
                        println!("published → {}", out.join("feed").display());
                        ExitCode::SUCCESS
                    }
                    Ok(why) => {
                        eprintln!("NOT published, the live feed stays as it was:");
                        for w in why {
                            eprintln!("  - {w}");
                        }
                        eprintln!("the refused build is in {}", out.join("staging").display());
                        ExitCode::from(2)
                    }
                    Err(e) => {
                        eprintln!("publish failed: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            Err(e) => {
                eprintln!("build failed: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if args.first().is_some_and(|c| c == "rollback") {
        let out = args
            .get(1)
            .map_or(PathBuf::from("data/gtfs"), PathBuf::from);
        return match pdf2gtfs::build::rollback(&out) {
            Ok(()) => {
                println!("swapped the live feed with the previous one (run again to undo)");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("rollback failed: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if let [cmd, pbf, rest @ ..] = args.as_slice()
        && cmd == "osm-extract"
    {
        let out = rest.first().map_or("data/osm/stops.json", String::as_str);
        let extracted =
            pdf2gtfs::osm::extract(pbf.as_ref(), pdf2gtfs::osm::BBOX).and_then(|mut x| {
                x.bus_stations.extend(pdf2gtfs::osm::bus_station_areas(
                    pbf.as_ref(),
                    pdf2gtfs::osm::BBOX,
                )?);
                Ok(x)
            });
        return match extracted {
            Ok(x) => {
                let json = serde_json::to_string_pretty(&x).unwrap();
                if let Err(e) = std::fs::write(out, json) {
                    eprintln!("cannot write {out}: {e}");
                    return ExitCode::FAILURE;
                }
                println!(
                    "{} stops, {} places, {} railway stations, {} bus stations → {out}",
                    x.stops.len(),
                    x.places.len(),
                    x.stations.len(),
                    x.bus_stations.len()
                );
                let roads_out = std::path::Path::new(out).with_file_name("roads.bin");
                match pdf2gtfs::osm::roads(pbf.as_ref(), pdf2gtfs::osm::BBOX)
                    .and_then(|r| r.save(&roads_out).map(|()| r))
                {
                    Ok(r) => {
                        println!(
                            "{} road nodes, {} road segments → {}",
                            r.nodes.len(),
                            r.edges.len(),
                            roads_out.display()
                        );
                        let walk_out = roads_out.with_file_name("walk.bin");
                        match pdf2gtfs::osm::walkways(pbf.as_ref(), pdf2gtfs::osm::BBOX)
                            .and_then(|w| w.save(&walk_out).map(|()| w))
                        {
                            Ok(w) => {
                                println!(
                                    "{} path nodes, {} path segments → {}",
                                    w.nodes.len(),
                                    w.edges.len(),
                                    walk_out.display()
                                );
                                ExitCode::SUCCESS
                            }
                            Err(e) => {
                                eprintln!("walkways: {e}");
                                ExitCode::FAILURE
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("roads: {e}");
                        ExitCode::FAILURE
                    }
                }
            }
            Err(e) => {
                eprintln!("cannot read {pbf}: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let archive_dir = match args.as_slice() {
        [cmd] if cmd == "update" => PathBuf::from("data/archive"),
        [cmd, flag, dir] if cmd == "update" && flag == "--archive" => PathBuf::from(dir),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(64);
        }
    };

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .user_agent(concat!(
            "tyrkobus-pdf2gtfs/",
            env!("CARGO_PKG_VERSION"),
            " (open timetable data for Nitra region; https://github.com/Silorak/tyrkobus)"
        ))
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into();
    let http = Polite {
        agent,
        pause: Duration::from_millis(500),
    };

    let mut archive = match Archive::open(&archive_dir) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cannot open archive {}: {e}", archive_dir.display());
            return ExitCode::FAILURE;
        }
    };

    match update(&http, &mut archive, LISTING_PAGES, chrono::Utc::now()) {
        Ok(r) => {
            println!(
                "listed {} | new versions {} | not modified {} | unchanged {} | delisted {} | failed {}",
                r.listed,
                r.new_versions.len(),
                r.not_modified,
                r.unchanged,
                r.delisted.len(),
                r.failed.len()
            );
            for url in &r.new_versions {
                println!("new: {url}");
            }
            for url in &r.delisted {
                println!("delisted: {url}");
            }
            for (url, why) in &r.failed {
                eprintln!("failed: {url}: {why}");
            }
            if r.failed.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(e) => {
            eprintln!("update stopped, archive untouched: {e:?}");
            ExitCode::FAILURE
        }
    }
}
