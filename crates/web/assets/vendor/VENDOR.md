# Vendored third-party files

Served by the web app so maps work offline and no request leaves for a CDN.
Every package was checked before use (see `scripts/audit.py`): genuine
publisher, tarball integrity equal to the registry's, no install scripts run
(files are taken from the published tarball, `npm install` is never run),
no known vulnerabilities in OSV, release at least two weeks old.

| Source | Version | Licence | Registry integrity |
|---|---|---|---|
| npm `maplibre-gl` (MapLibre org) | 6.17.0 | BSD-3-Clause | `sha512-2chIZwpLGenoUQOcnm9Pk4SLbA8ppNFq8jlpXiq9vcYm3Y1wJ3fc82FL0GLwBaPYJCLzPv/mC46KEupVov75rQ==` |
| npm `pmtiles` (Protomaps, bdon) | 4.5.0 | BSD-3-Clause | `sha512-CBeD4SoUluFziGdy/8k7FOjQxQy486n+929W/tWophauvMICpkZ26vGBWhDt/6b1FZQWNaf4jFdT/5+q0Wii5w==` |
| npm `@protomaps/basemaps` (Protomaps, bdon) | 5.7.2 | BSD-3-Clause | `sha512-K1Yk6bWdULulYg+R2QRVXx4NzJZan5YQhpejEG0c1/sXruJrfPIPZuakpf3jwAgVmjIRVQwAv+yRafDeN0aaUQ==` |
| github.com/protomaps/basemaps-assets `fonts/`, `sprites/v4/` | commit `028c18f713baecad011301ff7a69acc39bcc2ae7` | fonts OFL-1.1, sprites MIT (tangrams/icons) | pinned commit |

Map tiles (`data/map/region.pmtiles`, not in git): cut from the Protomaps
daily build `20261124.pmtiles` with go-pmtiles 1.31.2 (release asset
sha256 `3ed7dbf4…a312f`, equal to GitHub's recorded digest), bbox
17.40,47.70,19.20,48.60, maxzoom 14. Data © OpenStreetMap contributors, ODbL.

## Files (sha256)

- `basemaps.js` `551864f01da1bfe3005e99987197f22f56d0c35f9a10fa1727a889e2434af16d`
- `fonts/Noto Sans Italic/0-255.pbf` `43edfca91c285ba1226f09d5e74d68e1473a088c517f02a5212ff6ccb10037dc`
- `fonts/Noto Sans Italic/256-511.pbf` `a6f9f6574c86a4a28aba630ca1017857ceadd2370ed32a1293c35f819ab9bd60`
- `fonts/Noto Sans Italic/8192-8447.pbf` `5dc4e6680116fef01be024e272859d130262613c7e634409a590e5d50ad124ae`
- `fonts/Noto Sans Medium/0-255.pbf` `ba2f0118dd024e3041b158e5f9eb49bc0a658019f53f458e9f5c0b8efcd79b91`
- `fonts/Noto Sans Medium/256-511.pbf` `d5e801a1a5b1d409d3298c3a1e1ca76328e2314a751078833a618620e8e66e4d`
- `fonts/Noto Sans Medium/8192-8447.pbf` `cc38e4956207f0edba1aaf749b61e8f6f1678ef1ad443d32ee285b21d0eb67aa`
- `fonts/Noto Sans Regular/0-255.pbf` `62c6d49b15fa836eb6aa45e259c7ca6762f44b011b09e47776efbe4a6db1b397`
- `fonts/Noto Sans Regular/256-511.pbf` `2eca7561f9f566bcacfda5dd04fb5880baec1328ec0f5484678289a13994de8a`
- `fonts/Noto Sans Regular/8192-8447.pbf` `8ea977a587352fe31b4159ffdbc9a40be79056f2472017c742ea1e4a931864b9`
- `fonts/OFL.txt` `7713cfc8e3c36d5ec4aa3d6cffe7500a1b3310f8a86d914b8ea09c2a9dee7c2d`
- `maplibre/LICENSE.txt` `ee5fc05a0677eaf69601d2c7db0d9ecd6cc27c3abc1d0733bc9ed34707cf8ef2`
- `maplibre/maplibre-gl-shared.mjs` `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `maplibre/maplibre-gl-worker.mjs` `73d7a024a4efe1a46e49db193e18cfe33271e3ecc652ec7542c281a4ef5f0f04`
- `maplibre/maplibre-gl.css` `a90798a1953017790e590f7e1bd35ff103a8a4263972cb47290b78adf86f8c20`
- `maplibre/maplibre-gl.mjs` `e02cb08c572fd0950a75343cd558099c2a1dbbaf67f26b0c96f115431962d1ae`
- `pmtiles.js` `caf981bc46f6327ee7e65d5dc964d89d38a69f60edca2bd4c5c890c21b554c6c`
- `sprites/LICENSE.md` `46d0ca73c10d7366ef7bf3932d8508267096393ccc9ef3a41d1b1d1fe37023f1`
- `sprites/light.json` `bfac76cf7ed5c2aa2992695904056a1c6b07785b7fd20e6c640cb44fd6244a2e`
- `sprites/light.png` `b6a34640917bdc57d0bd080836db33376371a3312ebe7b849045268015de3481`
- `sprites/light@2x.json` `1fb5b123fbe35d2e1f6ac171513ce01df89e671273a417ee89c44886a4c132f0`
- `sprites/light@2x.png` `23f6e9df27c2e9a14385763980a24e3608966bfc8192a106058bc2ca959ab563`
