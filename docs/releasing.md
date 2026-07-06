# Cutting a release

One command bumps the version, tags and pushes; CI does the rest.

```bash
deploy/bump.sh 0.1.23        # add --dry-run first to preview every edit
```

`bump.sh` rewrites the version in `Cargo.toml`/`Cargo.lock`, stamps the
`## [Unreleased]` heading in `CHANGELOG.md` with the version and date, updates the
install commands in `README.md` and every version label + `cdn.thern.io` download
URL in `site/index.html`, then commits `release: vX.Y.Z`, tags `vX.Y.Z` and pushes
(it asks first; `-y` skips the prompt, `--no-push` stops after the commit).

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

## Two manual follow-ups

`bump.sh` deliberately leaves these to you, because neither can be done correctly
before the tag exists:

- **Deploy the website.** `site/` is gitignored and hosted separately. Publish the
  updated `site/index.html` to `moraine.thern.io` *after* `release.yml` has pushed
  the new packages to the CDN — otherwise the download buttons 404.
- **Refresh downstream packaging recipes.** The AUR / Homebrew / nixpkgs / … recipes
  under `packaging/` pin source and binary checksums that can only be computed from
  the published tag tarball. Update their versions and checksums with the platform
  tools once the tag is live.
