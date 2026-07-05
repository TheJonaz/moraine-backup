$ErrorActionPreference = 'Stop'

$packageName = 'moraine'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64       = 'https://github.com/TheJonaz/moraine-backup/releases/download/v0.1.22/moraine-windows-x86_64.zip'
$checksum64  = '480a23d3441e99411baf17146acd4899577c84f960191bfa25fb725d3b32c6fb'

Install-ChocolateyZipPackage `
  -PackageName    $packageName `
  -Url64bit       $url64 `
  -Checksum64     $checksum64 `
  -ChecksumType64 'sha256' `
  -UnzipLocation  $toolsDir

# Chocolatey auto-shims moraine.exe (unzipped to tools\moraine-windows-x86_64\)
# onto the PATH, so `moraine` works from any shell.
