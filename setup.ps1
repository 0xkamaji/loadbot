$ErrorActionPreference = "Stop"

function Require-Command {
    param([string]$Name)

    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "'$Name' is required but was not found in PATH"
    }
}

Write-Host "Checking Loadbot prerequisites..."
Require-Command "cargo"
Require-Command "rustc"
Require-Command "git"

$ProjectDir = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not (Test-Path (Join-Path $ProjectDir "Cargo.toml") -PathType Leaf)) {
    throw "Cargo.toml was not found in $ProjectDir"
}

if ($env:CARGO_HOME) {
    $InstallRoot = $env:CARGO_HOME
} else {
    $InstallRoot = Join-Path $HOME ".cargo"
}
$InstallBin = Join-Path $InstallRoot "bin"
$LoadbotExe = Join-Path $InstallBin "loadbot.exe"
$CompletionDir = Join-Path $InstallRoot "completions"

Write-Host "Installing Loadbot from source..."
& cargo install --path $ProjectDir --root $InstallRoot --locked --force
if ($LASTEXITCODE -ne 0) {
    throw "Cargo failed to install Loadbot"
}
if (-not (Test-Path $LoadbotExe -PathType Leaf)) {
    throw "Cargo completed, but $LoadbotExe was not created"
}

& $LoadbotExe --help *> $null
if ($LASTEXITCODE -ne 0) {
    throw "Loadbot was installed but failed its verification check"
}

Write-Host "Generating shell completion scripts..."
New-Item -ItemType Directory -Force -Path $CompletionDir | Out-Null
$PreviousComplete = $env:COMPLETE
try {
    foreach ($Shell in @("bash", "zsh", "fish", "powershell")) {
        $env:COMPLETE = $Shell
        $Extension = if ($Shell -eq "powershell") { "ps1" } else { $Shell }
        $Destination = Join-Path $CompletionDir "loadbot.$Extension"
        & $LoadbotExe | Set-Content -Encoding utf8 $Destination
        if ($LASTEXITCODE -ne 0) {
            throw "Loadbot failed to generate $Shell completions"
        }
    }
} finally {
    $env:COMPLETE = $PreviousComplete
}

Write-Host "Loadbot installed successfully:"
Write-Host "  $LoadbotExe"
Write-Host "Completion scripts generated in:"
Write-Host "  $CompletionDir"
Write-Host "Dot-source the PowerShell script for this session:"
Write-Host "  . `"$(Join-Path $CompletionDir 'loadbot.ps1')`""
Write-Host "Loadbot did not modify your PowerShell profile."

$PathEntries = $env:PATH -split ";"
if ($PathEntries -contains $InstallBin) {
    Write-Host ""
    Write-Host "Run:"
    Write-Host "  loadbot --help"
} else {
    Write-Host ""
    Write-Host "Cargo's binary directory is not currently in PATH."
    Write-Host "Add this directory to your user PATH when convenient:"
    Write-Host "  $InstallBin"
    Write-Host ""
    Write-Host "Loadbot did not modify your PowerShell profile or PATH."
    Write-Host "Until then, run Loadbot directly:"
    Write-Host "  & `"$LoadbotExe`" --help"
}
