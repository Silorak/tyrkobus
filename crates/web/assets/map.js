// Journey map: offline vector map of the region (MapLibre + PMTiles file served
// by our own server) with the open journey drawn on top. Optional: without it
// the journey list works the same.
import { Map, Marker, addProtocol, NavigationControl, AttributionControl, LngLatBounds } from "/vendor/maplibre/maplibre-gl.mjs";

const box = document.getElementById("map");
const data = document.getElementById("journeys-data");
if (box && data && window.pmtiles && window.basemaps) {
  const journeys = JSON.parse(data.textContent);
  const lang = document.documentElement.lang || "sk";
  const origin = location.origin;
  const protocol = new pmtiles.Protocol();
  addProtocol("pmtiles", protocol.tile);

  const INK = "#06343c", INK2 = "#4a6b70";
  // A guessed stop position is said in words, never shown as certain.
  const APPROX = { sk: " (približne)", hu: " (kb.)", en: " (approx.)" }[lang] || " (?)";
  // The base map in the app's own world: mist land, teal water and green, white roads.
  const flavor = Object.assign(basemaps.namedFlavor("light"), {
    background: "#e9f1f1", earth: "#f3f7f7", water: "#a9dfe3", ocean_label: "#3f7f86",
    park_a: "#dcece8", park_b: "#c3e3dc", wood_a: "#dfede9", wood_b: "#c6e4dd",
    scrub_a: "#e1eeea", scrub_b: "#cbe6df", hospital: "#ecf1f1", school: "#ecf1f1",
    industrial: "#e3ebeb", pedestrian: "#eef3f3", sand: "#eef1ec", beach: "#eef1ec",
    zoo: "#d9ebe7", military: "#e6ecec", aerodrome: "#e3eaea", runway: "#f6f9f9",
    buildings: "#dde7e8", railway: "#a9bcbf", boundaries: "#a9bcbf",
    roads_label_minor: INK2, roads_label_major: INK2, address_label: INK2,
    city_label: INK, city_label_halo: "#f3f7f7", subplace_label: INK2, subplace_label_halo: "#f3f7f7",
    state_label: "#8fa7aa", state_label_halo: "#f3f7f7", country_label: "#8fa7aa",
    landcover: { grassland: "#e4f0ec", barren: "#f1f3ee", urban_area: "#eaf0f0", farmland: "#eef4f0",
      glacier: "#ffffff", scrub: "#e6f0ec", forest: "#d6eae4" },
  });
  // One colour per bus in the journey, the same as in the list (app.css --leg-N).
  const LEG = ["#007c87", "#c2410c", "#6741d9", "#2b7a3b"];
  const WALKER =
    '<svg class="i" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" ' +
    'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="13.5" cy="3.8" r="2" fill="currentColor" stroke="none"/>' +
    '<path d="M11.5 8.2 8.2 10.4 7.4 14M11.5 8.2l2.6.6 2.2 3.4 2.6 1M11.5 8.2 10.4 14l3.4 3.2 1.4 5.2M10.4 14l-2.2 4.2-2.6 3.6"/></svg>';

  box.hidden = false;
  box.parentElement.classList.add("has-map");
  const map = new Map({
    container: box,
    style: {
      version: 8,
      glyphs: origin + "/vendor/fonts/{fontstack}/{range}.pbf",
      sprite: origin + "/vendor/sprites/light",
      sources: {
        protomaps: {
          type: "vector",
          url: "pmtiles://" + origin + "/map/region.pmtiles",
          attribution: "© <a href=\"https://www.openstreetmap.org/copyright\">OpenStreetMap</a>",
        },
      },
      layers: basemaps.layers("protomaps", flavor, { lang }),
    },
    center: [18.2, 47.9],
    zoom: 9,
    maxBounds: [[17.2, 47.5], [19.4, 48.8]],
    attributionControl: false,
  });
  // Top left: the sheet covers the map's bottom edge on phones.
  map.addControl(new AttributionControl({ compact: true }), "top-left");
  box.map = map; // for tests and debugging: document.getElementById("map").map
  map.addControl(new NavigationControl({ showCompass: false }), "top-right");

  const lngLat = (p) => [p[1], p[0]];
  const metresOf = (path) => {
    let m = 0;
    for (let i = 1; i < path.length; i++) {
      const dy = (path[i][0] - path[i - 1][0]) * 111195;
      const dx = (path[i][1] - path[i - 1][1]) * 111195 * Math.cos((path[i][0] * Math.PI) / 180);
      m += Math.hypot(dx, dy);
    }
    return m;
  };

  // GeoJSON for one journey: rides along their roads, walks, stops.
  // Metres on a local flat projection, for comparing lines.
  const xy = ([lon, lat]) => [lon * Math.cos((lat * Math.PI) / 180) * 111320, lat * 110540];
  const segDist = (p, a, b) => {
    const dx = b[0] - a[0], dy = b[1] - a[1], l2 = dx * dx + dy * dy;
    const t = l2 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2)) : 0;
    return Math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy);
  };
  // How much of line `b` runs along line `a` (metres), and whether mostly the
  // same way (> 0) or the opposite way (< 0).
  const along = (b, a) => {
    const A = a.map(xy), B = b.map(xy);
    let metres = 0, heading = 0;
    for (let i = 0; i + 1 < B.length; i++) {
      const p = B[i], q = B[i + 1], len = Math.hypot(q[0] - p[0], q[1] - p[1]);
      if (len < 1) continue;
      const mid = [(p[0] + q[0]) / 2, (p[1] + q[1]) / 2];
      let best = Infinity, cos = 0;
      for (let k = 0; k + 1 < A.length; k++) {
        const d = segDist(mid, A[k], A[k + 1]);
        if (d < best) {
          best = d;
          const la = Math.hypot(A[k + 1][0] - A[k][0], A[k + 1][1] - A[k][1]) || 1;
          cos = ((q[0] - p[0]) * (A[k + 1][0] - A[k][0]) + (q[1] - p[1]) * (A[k + 1][1] - A[k][1])) / (len * la);
        }
      }
      if (best < 15) { metres += len; heading += cos * len; }
    }
    return { metres, heading };
  };

  const SIDE = 4; // px: two 8 px lines just touching
  const features = (j) => {
    const out = [];
    const drawn = []; // rides so far: { coords, offset }
    let ride = 0;
    for (const leg of j.legs) {
      const n = leg.kind === "ride" ? ride++ : -1;
      const color = n >= 0 ? LEG[n % LEG.length] : INK;
      const coords = leg.path.map(lngLat);
      // Two buses on one street: side by side, not one hiding the other.
      // A line's offset side is relative to the direction it is drawn in, so
      // the shared street must be drawn the same way for both lines; then
      // opposite offsets are opposite sides, whichever way the buses run.
      let offset = 0;
      // 1 when drawn the way the bus drives, -1 when drawn reversed.
      let dir = 1;
      if (n >= 0) {
        let best = null;
        for (const d of drawn) {
          const s = along(coords, d.coords);
          if (s.metres > 20 && (!best || s.metres > best.s.metres)) best = { d, s };
        }
        if (best) {
          if (best.s.heading < 0) {
            coords.reverse();
            dir = -1;
          }
          offset = -best.d.offset;
        } else {
          offset = n % 2 === 0 ? -SIDE : SIDE;
        }
        drawn.push({ coords, offset });
      }
      const long = leg.kind === "ride" || metresOf(leg.path) > 25;
      if (long) {
        out.push({
          type: "Feature",
          properties: { kind: leg.kind, line: leg.line || "", color, offset, dir },
          geometry: { type: "LineString", coordinates: coords },
        });
      }
      leg.stops.forEach((s, i) => {
        const end = i === 0 || i === leg.stops.length - 1;
        out.push({
          type: "Feature",
          properties: { kind: end ? "end" : "via", name: s.name, approx: s.approx, color },
          geometry: { type: "Point", coordinates: [s.lon, s.lat] },
        });
      });
    }
    return { type: "FeatureCollection", features: out };
  };

  // The one authored moment: the bus's route draws itself along the roads.
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const drawIn = () => {
    if (still || !map.getLayer("ride")) return;
    const t0 = performance.now(), ms = 900;
    const step = (now) => {
      const k = Math.min(1, (now - t0) / ms), e = 1 - Math.pow(1 - k, 4);
      map.setPaintProperty("ride", "line-opacity", 0.25 + 0.75 * e);
      map.setPaintProperty("ride", "line-width", 2 + 6 * e);
      if (k < 1) requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  };

  let walkers = [];
  const show = (i) => {
    const j = journeys[i];
    if (!j || !map.getSource("journey")) return;
    map.getSource("journey").setData(features(j));
    drawIn();
    walkers.forEach((m) => m.remove());
    walkers = j.legs
      .filter((l) => l.kind === "walk")
      .map((l) => {
        // A pill halfway along the walk: walking figure + minutes.
        const P = l.path.map((p) => xy([p[1], p[0]]));
        const lens = P.slice(1).map((q, k) => Math.hypot(q[0] - P[k][0], q[1] - P[k][1]));
        let rest = lens.reduce((s, x) => s + x, 0) / 2, k = 0;
        while (k < lens.length - 1 && rest > lens[k]) rest -= lens[k++];
        const t = lens[k] ? rest / lens[k] : 0, a = l.path[k], b = l.path[Math.min(k + 1, l.path.length - 1)];
        const mid = [a[1] + (b[1] - a[1]) * t, a[0] + (b[0] - a[0]) * t];
        const el = document.createElement("div");
        el.className = "walker";
        el.innerHTML = WALKER + '<span class="walker-min"></span>';
        el.querySelector(".walker-min").textContent = l.label || "";
        return new Marker({ element: el, anchor: "bottom", offset: [0, -10] })
          .setLngLat(mid)
          .addTo(map);
      });
    const bounds = new LngLatBounds();
    j.legs.forEach((l) => l.path.forEach((p) => bounds.extend(lngLat(p))));
    // The sheet covers the map's bottom edge on phones.
    const sheetOver = matchMedia("(min-width: 64rem)").matches ? 24 : 56;
    map.fitBounds(bounds, {
      padding: { top: 48, bottom: 40 + sheetOver, left: 64, right: 64 },
      maxZoom: 15,
      duration: matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 600,
    });
  };

  map.on("load", () => {
    map.addSource("journey", { type: "geojson", data: { type: "FeatureCollection", features: [] } });
    const ride = ["==", ["get", "kind"], "ride"];
    map.addLayer({ id: "walk", type: "line", source: "journey", filter: ["==", ["get", "kind"], "walk"],
      layout: { "line-cap": "round" },
      paint: { "line-color": INK, "line-width": 4.5, "line-dasharray": [0, 1.8] } });
    map.addLayer({ id: "ride-casing", type: "line", source: "journey", filter: ride,
      layout: { "line-join": "round", "line-cap": "round" },
      paint: { "line-color": "#fff", "line-width": 12, "line-offset": ["get", "offset"] } });
    map.addLayer({ id: "ride", type: "line", source: "journey", filter: ride,
      layout: { "line-join": "round", "line-cap": "round" },
      paint: { "line-color": ["get", "color"], "line-width": 8, "line-offset": ["get", "offset"] } });
    // Small white chevrons along each bus line, pointing the way the bus drives:
    // two buses on one street in opposite directions read at a glance.
    const chevron = (forward) => {
      // drawn at 2x: 10 x 8 px on screen, inside an 8 px line
      const c = document.createElement("canvas");
      c.width = 20; c.height = 16;
      const g = c.getContext("2d");
      g.strokeStyle = "#fff"; g.lineWidth = 3.2; g.lineCap = "round"; g.lineJoin = "round";
      g.beginPath();
      if (forward) { g.moveTo(6, 2.5); g.lineTo(13.5, 8); g.lineTo(6, 13.5); }
      else { g.moveTo(14, 2.5); g.lineTo(6.5, 8); g.lineTo(14, 13.5); }
      g.stroke();
      return g.getImageData(0, 0, 20, 16);
    };
    map.addImage("chev-f", chevron(true), { pixelRatio: 2 });
    map.addImage("chev-b", chevron(false), { pixelRatio: 2 });
    map.addLayer({ id: "ride-arrow", type: "symbol", source: "journey", filter: ride,
      layout: {
        "symbol-placement": "line", "symbol-spacing": 56,
        "icon-image": ["case", ["==", ["get", "dir"], -1], "chev-b", "chev-f"],
        "icon-rotation-alignment": "map", "icon-allow-overlap": true, "icon-ignore-placement": true,
        // symbols sit on the geometry; move them onto the offset line
        "icon-offset": ["case", [">", ["get", "offset"], 0], ["literal", [0, SIDE]],
          ["<", ["get", "offset"], 0], ["literal", [0, -SIDE]], ["literal", [0, 0]]],
      } });
    map.addLayer({ id: "ride-label", type: "symbol", source: "journey", filter: ride,
      layout: {
        "symbol-placement": "line-center", "text-field": ["get", "line"], "text-font": ["Noto Sans Medium"],
        "text-size": 13, "text-letter-spacing": 0.04, "text-offset": [0, -1.1],
      },
      paint: { "text-color": ["get", "color"], "text-halo-color": "#fff", "text-halo-width": 2.5 } });
    map.addLayer({ id: "via", type: "circle", source: "journey", filter: ["==", ["get", "kind"], "via"],
      paint: {
        "circle-radius": ["case", ["get", "approx"], 3, 3.5], "circle-color": "#fff", "circle-stroke-width": 2,
        "circle-stroke-color": ["case", ["get", "approx"], INK2, ["get", "color"]],
        "circle-stroke-opacity": ["case", ["get", "approx"], 0.7, 1],
      } });
    // Names of the stops passed, once zoomed in far enough to read them.
    map.addLayer({ id: "via-label", type: "symbol", source: "journey", minzoom: 13,
      filter: ["==", ["get", "kind"], "via"],
      layout: {
        "text-field": ["concat", ["get", "name"], ["case", ["get", "approx"], APPROX, ""]],
        "text-font": ["Noto Sans Regular"], "text-size": 12, "text-anchor": "left", "text-offset": [0.8, 0],
        "text-optional": true,
      },
      paint: { "text-color": INK, "text-halo-color": "#fff", "text-halo-width": 2 } });
    // Where you get on or off: large; a guessed position is hollow (white fill, ride-colour ring), never shown as certain.
    map.addLayer({ id: "end", type: "circle", source: "journey", filter: ["==", ["get", "kind"], "end"],
      paint: {
        "circle-radius": 7.5,
        "circle-color": ["case", ["get", "approx"], "#fff", ["get", "color"]],
        "circle-stroke-color": ["case", ["get", "approx"], ["get", "color"], "#fff"],
        "circle-stroke-width": 3.5,
      } });
    map.addLayer({ id: "end-label", type: "symbol", source: "journey", filter: ["==", ["get", "kind"], "end"],
      layout: {
        "text-field": ["concat", ["get", "name"], ["case", ["get", "approx"], APPROX, ""]],
        "text-font": ["Noto Sans Medium"], "text-size": 13, "text-offset": [0, 1.3], "text-anchor": "top",
        "text-optional": true,
      },
      paint: { "text-color": INK, "text-halo-color": "#fff", "text-halo-width": 2 } });
    const open = [...document.querySelectorAll(".journey details")].findIndex((d) => d.open);
    show(open < 0 ? 0 : open);
  });

  // The journey opened in the list is the one on the map.
  document.querySelectorAll(".journey details").forEach((d, i) => {
    d.addEventListener("toggle", () => {
      if (!d.open) return;
      document.querySelectorAll(".journey details").forEach((o) => { if (o !== d) o.open = false; });
      show(i);
    });
  });
}
