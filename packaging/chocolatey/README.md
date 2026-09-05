# Chocolatey packaging for Moraine (Windows CLI)

A [Chocolatey](https://community.chocolatey.org) package for the `moraine`
command-line client (the GTK desktop app is Linux-only). It downloads the
Windows release zip and shims `moraine.exe` onto the PATH.

```powershell
choco install moraine
```

## Files

| File | Purpose |
|------|---------|
| `moraine.nuspec` | package metadata |
| `tools/chocolateyinstall.ps1` | downloads + unzips the release, with pinned SHA-256 |
| `build-nupkg.py` | builds the `.nupkg` on Linux, where `choco pack` does not run |

## Build

`choco pack` is Windows-only, so there is a second route. Both produce the same
package; the payload here is a downloader script, not compiled artefacts, so
nothing about the build host ends up in it.

```powershell
cd packaging\chocolatey        # on Windows
choco pack
choco install moraine -source . -y
moraine --version
```

```sh
./packaging/chocolatey/build-nupkg.py       # on Linux, no toolchain needed
```

A `.nupkg` is an OPC zip: the nuspec, `tools/`, and three bookkeeping parts
(`[Content_Types].xml`, `_rels/.rels`, a `.psmdcp`) that NuGet clients require.
[`build-nupkg.py`](build-nupkg.py) writes them directly. The output is
reproducible — rebuilding the same sources gives a byte-identical file, so what
was pushed can be checked later.

Only a Windows host can actually *test* the install; the Linux route is for
building and pushing.

## Publish / update

Pushing is a plain HTTP PUT with the API key from
<https://community.chocolatey.org/account>, so it also works without choco:

```powershell
choco apikey --key <YOUR_KEY> --source https://push.chocolatey.org/
choco push moraine.0.3.0.nupkg --source https://push.chocolatey.org/
```

```sh
curl -H "X-NuGet-ApiKey: $CHOCO_API_KEY" \
     -T packaging/chocolatey/moraine.0.3.0.nupkg \
     https://push.chocolatey.org/api/v2/package
```

A `201` means accepted, not published: the first version of a new package goes
through automated validation and then human moderation, and lands in the
`unlisted`/pending state until a moderator approves it.

## On each new release

- `<version>` in the nuspec
- `$url64` + `$checksum64` in `tools/chocolateyinstall.ps1`
  (`sha256sum` on the release zip, or `Get-FileHash <zip> -Algorithm SHA256`)
- the tag in `<iconUrl>` — it is pinned to a release tag so the icon cannot
  change under an approved package, which means it needs bumping like the rest
