$ErrorActionPreference = 'Stop'

$packageName = 'moraine'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64       = 'https://github.com/TheJonaz/moraine-backup/releases/download/v0.3.1/moraine-windows-x86_64.zip'
$checksum64  = 'c4d94f2ee160cdf99fb1e209f197f95098b8c119e64424fb71203cf48e47dd38'

Install-ChocolateyZipPackage `
  -PackageName    $packageName `
  -Url64bit       $url64 `
  -Checksum64     $checksum64 `
  -ChecksumType64 'sha256' `
  -UnzipLocation  $toolsDir

# Chocolatey auto-shims moraine.exe (unzipped to tools\moraine-windows-x86_64\)
# onto the PATH, so `moraine` works from any shell.
