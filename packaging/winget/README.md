# winget packaging for Moraine

Manifests for the Windows Package Manager (`winget`), which is built into
Windows 10/11. Publishing these to the community repo
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) makes Moraine
installable with:

```powershell
winget install TheJonaz.Moraine
```

The package ships the **`moraine` command-line client** only — the GTK desktop
app is Linux-only. It's a `zip` installer with a `portable` nested `moraine.exe`,
so winget adds it to `PATH` and there's nothing to uninstall but the alias.

## Files

| File | Purpose |
|------|---------|
| `TheJonaz.Moraine.yaml` | version manifest (points at the default locale) |
| `TheJonaz.Moraine.installer.yaml` | installer type, URL and SHA-256 |
| `TheJonaz.Moraine.locale.en-US.yaml` | name, description, license, tags |

## Validate & test locally

```powershell
winget validate --manifest packaging\winget
# sandbox test (needs Windows Sandbox enabled):
winget install --manifest packaging\winget
```

## Publish / update

The community repo lays manifests out under
`manifests/t/TheJonaz/Moraine/<version>/`. Easiest is [`wingetcreate`]:

```powershell
# Auto-bump URL + version and refresh the SHA-256 from the new release asset:
wingetcreate update TheJonaz.Moraine `
  --version 0.1.18 `
  --urls https://github.com/TheJonaz/moraine-backup/releases/download/v0.1.18/moraine-windows-x86_64.zip `
  --submit
```

Or copy the files into a `winget-pkgs` fork under the versioned path, bump
`PackageVersion` in all three files, refresh `InstallerSha256`
(`Get-FileHash <zip> -Algorithm SHA256`), and open a PR.

[`wingetcreate`]: https://github.com/microsoft/winget-create

## On each new release

1. Publish the `moraine-windows-x86_64.zip` release asset for the new tag.
2. Bump `PackageVersion` in all three manifests and `ReleaseDate` in the
   installer manifest.
3. Refresh `InstallerSha256` and the `InstallerUrl` version.
4. `winget validate`, then submit to `microsoft/winget-pkgs`.
