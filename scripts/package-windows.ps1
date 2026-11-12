# Builds the Windows downloads from an already built release binary (cargo build --release):
#
#   pwsh scripts/package-windows.ps1
#
# Writes into dist/: the installer sdterm-native-<version>-win-x64-setup.exe (NSIS), a plain .zip,
# the update feed latest-win.json and the checksums. Meant for CI (windows-latest). The files are
# not code signed: Windows SmartScreen may warn the first time, "More info", "Run anyway".
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $root

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.*)"').Matches[0].Groups[1].Value
$exe = Join-Path $root 'target\release\sdterm-native.exe'
if (-not (Test-Path $exe)) { throw 'Build first: cargo build --release' }
$out = Join-Path $root 'dist'
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force -Path $out | Out-Null

# The plain zip: the program with its resources folder beside it (where the app looks for it).
$name = "sdterm-native-$version-win-x64"
$stage = Join-Path $out $name
New-Item -ItemType Directory -Force -Path "$stage\resources" | Out-Null
Copy-Item $exe $stage
Copy-Item -Recurse 'resources\shell' "$stage\resources\shell"
Copy-Item 'LICENSE', 'README.md' $stage
Compress-Archive -Path $stage -DestinationPath "$out\$name.zip" -Force
Remove-Item -Recurse -Force $stage

# The installer.
$makensis = $null
$found = Get-Command makensis -ErrorAction SilentlyContinue
if ($found) { $makensis = $found.Source }
if (-not $makensis) {
  foreach ($dir in @($env:ProgramFiles, ${env:ProgramFiles(x86)})) {
    $candidate = Join-Path $dir 'NSIS\makensis.exe'
    if (Test-Path $candidate) { $makensis = $candidate; break }
  }
}
if (-not $makensis) {
  choco install nsis -y --no-progress
  $makensis = Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe'
}
Write-Host "makensis: $makensis"
$setup = "sdterm-native-$version-win-x64-setup.exe"
& $makensis /V2 "/DROOT=$root" "/DVERSION=$version" "/DOUTFILE=$out\$setup" "$root\scripts\windows\installer.nsi"
if ($LASTEXITCODE -ne 0) { throw "makensis failed with exit code $LASTEXITCODE" }

# The feed the updater reads, and the checksums.
node scripts/make-feed.mjs win $out $setup x64
if ($LASTEXITCODE -ne 0) { throw 'make-feed failed' }
Get-ChildItem $out | Format-Table Name, Length
