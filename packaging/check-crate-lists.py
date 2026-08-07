#!/usr/bin/env python3
"""Check the vendored crate lists against a Cargo.lock.

Three recipes build offline from a hand-maintained list of every crates.io
dependency:

    packaging/freebsd/Makefile          CARGO_CRATES=  (name-version)
    packaging/gentoo/moraine-*.ebuild   CRATES="       (name@version)
    packaging/flatpak/cargo-sources.json               (crates.io URLs)

They are generated once and then quietly rot: nothing regenerates them, and a
list that is missing a crate only fails on the distribution that builds
offline — long after the release, and nowhere the author would look. This
compares all three against a lockfile and fails if any diverges.

    check-crate-lists.py <path/to/Cargo.lock>

Exit 0 when every list matches exactly, 1 otherwise. Run it against the
Cargo.lock of the tag the recipes will build, not the working tree — those
differ whenever dependencies changed since the last release.
"""

import json
import re
import sys
from pathlib import Path

try:
    import tomllib  # 3.11+
except ModuleNotFoundError:  # pragma: no cover
    print("check-crate-lists: needs Python 3.11+ (tomllib)", file=sys.stderr)
    sys.exit(2)

PKG = Path(__file__).resolve().parent


def from_lockfile(path):
    """Every crates.io dependency as {name}-{version}.

    Only `registry+` sources: the root package and any path or git dependency
    is never vendored, so listing it would be a false positive.
    """
    data = tomllib.loads(Path(path).read_text())
    return {
        f"{p['name']}-{p['version']}"
        for p in data.get("package", [])
        if p.get("source", "").startswith("registry+")
    }


def from_freebsd(path):
    text = path.read_text()
    m = re.search(r"^CARGO_CRATES=\s*(.*?)(?=^\w|\Z)", text, re.S | re.M)
    if not m:
        return None
    return set(re.findall(r"([A-Za-z0-9_.-]+-\d[\w.+-]*)", m.group(1)))


def from_gentoo(path):
    text = path.read_text()
    m = re.search(r'^CRATES="(.*?)"', text, re.S | re.M)
    if not m:
        return None
    # Gentoo writes name@version; normalise to the name-version used elsewhere.
    return {c.replace("@", "-") for c in re.findall(r"([A-Za-z0-9_.-]+@[\w.+-]+)", m.group(1))}


def from_flatpak(path):
    out = set()
    for entry in json.loads(path.read_text()):
        m = re.search(r"/crates/[^/]+/(.+)\.crate$", entry.get("url", ""))
        if m:
            out.add(m.group(1))
    return out


def from_openbsd(path):
    # MODCARGO_CRATES += name<TAB>version   [# licence]
    return {
        f"{m.group(1)}-{m.group(2)}"
        for m in re.finditer(
            r"^MODCARGO_CRATES\s*\+=\s*(\S+)\s+(\S+)", path.read_text(), re.M
        )
    }


def from_netbsd(path):
    # CARGO_CRATE_DEPENDS+= name-version
    return set(
        re.findall(r"^CARGO_CRATE_DEPENDS\s*\+=\s*(\S+)", path.read_text(), re.M)
    )


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-4], file=sys.stderr)
        print("usage: check-crate-lists.py <path/to/Cargo.lock>", file=sys.stderr)
        return 2

    lockfile = Path(sys.argv[1])
    if not lockfile.is_file():
        print(f"check-crate-lists: no such lockfile: {lockfile}", file=sys.stderr)
        return 2
    expected = from_lockfile(lockfile)

    ebuilds = sorted(PKG.glob("gentoo/moraine-*.ebuild"))
    lists = [
        ("freebsd/Makefile", from_freebsd, PKG / "freebsd/Makefile",
         "cd packaging/freebsd && make makesum cargo-crates   (on a FreeBSD host)"),
        ("gentoo ebuild", from_gentoo, ebuilds[-1] if ebuilds else None,
         "pycargoebuild, or regenerate CRATES= from Cargo.lock"),
        ("flatpak/cargo-sources.json", from_flatpak, PKG / "flatpak/cargo-sources.json",
         "flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json"),
        ("openbsd/crates.inc", from_openbsd, PKG / "openbsd/crates.inc",
         "packaging/bsd-distinfo.py --tag v<version>"),
        ("netbsd/cargo-depends.mk", from_netbsd, PKG / "netbsd/cargo-depends.mk",
         "packaging/bsd-distinfo.py --tag v<version>"),
    ]

    print(f"check-crate-lists: {len(expected)} crates.io dependencies in {lockfile}")
    failed = False
    for label, parse, path, howto in lists:
        if path is None or not path.exists():
            print(f"  ?? {label}: not found — skipped")
            continue
        listed = parse(path)
        if listed is None:
            print(f"  !! {label}: could not parse the crate list", file=sys.stderr)
            failed = True
            continue
        missing = expected - listed
        extra = listed - expected
        if not missing and not extra:
            print(f"  ok {label}: {len(listed)} crates, matches")
            continue

        failed = True
        print(f"  FAIL {label}: {len(listed)} listed, "
              f"{len(missing)} missing, {len(extra)} stale", file=sys.stderr)
        for c in sorted(missing)[:10]:
            print(f"       missing: {c}", file=sys.stderr)
        if len(missing) > 10:
            print(f"       ... and {len(missing) - 10} more", file=sys.stderr)
        for c in sorted(extra)[:10]:
            print(f"       stale:   {c}", file=sys.stderr)
        if len(extra) > 10:
            print(f"       ... and {len(extra) - 10} more", file=sys.stderr)
        print(f"       regenerate: {howto}", file=sys.stderr)

    if failed:
        print("\ncheck-crate-lists: the offline builds (FreeBSD, Gentoo, Flatpak) would "
              "fetch the wrong set of crates.\nRegenerate the lists above, then re-run.",
              file=sys.stderr)
        return 1
    print("check-crate-lists: all lists match")
    return 0


if __name__ == "__main__":
    sys.exit(main())
