#!/usr/bin/env python3
"""Supply-chain check: known vulnerabilities and tampered vendored files.

    python3 scripts/audit.py

1. Every crate in Cargo.lock and every vendored JS package is looked up in
   OSV (https://osv.dev), the database behind RustSec and GitHub advisories.
2. Every vendored file must still have the sha256 recorded in
   crates/web/assets/vendor/VENDOR.md.

Advisories that were reviewed and do not apply are listed in
scripts/audit-exceptions.json with the reason; anything else fails.
Standard library only, so the check itself adds nothing to trust.
"""

import hashlib
import json
import pathlib
import re
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
VENDOR = ROOT / "crates/web/assets/vendor"
NPM = [("maplibre-gl", "6.17.0"), ("pmtiles", "4.5.0"), ("@protomaps/basemaps", "5.7.2")]


def osv(queries):
    req = urllib.request.Request(
        "https://api.osv.dev/v1/querybatch",
        data=json.dumps({"queries": queries}).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)["results"]


def main():
    failures = []
    lock = (ROOT / "Cargo.lock").read_text()
    crates = re.findall(
        r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\nsource = "registry', lock
    )
    packages = [("crates.io", n, v) for n, v in crates] + [("npm", n, v) for n, v in NPM]
    results = osv(
        [{"package": {"name": n, "ecosystem": e}, "version": v} for e, n, v in packages]
    )
    exceptions = json.loads((ROOT / "scripts/audit-exceptions.json").read_text())
    for (eco, name, version), result in zip(packages, results):
        for vuln in result.get("vulns", []):
            reason = exceptions.get(vuln["id"])
            if reason:
                print(f"accepted  {vuln['id']}  {name} {version}: {reason}")
            else:
                failures.append(f"{vuln['id']} in {eco} {name} {version} (see https://osv.dev/{vuln['id']})")
    print(f"checked {len(packages)} packages ({len(crates)} crates, {len(NPM)} npm) against OSV")

    recorded = dict(
        (path, digest)
        for path, digest in re.findall(r"^- `([^`]+)` `([0-9a-f]{64})`$", (VENDOR / "VENDOR.md").read_text(), re.M)
    )
    on_disk = {
        str(p.relative_to(VENDOR)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in VENDOR.rglob("*")
        if p.is_file() and p.name != "VENDOR.md"
    }
    for path in sorted(set(recorded) | set(on_disk)):
        if recorded.get(path) != on_disk.get(path):
            failures.append(f"vendored file {path}: recorded {recorded.get(path)}, on disk {on_disk.get(path)}")
    print(f"checked {len(on_disk)} vendored files against VENDOR.md")

    for f in failures:
        print("FAIL", f)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
