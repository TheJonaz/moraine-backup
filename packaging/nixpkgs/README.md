# nixpkgs submission for Moraine

`package.nix` is a nixpkgs-style derivation (callPackage form) for submitting
Moraine to [NixOS/nixpkgs](https://github.com/NixOS/nixpkgs), so users get it via
`nix-env`, `environment.systemPackages`, or `nix run nixpkgs#moraine`.

> This is separate from `../nix/flake.nix` (which is for building straight from
> this repo). Use *this* file for the upstream nixpkgs PR.

## Add it to a nixpkgs checkout

```sh
mkdir -p pkgs/by-name/mo/moraine
cp /path/to/moraine-backup/packaging/nixpkgs/package.nix pkgs/by-name/mo/moraine/
# pin the hashes (see below), then:
nix-build -A moraine
```

`pkgs/by-name/` is auto-discovered, so no edit to `all-packages.nix` is needed.

## Pinning the hashes

Both `src.hash` and `cargoHash` start as `lib.fakeHash`; the build fails with the
real values, which you paste back in:

```sh
nix-build -A moraine 2>&1 | grep -E 'got:|specified:'
```

Fix `src.hash` first (rebuild), then `cargoHash`. Add yourself under
`meta.maintainers` before opening the PR.

## On each new release

Bump `version` (drives the `v${version}` tag) and reset both hashes to
`lib.fakeHash`, then re-pin.
