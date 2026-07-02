# Homebrew formula for the Moraine CLI

`moraine.rb` installs the **command-line client** on macOS and Linux (Homebrew).
It builds from the release tag with Cargo and drops just the `moraine` binary
(the GTK desktop app is Linux-only).

## Publish via a tap

A Homebrew *tap* is just a GitHub repo named `homebrew-<something>`:

```sh
# 1. Create the tap repo (once)
gh repo create TheJonaz/homebrew-moraine --public \
  -d "Homebrew tap for Moraine"

# 2. Add the formula
git clone https://github.com/TheJonaz/homebrew-moraine
cd homebrew-moraine
mkdir -p Formula
cp /path/to/moraine-backup/packaging/homebrew/moraine.rb Formula/
git add Formula/moraine.rb
git commit -m "moraine 0.1.17"
git push
```

Users then install with:

```sh
brew install TheJonaz/moraine/moraine
# or:  brew tap TheJonaz/moraine && brew install moraine
```

## On each new release

1. Update `url` to the new `vX.Y.Z` tag and refresh `sha256`
   (`curl -sL <tarball-url> | sha256sum`).
2. Copy the formula into the tap and push.

`brew test moraine` / `brew audit --strict moraine` require a machine with
Homebrew; the `cargo install --no-default-features` build step is validated on
Linux, but the macOS bottle should be built/tested on a Mac.
