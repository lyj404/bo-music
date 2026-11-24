param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Binary = 'target/release/bo-music.exe',
    [string]$OutputDirectory = 'target/dist'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$') {
    throw 'Version must be MAJOR.MINOR.PATCH with an optional prerelease suffix.'
}
$coreVersion = ($Version -split '-', 2)[0]
$root = Split-Path $PSScriptRoot -Parent
$binaryPath = (Resolve-Path -LiteralPath $Binary).Path
$runtimePath = Join-Path $root 'libmpv/libmpv-2.dll'
if (-not (Test-Path -LiteralPath $runtimePath -PathType Leaf)) { throw 'libmpv-2.dll is missing.' }
$candidates = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
)
$isccCommand = Get-Command ISCC.exe -ErrorAction SilentlyContinue
if ($isccCommand) { $candidates += $isccCommand.Source }
$iscc = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $iscc) { throw 'Install Inno Setup 6.5 or newer before packaging.' }
$languageFile = Join-Path $root 'installer/languages/ChineseSimplified.isl'
if (-not (Test-Path -LiteralPath $languageFile -PathType Leaf)) { throw "The bundled Inno Setup Simplified Chinese translation is missing: $languageFile" }
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDirectory).Path
$name = "bo-music_${Version}_windows_x64"
$bundle = Join-Path $output $name
$zip = Join-Path $output "$name.zip"
$setup = Join-Path $output "${name}_setup.exe"
foreach ($path in @($bundle, $zip, $setup)) {
    if (Test-Path -LiteralPath $path) { throw "Output already exists: $path" }
}
New-Item -ItemType Directory -Path $bundle | Out-Null
Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $bundle 'bo-music.exe')
Copy-Item -LiteralPath $runtimePath -Destination $bundle
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $bundle
Copy-Item -LiteralPath (Join-Path $root 'libmpv/README.md') -Destination (Join-Path $bundle 'libmpv-README.md')
# A video-enabled libmpv may be supplied alongside the audio runtime.
$videoRuntime = Join-Path $root 'libmpv/libmpv-video-2.dll'
if (Test-Path -LiteralPath $videoRuntime) { Copy-Item -LiteralPath $videoRuntime -Destination $bundle }
Compress-Archive -Path $bundle -DestinationPath $zip -CompressionLevel Optimal
& $iscc "/DAPP_VERSION=$Version" "/DAPP_VERSION_CORE=$coreVersion" "/DSOURCE_DIR=$bundle" "/DOUTPUT_DIR=$output" (Join-Path $root 'installer/bo_music.iss')
if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed: $LASTEXITCODE" }
foreach ($path in @($zip, $setup)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Package was not generated: $path" }
}
