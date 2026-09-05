$ErrorActionPreference = 'Stop'

$packageName = 'moraine'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64       = 'https://github.com/TheJonaz/moraine-backup/releases/download/v0.3.0/moraine-windows-x86_64.zip'
$checksum64  = '0f3049226ba159504758912f29dd04b5fa3c0dd420ba996da3b0e4a9de0a6676'

Install-ChocolateyZipPackage `
  -PackageName    $packageName `
  -Url64bit       $url64 `
  -Checksum64     $checksum64 `
  -ChecksumType64 'sha256' `
  -UnzipLocation  $toolsDir

# Chocolatey auto-shims moraine.exe (unzipped to tools\moraine-windows-x86_64\)
# onto the PATH, so `moraine` works from any shell.
