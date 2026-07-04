$ErrorActionPreference = 'Stop'

$packageName = 'moraine'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64       = 'https://github.com/TheJonaz/moraine-backup/releases/download/v0.1.19/moraine-windows-x86_64.zip'
$checksum64  = '7afb6df8b645ff54d4414da6dcc11781bc84b7263990553b55df497eaf135e71'

Install-ChocolateyZipPackage `
  -PackageName    $packageName `
  -Url64bit       $url64 `
  -Checksum64     $checksum64 `
  -ChecksumType64 'sha256' `
  -UnzipLocation  $toolsDir

# Chocolatey auto-shims moraine.exe (unzipped to tools\moraine-windows-x86_64\)
# onto the PATH, so `moraine` works from any shell.
