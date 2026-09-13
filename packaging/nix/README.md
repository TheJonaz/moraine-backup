# Nix packaging for Moraine

The flake now lives at the **repo root** (`/flake.nix`), so it works straight from
GitHub with no per-release hash to maintain:

```bash
# Run the CLI (headless-friendly)
nix run github:TheJonaz/moraine-backup

# Launch the GTK desktop app
nix run github:TheJonaz/moraine-backup#gui

# Install into a profile
nix profile install github:TheJonaz/moraine-backup

# Build from a local checkout
nix build .
```

It builds the CLI **and** the GTK desktop app, wrapping both so `rsync`, `ssh`
and `rclone` are on their runtime `PATH`.

> **Requires moraine ≥ 0.1.19** — asset lookup uses `XDG_DATA_DIRS`, which
> `wrapGAppsHook4` points at the package's `share/` directory.

## Why no hashes any more

The old flake used `fetchFromGitHub` + `cargoHash`, both of which had to be
re-pinned on every release (and were shipped as `lib.fakeHash` placeholders, so
they never actually built). The root flake instead uses:

- `src = self;` — builds the flake's own tree, so there is no source hash;
- `cargoLock.lockFile = ./Cargo.lock;` — vendors deps from the lockfile (all
  crates.io, no git sources), so there is no `cargoHash`;
- `version` read from `Cargo.toml`, so releases need no flake edit at all.

This flake is **separate from the nixpkgs submission** (`packaging/nixpkgs/`) —
it lets Nix users run moraine today without waiting on the nixpkgs review.

> Not yet verified with an actual `nix build` from this repo (no Nix on the dev
> box). Run `nix build .` once to confirm; if it builds, nothing here drifts.
