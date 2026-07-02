# Scoop manifest for the Moraine CLI

`moraine.json` installs the **command-line client** on Windows via
[Scoop](https://scoop.sh). It downloads the prebuilt `moraine.exe` from the
GitHub release (no compiling). The GTK desktop app is Linux-only.

## Publish via a bucket

A Scoop *bucket* is just a GitHub repo (conventionally `scoop-<name>` with a
`bucket/` directory):

```sh
# 1. Create the bucket repo (once)
gh repo create TheJonaz/scoop-moraine --public -d "Scoop bucket for Moraine"

# 2. Add the manifest
git clone https://github.com/TheJonaz/scoop-moraine
cd scoop-moraine
mkdir -p bucket
cp /path/to/moraine-backup/packaging/scoop/moraine.json bucket/
git add bucket/moraine.json
git commit -m "moraine 0.1.17"
git push
```

Users then install with:

```powershell
scoop bucket add moraine https://github.com/TheJonaz/scoop-moraine
scoop install moraine
```

## On each new release

The manifest has `checkver`/`autoupdate` set to the GitHub releases, so on an
Arch/WSL/any machine with Scoop's tooling you can run
`scoop update` on the bucket, or bump `version` + `hash` manually
(`sha256sum moraine-windows-x86_64.zip`) and push.
