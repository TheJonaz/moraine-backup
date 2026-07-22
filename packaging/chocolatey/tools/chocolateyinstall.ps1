$ErrorActionPreference = 'Stop'

$packageName = 'moraine'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64       = 'https://github.com/TheJonaz/moraine-backup/releases/download/v0.2.2/moraine-windows-x86_64.zip'
$checksum64  = 'd5cbb9a588b446720acb7b7851aab6fe7270ae450f4c4f53c86dab29069ebe91'

Install-ChocolateyZipPackage `
  -PackageName    $packageName `
  -Url64bit       $url64 `
  -Checksum64     $checksum64 `
  -ChecksumType64 'sha256' `
  -UnzipLocation  $toolsDir

# Chocolatey auto-shims moraine.exe (unzipped to tools\moraine-windows-x86_64\)
# onto the PATH, so `moraine` works from any shell.
