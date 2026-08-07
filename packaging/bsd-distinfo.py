#!/usr/bin/env python3
"""Generate the OpenBSD and NetBSD distfile manifests from a Cargo.lock.

Both trees build Rust offline from a per-crate list plus a checksum file, and
each wants a different set of digests in a different encoding:

    packaging/openbsd/crates.inc   MODCARGO_CRATES += name<TAB>version
    packaging/openbsd/distinfo     SHA256 (base64) + SIZE, "cargo/NAME-VER.tar.gz"
    packaging/netbsd/cargo-depends.mk   CARGO_CRATE_DEPENDS+= name-version
    packaging/netbsd/distinfo      BLAKE2s + SHA512 + Size, "NAME-VER.crate"

Normally these are produced on the target OS (`make modcargo-gen-crates` /
`make makesum`), which needs an OpenBSD and a NetBSD machine. Everything they
compute is a property of bytes on crates.io, though, so it can be done here —
*provided the bytes are the right ones*. That is what makes this trustworthy
rather than plausible: Cargo.lock already records the SHA-256 of every .crate
file, so each download is verified against the lockfile before any other digest
is taken. A wrong or truncated file cannot reach the output.

The formats are not invented; they were read off live ports:
textproc/ripgrep in openbsd/ports and in NetBSD/pkgsrc.

    packaging/bsd-distinfo.py --tag v0.2.2

The crate list is read from the Cargo.lock **inside the release tarball**, never
from the working tree. Those two diverge the moment a dependency changes after a
release, and a list generated from the working tree would describe a source
tree the port does not build — the same class of silent breakage that
check-crate-lists.py exists to catch. Taking the lockfile from the tarball makes
the mismatch impossible rather than merely detectable.

Re-runnable: downloads are cached, so a second run only fetches what changed.
"""

import argparse
import base64
import hashlib
import io
import re
import sys
import tarfile
import tomllib
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PKG = REPO / "packaging"

CRATE_URL = "https://static.crates.io/crates/{name}/{name}-{version}.crate"
TARBALL_URL = "https://github.com/TheJonaz/moraine-backup/archive/refs/tags/{tag}.tar.gz"

# GitHub's tag archive is named after the *project*, with a leading "v" stripped
# from the tag — that is what both trees end up with on disk:
#   OpenBSD  DISTNAME  ?= ${GH_PROJECT}-${GH_TAGNAME without v}
#   pkgsrc   DISTFILES  = ${DISTNAME}${EXTRACT_SUFX}, DISTNAME set by hand
# so the two differ, and each distinfo has to name its own.
OPENBSD_DISTFILE = "moraine-backup-{version}.tar.gz"
NETBSD_DISTFILE = "moraine-{version}.tar.gz"


def crates_from_lock(text):
    """[(name, version, sha256-hex)] for every crates.io dependency."""
    data = tomllib.loads(text)
    out = []
    for p in data.get("package", []):
        if not p.get("source", "").startswith("registry+"):
            continue
        # A registry package always carries a checksum; anything else is a
        # lockfile we do not understand and must not guess about.
        if "checksum" not in p:
            sys.exit(f"bsd-distinfo: {p['name']} has no checksum in Cargo.lock")
        out.append((p["name"], p["version"], p["checksum"]))
    return sorted(out)


def fetch(url, cache, expect_sha256=None):
    """Download to `cache` (once), verifying the digest when one is known."""
    if cache.exists():
        blob = cache.read_bytes()
        if expect_sha256 is None or hashlib.sha256(blob).hexdigest() == expect_sha256:
            return blob
        cache.unlink()  # cached copy is stale or corrupt — refetch

    with urllib.request.urlopen(url, timeout=60) as r:
        blob = r.read()
    if expect_sha256 is not None:
        got = hashlib.sha256(blob).hexdigest()
        if got != expect_sha256:
            sys.exit(
                f"bsd-distinfo: checksum mismatch for {url}\n"
                f"  Cargo.lock says {expect_sha256}\n"
                f"  download is     {got}"
            )
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_bytes(blob)
    return blob


def crate_license(blob, name, version):
    """The `license` field out of the crate's own Cargo.toml.

    OpenBSD's crates.inc carries it as a trailing comment so a reviewer can see
    the licence spread without unpacking 200 tarballs. Missing or exotic
    (`license-file`) entries become an empty comment rather than a guess.
    """
    try:
        with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tar:
            member = tar.extractfile(f"{name}-{version}/Cargo.toml")
            if member is None:
                return ""
            meta = tomllib.loads(member.read().decode("utf-8"))
        return meta.get("package", {}).get("license", "") or ""
    except (tarfile.TarError, KeyError, tomllib.TOMLDecodeError, UnicodeDecodeError):
        return ""


