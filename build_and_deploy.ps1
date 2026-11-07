# Build release binary and copy to cadcraftPrototyp
$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "Building release binary..." -ForegroundColor Cyan
cargo build --release --manifest-path "$ScriptDir\Cargo.toml" -p cadcraft

$Src = "$ScriptDir\target\release\cadcraft.exe"
$Dest = "$ScriptDir\..\cadcraftPrototyp\cadcraft.exe"

Write-Host "Copying $Src to $Dest..." -ForegroundColor Cyan
Copy-Item -Path $Src -Destination $Dest -Force

Write-Host "Successfully deployed cadcraft.exe to cadcraftPrototyp!" -ForegroundColor Green

