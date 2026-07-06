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

Pushing the tag triggers [`release.yml`](../.github/workflows/release.yml):

1. Builds the per-OS archives, the `.deb`, `.rpm` and `.pkg.tar.zst`, and attaches
   them to the GitHub Release.
2. The **`cdn`** job downloads those packages and runs
   [`deploy/cdn-publish.sh`](../deploy/cdn-publish.sh) on `cdn.thern.io`, refreshing
   the apt/dnf/pacman repo metadata — so the CDN serves the new version
   automatically. This job is a no-op unless the `CDN_SSH_KEY` / `CDN_HOST` /
   `CDN_USER` repo secrets are set (see the header of `release.yml`).

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