def tarball_metadata(blob):
    """(Cargo.lock text, Cargo.toml version) from inside the release tarball."""
    with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tar:
        want = {"Cargo.lock": None, "Cargo.toml": None}
        for member in tar:
            # GitHub's archive has a single top-level directory whose name we do
            # not want to hard-code; match on the path below it instead.
            rest = member.name.partition("/")[2]
            if rest in want and want[rest] is None:
                f = tar.extractfile(member)
                if f is not None:
                    want[rest] = f.read().decode("utf-8")
    for name, text in want.items():
        if text is None:
            sys.exit(f"bsd-distinfo: {name} not found in the release tarball")
    version = re.search(r'^version\s*=\s*"([^"]+)"', want["Cargo.toml"], re.M).group(1)
    return want["Cargo.lock"], version


def digests(blob):
    return {
        "sha256_b64": base64.b64encode(hashlib.sha256(blob).digest()).decode(),
        "blake2s": hashlib.blake2s(blob).hexdigest(),
        "sha512": hashlib.sha512(blob).hexdigest(),
        "size": len(blob),
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tag", help="release tag to checksum (default: v<Cargo.toml version>)")
    ap.add_argument(
        "--cache",
        type=Path,
        default=Path.home() / ".cache" / "moraine-bsd-distfiles",
        help="where downloaded distfiles are kept between runs",
    )
    ap.add_argument("--jobs", type=int, default=8)
    args = ap.parse_args()

    tag = args.tag or "v" + re.search(
        r'^version\s*=\s*"([^"]+)"', (REPO / "Cargo.toml").read_text(), re.M
    ).group(1)

    # The tarball comes first: it is both the thing being checksummed and the
    # source of truth for what the port will actually compile.
    tarball = fetch(TARBALL_URL.format(tag=tag), args.cache / f"moraine-{tag}.tar.gz")
    td = digests(tarball)
    lock_text, version = tarball_metadata(tarball)
    if f"v{version}" != tag:
        sys.exit(
            f"bsd-distinfo: tag {tag} contains Cargo.toml version {version} — "
            "the tag and its contents disagree"
        )
    print(f"==> tag {tag}: {td['size']} byte tarball, Cargo.toml says {version}",
          file=sys.stderr)

    crates = crates_from_lock(lock_text)
    print(f"==> {len(crates)} crates in the tagged Cargo.lock", file=sys.stderr)

    def one(entry):
        name, ver, sha = entry
        blob = fetch(
            CRATE_URL.format(name=name, version=ver),
            args.cache / f"{name}-{ver}.crate",
            expect_sha256=sha,
        )
        return name, ver, digests(blob), crate_license(blob, name, ver)

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        rows = list(pool.map(one, crates))
    print(f"==> {len(rows)} crates verified against Cargo.lock", file=sys.stderr)

    write_openbsd(rows, version, td)
    write_netbsd(rows, version, td)


def write_openbsd(rows, version, td):
    out = PKG / "openbsd"
    out.mkdir(exist_ok=True)

    # ASCII only: pkglint rejects non-ASCII outright and OpenBSD's tree is no
    # friendlier, so the generator must not introduce a character the ports
    # themselves are not allowed to contain.
    inc = ["# Generated by packaging/bsd-distinfo.py - do not edit by hand.\n"]
    for name, ver, _, lic in rows:
        comment = f"\t# {lic}" if lic else ""
        inc.append(f"MODCARGO_CRATES +=\t{name}\t{ver}{comment}\n")
    (out / "crates.inc").write_text("".join(inc))

    # SHA256 block first, then SIZE block — both sorted by the parenthesised
    # name, which puts "cargo/..." before the release tarball.
    files = {f"cargo/{n}-{v}.tar.gz": d for n, v, d, _ in rows}
    files[OPENBSD_DISTFILE.format(version=version)] = td
    names = sorted(files)
    lines = [f"SHA256 ({n}) = {files[n]['sha256_b64']}\n" for n in names]
    lines += [f"SIZE ({n}) = {files[n]['size']}\n" for n in names]
    (out / "distinfo").write_text("".join(lines))
    print(f"==> wrote {out}/crates.inc and {out}/distinfo", file=sys.stderr)


def write_netbsd(rows, version, td):
    out = PKG / "netbsd"
    out.mkdir(exist_ok=True)

    dep = ["# $NetBSD$\n", "\n"]
    for name, ver, _, _ in rows:
        dep.append(f"CARGO_CRATE_DEPENDS+=\t{name}-{ver}\n")
    (out / "cargo-depends.mk").write_text("".join(dep))

    # One BLAKE2s/SHA512/Size triple per file, all sorted together by filename.
    files = {f"{n}-{v}.crate": d for n, v, d, _ in rows}
    files[NETBSD_DISTFILE.format(version=version)] = td
    lines = ["$NetBSD$\n", "\n"]
    for n in sorted(files):
        d = files[n]
        lines.append(f"BLAKE2s ({n}) = {d['blake2s']}\n")
        lines.append(f"SHA512 ({n}) = {d['sha512']}\n")
        lines.append(f"Size ({n}) = {d['size']} bytes\n")
    (out / "distinfo").write_text("".join(lines))
    print(f"==> wrote {out}/cargo-depends.mk and {out}/distinfo", file=sys.stderr)


if __name__ == "__main__":
    main()
