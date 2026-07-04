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

## Build & test locally

```powershell
cd packaging\chocolatey
choco pack
choco install moraine -source . -y
moraine --version
```

## Publish / update

```powershell
choco apikey --key <YOUR_KEY> --source https://push.chocolatey.org/
choco push moraine.0.1.19.nupkg --source https://push.chocolatey.org/
```

Community-repo submissions go through automated + human moderation on the first
version.

## On each new release

Bump `<version>` in the nuspec, and update `$url64` + `$checksum64` in
`tools/chocolateyinstall.ps1` (`Get-FileHash <zip> -Algorithm SHA256`).
