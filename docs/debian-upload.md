# Packaging the Moraine CLI for Debian

Moraine ships two binaries: the `moraine` command-line client and the
`moraine-gui` desktop app (built on iced/wgpu). **Only the CLI is submitted to
Debian.** The desktop app's graphics stack is not packaged in Debian, so the
GUI stays in the upstream release archives and the self-hosted apt repo; getting
it into Debian would first require that whole crate tree to be packaged. Every
step below concerns the CLI package only.

Status of the packaging (phases 0–3, all done):

- `debian/` is complete (control, rules, copyright, watch, changelog, manpage,
  autopkgtest, cargo-checksum).
- The CLI's dependency tree (35 crates) is **fully in Debian** — nothing to
  package first.
- Builds cleanly with **sbuild** in a sid chroot via dh-cargo.
- **lintian** is clean apart from the placeholder ITP bug number.
- **autopkgtest** (smoke test) passes.

What remains is phase 4: the bureaucracy, which must come from your identity
(BTS email, GPG key, mentors account, a sponsor). This file is the runbook.

## One-time prerequisites

- A GPG key tied to `Jonaz Thern <info@thern.io>` (`gpg --list-secret-keys`).
- An account on https://mentors.debian.net with that key uploaded.
- Tools: `sudo apt install devscripts dput-ng reportbug` (most already installed).
- Configure `dput`: dput-ng ships a `mentors` target out of the box.

## Step 1 — File the ITP (Intent To Package)

This reserves the name and gives you the bug number that goes in the changelog.

    reportbug --email info@thern.io wnpp
    # choose: ITP; follow the prompts using the text in ITP-moraine.txt

or send the prepared mail directly (see `ITP-moraine.txt` in the build dir).
You will receive a bug number, e.g. **#1099999**.

## Step 2 — Put the bug number in the changelog

    dch -e        # or edit debian/changelog
    # change:  * Initial release. (Closes: #NNNNNN)
    # to:      * Initial release. (Closes: #1099999)

Commit it, then regenerate the source package (steps below) so it matches.

## Step 3 — Build and sign the source package

From a clean checkout of the tagged release, with `debian/` in place:

    # orig tarball (upstream only, no debian/):
    git archive --prefix=moraine-0.1.0/ HEAD -- . ':(exclude)debian' \
        | gzip > ../moraine_0.1.0.orig.tar.gz
    # source-only build, signed with your key:
    dpkg-buildpackage -S -sa
    # (or build unsigned with -us -uc -d, then: debsign ../moraine_0.1.0-1_source.changes)

This produces `moraine_0.1.0-1_source.changes` + `.dsc` + `.debian.tar.xz`.

Re-verify before uploading:

    sbuild --chroot-mode=unshare -d unstable ../moraine_0.1.0-1.dsc   # builds clean
    lintian -i ../moraine_0.1.0-1_source.changes                      # clean

## Step 4 — Upload to mentors

    dput mentors ../moraine_0.1.0-1_source.changes

The package then appears at https://mentors.debian.net/package/moraine/

## Step 5 — Request a sponsor (RFS)

File the sponsorship-requests bug (see `RFS-moraine.txt`):

    reportbug --email info@thern.io sponsorship-requests
    # or send the prepared RFS mail; reference the ITP number and the mentors URL

Then ping the Rust team — they often sponsor their own ecosystem packages:
`debian-rust@lists.debian.org` / `#debian-rust` on OFTC.

## After acceptance

A DD uploads it. First upload lands in the **NEW queue** (ftpmaster reviews
license/copyright), then **unstable (sid)**, then migrates to **testing** after
~10 days with no RC bugs, and ships in the next **stable** release.

## Recommended follow-ups

- Move the packaging to **Salsa** under `rust-team`, and point `Vcs-Git`/
  `Vcs-Browser` there instead of GitHub.
- Consider `Maintainer: Debian Rust Maintainers <pkg-rust-maintainers@…>` with
  `Uploaders: Jonaz Thern <…>` for team maintenance.
- Path to uploading yourself: become a **Debian Maintainer (DM)** after a few
  sponsored uploads, then a **Debian Developer (DD)** via the NM process.
