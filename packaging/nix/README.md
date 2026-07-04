# Nix packaging for Moraine

A flake that builds the CLI **and** the GTK desktop app, wrapping both so
`rsync`, `ssh` and `rclone` are on their runtime `PATH`.

> **Requires moraine ≥ 0.1.19** — asset lookup uses `XDG_DATA_DIRS`, which
> `wrapGAppsHook4` points at the package's `share/` directory.

## Build & run

```sh
# from this directory
nix build .#moraine
./result/bin/moraine --version
nix run .#           # launches the GTK app (moraine-gui)
```

Install into your profile:

```sh
nix profile install github:TheJonaz/moraine-backup?dir=packaging/nix
```

## Pinning the hashes

`src.hash` and `cargoHash` start as `lib.fakeHash`. On the first build Nix fails
with the **real** hashes in the error — paste each into `flake.nix` and rebuild:

```sh
nix build .#moraine 2>&1 | grep -E 'got:|specified:'
```

Do `src.hash` first (fix, rebuild), then `cargoHash`.

## On each new release

1. Bump `version` (drives the `v${version}` git tag fetched by `fetchFromGitHub`).
2. Reset both hashes to `pkgs.lib.fakeHash` and re-pin them as above.

## NixOS / home-manager

Add the flake as an input and reference
`inputs.moraine.packages.${system}.default` in `environment.systemPackages` or
`home.packages`.
