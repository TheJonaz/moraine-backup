# Cutting a release

One command bumps the version, tags and pushes; CI does the rest.

```bash
deploy/bump.sh 0.1.23        # add --dry-run first to preview every edit
```

`bump.sh` rewrites the version in `Cargo.toml`/`Cargo.lock`, stamps the
`## [Unreleased]` heading in `CHANGELOG.md` with the version and date, updates the
install commands in `README.md` and every version label + `cdn.thern.io` download
URL in `site/index.html`, then commits `release: vX.Y.Z`, tags `vX.Y.Z` and pushes
(it asks first; `-y` skips the prompt, `--no-push` stops after the commit). After
pushing it also **deploys the updated `site/index.html` to moraine.thern.io** over
SSH (`notroot@web.thern.io`, `sudo install` into `/home/moraine/public_html`;
override via `MORAINE_SITE_*` env, skip with `--no-site`). The site's version
labels update immediately; its versioned CDN download links resolve once
`cdn-pull` publishes the release (~10 min).

Pushing the tag triggers [`release.yml`](../.github/workflows/release.yml): it
builds the per-OS archives, the `.deb`, `.rpm` and `.pkg.tar.zst`, and attaches
them to the GitHub Release.

## CDN (cdn.thern.io) is pull-based

The CDN host firewalls inbound SSH, so GitHub Actions can't push to it. Instead the
host **pulls**: [`deploy/cdn-pull.sh`](../deploy/cdn-pull.sh), run by a systemd timer
on the server, polls the latest GitHub release, downloads the packages and runs
[`deploy/cdn-publish.sh`](../deploy/cdn-publish.sh) locally to refresh the
apt/dnf/pacman metadata. Nothing to do at release time — the CDN picks up the new
version within the timer interval (~10 min).

One-time server setup (on `cdn.thern.io`, as the account that owns the web root):

```bash
sudo cp deploy/cdn-pull.sh deploy/cdn-publish.sh /usr/local/bin/
sudo chmod +x /usr/local/bin/cdn-pull.sh /usr/local/bin/cdn-publish.sh
sudo cp deploy/systemd/moraine-cdn-pull.* /etc/systemd/system/
# edit the .service User= / paths if the CDN account isn't `notroot`
sudo systemctl daemon-reload
sudo systemctl enable --now moraine-cdn-pull.timer
sudo systemctl start moraine-cdn-pull.service    # publish the current release now
```

Verify the repo-path defaults at the top of `cdn-publish.sh` match the real server
first. To force an immediate publish later: `cdn-pull.sh --force`.

## Downstream packaging recipes (automatic)

After the packages are built, `release.yml`'s **`recipes`** job runs
[`deploy/bump-recipes.sh`](../deploy/bump-recipes.sh), which bumps every downstream
recipe (AUR, Homebrew, nixpkgs, Alpine, Scoop, Chocolatey, winget, RPM, Snap,
Flatpak, FreeBSD, …), refreshes the source `sha256`/`sha512` and the Windows-zip
`sha256` from the just-built release, renames the Gentoo ebuild, adds RPM/Flatpak
release notes, and commits the result back to `main`. It's idempotent, so a
re-dispatched build is a no-op. (Run `deploy/bump-recipes.sh <version>` by hand as a
fallback.)

So a release is now a single command — `deploy/bump.sh <version>` — and the rest
(build, recipe bump, CDN publish, website) follows automatically.

## The one thing still manual

The vendored crate lists (`cargo-sources.json`, Gentoo `CRATES`, FreeBSD
`CARGO_CRATES`) are left untouched — regenerate them with their platform tools only
when `Cargo.lock`'s dependencies actually change.
