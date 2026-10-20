# Called from WSL with ./build-windows.sh; Cargo arguments such as --release are forwarded.
$ErrorActionPreference = 'Stop'
$cargoArgs = @($args)
$projectDir = Split-Path -Parent $PSScriptRoot
$buildDir = Join-Path $env:TEMP 'sm_todo-build'
$sourceDir = Join-Path $buildDir 'source'
$targetDir = Join-Path $buildDir 'target'

New-Item -ItemType Directory -Force -Path $sourceDir | Out-Null
# Replace the previous source snapshot so deleted Rust files cannot linger.
$stagedSrc = Join-Path $sourceDir 'src'
if (Test-Path -LiteralPath $stagedSrc) {
    Remove-Item -LiteralPath $stagedSrc -Recurse -Force
}
Copy-Item -LiteralPath (Join-Path $projectDir 'src') -Destination $sourceDir -Recurse
Copy-Item -LiteralPath (Join-Path $projectDir 'Cargo.lock') -Destination $sourceDir
Copy-Item -LiteralPath (Join-Path $projectDir 'Cargo.toml') -Destination $sourceDir

$env:CARGO_TARGET_DIR = $targetDir
Write-Host "Windows build source: $sourceDir"
Write-Host "Windows target: $targetDir"
Push-Location $sourceDir
try {
    & cargo build --locked @cargoArgs
    $buildExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $buildExitCode
