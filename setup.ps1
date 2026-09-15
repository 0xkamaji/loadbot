[CmdletBinding()]
param(
    [switch]$Cli,
    [switch]$Gui,
    [switch]$All,
    [switch]$Repair
)

$ErrorActionPreference = "Stop"

$script:StartMarker = "# >>> loadbot >>>"
$script:EndMarker = "# <<< loadbot <<<"

function Get-LoadbotCommand {
    param([Parameter(Mandatory)][string]$Name)
    Get-Command $Name -ErrorAction SilentlyContinue
}

function Get-LoadbotUserPath {
    [Environment]::GetEnvironmentVariable("Path", "User")
}

function Set-LoadbotUserPath {
    param([Parameter(Mandatory)][AllowEmptyString()][string]$Value)
    [Environment]::SetEnvironmentVariable("Path", $Value, "User")
}

function Get-LoadbotMachinePath {
    [Environment]::GetEnvironmentVariable("Path", "Machine")
}

function Sync-LoadbotProcessPath {
    foreach ($pathValue in @((Get-LoadbotMachinePath), (Get-LoadbotUserPath))) {
        foreach ($entry in ($pathValue -split [IO.Path]::PathSeparator)) {
            if (-not [string]::IsNullOrWhiteSpace($entry) -and -not (Test-LoadbotPathContains $env:PATH $entry)) {
                $env:PATH = if ([string]::IsNullOrEmpty($env:PATH)) { $entry } else { "$env:PATH$([IO.Path]::PathSeparator)$entry" }
            }
        }
    }
}

function Get-LoadbotProfilePath {
    $PROFILE.CurrentUserCurrentHost
}

function Test-LoadbotInteractive {
    [Environment]::UserInteractive -and
        -not [Console]::IsInputRedirected -and
        -not [Console]::IsOutputRedirected
}

function Invoke-LoadbotWinget {
    param([Parameter(Mandatory)][string[]]$Arguments)
    & winget @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "winget failed with exit code $LASTEXITCODE"
    }
}

function Invoke-LoadbotRustup {
    param(
        [Parameter(Mandatory)][string]$RustupExe,
        [Parameter(Mandatory)][string[]]$Arguments
    )
    & $RustupExe @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "rustup failed with exit code $LASTEXITCODE"
    }
}

function Invoke-LoadbotCargoInstall {
    param(
        [Parameter(Mandatory)][string]$CargoExe,
        [Parameter(Mandatory)][string]$ProjectDir,
        [Parameter(Mandatory)][string]$InstallRoot
    )
    & $CargoExe install --path $ProjectDir --root $InstallRoot --locked --force
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo failed to install Loadbot (exit code $LASTEXITCODE)"
    }
}

function Invoke-LoadbotExecutable {
    param(
        [Parameter(Mandatory)][string]$Executable,
        [Parameter(Mandatory)][AllowEmptyCollection()][string[]]$Arguments,
        [switch]$Capture
    )
    if ($Capture) {
        $output = & $Executable @Arguments
    } else {
        & $Executable @Arguments
    }
    if ($LASTEXITCODE -ne 0) {
        throw "$Executable failed with exit code $LASTEXITCODE"
    }
    if ($Capture) { $output }
}

function Test-LoadbotNodeSupported {
    $node = Get-LoadbotCommand "node"
    if (-not $node) { return $false }
    try {
        $version = (& $node.Source --version 2>$null).TrimStart("v")
        $parsed = [version]$version
        $parsed.Major -gt 22 -or ($parsed.Major -eq 22 -and $parsed.Minor -ge 12)
    } catch { $false }
}

function Test-LoadbotWindowsBuildTools {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $vswhere -PathType Leaf)) { return $false }
    $installation = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    -not [string]::IsNullOrWhiteSpace(($installation | Select-Object -First 1))
}

function Test-LoadbotWebView2 {
    foreach ($key in @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        "HKCU:\Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
    )) {
        if (Test-Path $key) {
            $version = (Get-ItemProperty -LiteralPath $key -Name pv -ErrorAction SilentlyContinue).pv
            if ($version -and $version -ne "0.0.0.0") { return $true }
        }
    }
    $false
}

function Test-LoadbotFrontendDependencies {
    param([Parameter(Mandatory)][string]$GuiRoot)
    $lockfile = Join-Path $GuiRoot "package-lock.json"
    $marker = Join-Path $GuiRoot "node_modules\.loadbot-package-lock.json"
    $api = Join-Path $GuiRoot "node_modules\@tauri-apps\api\package.json"
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf) -or -not (Test-Path -LiteralPath $api -PathType Leaf)) {
        return $false
    }
    (Get-FileHash -LiteralPath $lockfile -Algorithm SHA256).Hash -eq
        (Get-FileHash -LiteralPath $marker -Algorithm SHA256).Hash
}

function Assert-LoadbotConfigurationDirectories {
    $dataRoot = if ($env:LOADBOT_HOME) { $env:LOADBOT_HOME } else { Join-Path $env:LOCALAPPDATA "loadbot" }
    $configRoot = if ($env:LOADBOT_CONFIG_HOME) { $env:LOADBOT_CONFIG_HOME } else { Join-Path $env:APPDATA "loadbot" }
    foreach ($path in @($dataRoot, $configRoot)) {
        if (Test-Path -LiteralPath $path) {
            $item = Get-Item -LiteralPath $path -Force
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Loadbot configuration path is not a normal directory: $path"
            }
        }
    }
}

function Set-LoadbotInstallMode {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][ValidateSet("cli", "gui", "all")][string]$Mode
    )
    if (Test-Path -LiteralPath $Path) {
        $item = Get-Item -LiteralPath $Path -Force
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Loadbot installation record is not a normal file: $Path"
        }
    }
    $temporary = "$Path.$([Guid]::NewGuid().ToString('N')).tmp"
    try {
        [IO.File]::WriteAllText($temporary, "$Mode`n", [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporary -Destination $Path -Force
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Get-LoadbotInstalledFileState {
    param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return "missing" }
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        return "installed"
    }
    "unsafe"
}

function Get-LoadbotManagedIntegration {
    param(
        [Parameter(Mandatory)][string]$ProfilePath,
        [Parameter(Mandatory)][string]$CompletionName
    )
    if (-not (Test-Path -LiteralPath $ProfilePath)) {
        return [pscustomobject]@{ State = "missing"; CompletionReferenced = $false }
    }
    $item = Get-Item -LiteralPath $ProfilePath -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        return [pscustomobject]@{ State = "unsafe"; CompletionReferenced = $false }
    }
    $text = [IO.File]::ReadAllText($ProfilePath)
    $starts = [regex]::Matches($text, "(?m)^$([regex]::Escape($script:StartMarker))\r?$")
    $ends = [regex]::Matches($text, "(?m)^$([regex]::Escape($script:EndMarker))\r?$")
    if ($starts.Count -eq 0 -and $ends.Count -eq 0) {
        return [pscustomobject]@{ State = "missing"; CompletionReferenced = $false }
    }
    if ($starts.Count -ne 1 -or $ends.Count -ne 1 -or $starts[0].Index -ge $ends[0].Index) {
        return [pscustomobject]@{ State = "unsafe"; CompletionReferenced = $false }
    }
    [pscustomobject]@{
        State = "configured"
        CompletionReferenced = $text.Contains($CompletionName)
    }
}

function Select-LoadbotRepairMode {
    param([Parameter(Mandatory)][string]$NoninteractiveReason)
    if (-not (Test-LoadbotInteractive)) {
        throw "$NoninteractiveReason; rerun interactively or use -Cli, -Gui, or -All to select the installation explicitly"
    }
    Write-Host ""
    Write-Host "Which installation should Loadbot repair?"
    Write-Host "  1. CLI only"
    Write-Host "  2. GUI only"
    Write-Host "  3. CLI + GUI"
    Write-Host "  4. Cancel"
    $selection = Read-Host ">"
    switch ($selection) {
        "1" { "cli" }
        "2" { "gui" }
        "3" { "all" }
        "4" { throw "Setup cancelled; no changes were made" }
        default { throw "Invalid repair selection '$selection'" }
    }
}

function Resolve-LoadbotLegacyRepairMode {
    param(
        [Parameter(Mandatory)][string]$InstallBin,
        [Parameter(Mandatory)][string]$LoadbotExe,
        [Parameter(Mandatory)][string]$GuiExe,
        [Parameter(Mandatory)][string]$CompletionPath,
        [Parameter(Mandatory)][string]$ProfilePath
    )
    $cliState = Get-LoadbotInstalledFileState $LoadbotExe
    $guiState = Get-LoadbotInstalledFileState $GuiExe
    $integration = Get-LoadbotManagedIntegration -ProfilePath $ProfilePath -CompletionName "loadbot.ps1"
    $completionState = Get-LoadbotInstalledFileState $CompletionPath
    if ($completionState -eq "installed") { $completionState = "generated" }
    $pathState = if (Test-LoadbotPathContains (Get-LoadbotUserPath) $InstallBin) { "configured" } else { "missing" }
    $reachableState = if (Get-LoadbotCommand "loadbot") { "reachable" } else { "not-reachable" }
    $dataRoot = if ($env:LOADBOT_HOME) { $env:LOADBOT_HOME } else { Join-Path $env:LOCALAPPDATA "loadbot" }
    $configRoot = if ($env:LOADBOT_CONFIG_HOME) { $env:LOADBOT_CONFIG_HOME } else { Join-Path $env:APPDATA "loadbot" }
    $dataState = if (Test-Path -LiteralPath $dataRoot) { "present" } else { "missing" }
    $configState = if (Test-Path -LiteralPath $configRoot) { "present" } else { "missing" }

    Write-Host "No Loadbot installation record was found."
    Write-Host ""
    Write-Host "Existing Loadbot state detected:"
    Write-Host ("  {0,-22} {1}" -f "CLI executable:", $cliState)
    Write-Host ("  {0,-22} {1}" -f "GUI executable:", $guiState)
    Write-Host ("  {0,-22} {1}" -f "PATH integration:", $pathState)
    Write-Host ("  {0,-22} {1}" -f "Managed profile block:", $integration.State)
    Write-Host ("  {0,-22} {1}" -f "PowerShell completion:", $completionState)
    Write-Host ("  {0,-22} {1}" -f "loadbot on PATH:", $reachableState)
    Write-Host ("  {0,-22} {1}" -f "Data directory:", $dataState)
    Write-Host ("  {0,-22} {1}" -f "Config directory:", $configState)

    $inferred = $null
    if ($cliState -eq "installed" -and $guiState -eq "missing") {
        $inferred = "cli"
    } elseif ($cliState -eq "installed" -and $guiState -eq "installed") {
        if ($integration.State -eq "configured" -and $completionState -eq "generated" -and $integration.CompletionReferenced) {
            $inferred = "all"
        } elseif ($integration.State -eq "configured" -and $completionState -eq "missing" -and -not $integration.CompletionReferenced) {
            $inferred = "gui"
        } elseif ($pathState -eq "configured" -and $integration.State -eq "missing" -and $completionState -eq "missing") {
            $inferred = "gui"
        }
    }

    $meaningful = $cliState -ne "missing" -or $guiState -ne "missing" -or
        $pathState -eq "configured" -or $integration.State -ne "missing" -or
        $completionState -ne "missing" -or $reachableState -eq "reachable"

    if ($inferred) {
        $inferredLabel = switch ($inferred) {
            "cli" { "CLI-only" }
            "gui" { "GUI-only" }
            "all" { "CLI + GUI" }
        }
        Write-Host ""
        Write-Host "This appears to be an installation created by an earlier Loadbot setup version."
        if (Test-LoadbotInteractive) {
            $answer = Read-Host "Adopt this as a $inferredLabel installation and continue repair? [y/N]"
            if ($answer -notmatch '^(?i:y|yes)$') { throw "Setup cancelled; no changes were made" }
        } else {
            Write-Host "Adopting unambiguous legacy mode: $inferred."
        }
        return $inferred
    }
    if (-not $meaningful) {
        Write-Host ""
        Write-Host "No existing Loadbot installation was detected."
        return (Select-LoadbotRepairMode -NoninteractiveReason "No existing Loadbot installation was detected")
    }
    Write-Host ""
    Write-Host "Existing Loadbot files or integration were found, but the previous installation mode cannot be determined safely."
    Select-LoadbotRepairMode -NoninteractiveReason "The legacy Loadbot installation mode is ambiguous"
}

function Get-MissingLoadbotPrerequisites {
    param([switch]$IncludeGui)
    $missing = @()
    foreach ($name in @("git", "cargo", "rustc")) {
        if (-not (Get-LoadbotCommand $name)) { $missing += $name }
    }
    if ($IncludeGui) {
        if (-not (Test-LoadbotNodeSupported)) { $missing += "node" }
        if (-not (Get-LoadbotCommand "npm")) { $missing += "npm" }
        if (-not (Test-LoadbotWebView2)) { $missing += "webview2" }
        if (-not (Test-LoadbotWindowsBuildTools)) { $missing += "msvc-build-tools" }
    }
    $missing
}

function Get-LoadbotProfileState {
    param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return "missing" }
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            $hashBytes = $sha256.ComputeHash([IO.File]::ReadAllBytes($item.FullName))
            $hash = ([BitConverter]::ToString($hashBytes)).Replace("-", "")
        } finally {
            $sha256.Dispose()
        }
        return "file:$hash"
    }
    "unsafe"
}

function Assert-SafeLoadbotProfile {
    param([Parameter(Mandatory)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer) { throw "PowerShell profile is not a normal file: $Path" }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "Refusing to modify reparse-point PowerShell profile: $Path"
    }
}

function Get-LoadbotManagedBlock {
    param(
        [string]$InstallRoot = (Join-Path $HOME ".cargo"),
        [bool]$IncludeCompletion = $true
    )
    if (-not $IncludeCompletion) {
        return @'
# >>> loadbot >>>
# Loadbot's Cargo bin directory is managed in the user PATH by setup.
# <<< loadbot <<<
'@.TrimEnd("`r", "`n")
    }
    if ([string]::Equals(
        (Get-NormalizedLoadbotPath $InstallRoot),
        (Get-NormalizedLoadbotPath (Join-Path $HOME ".cargo")),
        [StringComparison]::OrdinalIgnoreCase
    )) {
        return @'
# >>> loadbot >>>
$LoadbotCompletion = Join-Path $HOME ".cargo\completions\loadbot.ps1"
if (Test-Path $LoadbotCompletion -PathType Leaf) {
    . $LoadbotCompletion
}
# <<< loadbot <<<
'@.TrimEnd("`r", "`n")
    }
    $quotedRoot = "'" + $InstallRoot.Replace("'", "''") + "'"
    @"
# >>> loadbot >>>
`$LoadbotCompletion = Join-Path $quotedRoot "completions\loadbot.ps1"
if (Test-Path `$LoadbotCompletion -PathType Leaf) {
    . `$LoadbotCompletion
}
# <<< loadbot <<<
"@.TrimEnd("`r", "`n")
}

function Get-LoadbotProfilePlan {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Block
    )
    Assert-SafeLoadbotProfile $Path
    if (-not (Test-Path -LiteralPath $Path)) { return "create" }
    $text = [IO.File]::ReadAllText($Path)
    $starts = [regex]::Matches($text, "(?m)^$([regex]::Escape($script:StartMarker))\r?$")
    $ends = [regex]::Matches($text, "(?m)^$([regex]::Escape($script:EndMarker))\r?$")
    if ($starts.Count -ne $ends.Count -or $starts.Count -gt 1) {
        throw "Malformed or duplicate Loadbot managed markers in $Path"
    }
    if ($starts.Count -eq 0) { return "append" }
    if ($starts[0].Index -ge $ends[0].Index) {
        throw "Malformed Loadbot managed markers in $Path"
    }
    $length = $ends[0].Index + $ends[0].Length - $starts[0].Index
    $existing = $text.Substring($starts[0].Index, $length) -replace "`r`n", "`n"
    if ($existing -eq $Block) { "unchanged" } else { "replace" }
}

function Get-LoadbotTextEncoding {
    param([Parameter(Mandatory)][byte[]]$Bytes)
    if ($Bytes.Length -ge 3 -and $Bytes[0] -eq 0xEF -and $Bytes[1] -eq 0xBB -and $Bytes[2] -eq 0xBF) {
        return [Text.UTF8Encoding]::new($true)
    }
    if ($Bytes.Length -ge 4 -and $Bytes[0] -eq 0xFF -and $Bytes[1] -eq 0xFE -and $Bytes[2] -eq 0 -and $Bytes[3] -eq 0) {
        return [Text.UTF32Encoding]::new($false, $true)
    }
    if ($Bytes.Length -ge 4 -and $Bytes[0] -eq 0 -and $Bytes[1] -eq 0 -and $Bytes[2] -eq 0xFE -and $Bytes[3] -eq 0xFF) {
        return [Text.UTF32Encoding]::new($true, $true)
    }
    if ($Bytes.Length -ge 2 -and $Bytes[0] -eq 0xFF -and $Bytes[1] -eq 0xFE) {
        return [Text.UnicodeEncoding]::new($false, $true)
    }
    if ($Bytes.Length -ge 2 -and $Bytes[0] -eq 0xFE -and $Bytes[1] -eq 0xFF) {
        return [Text.UnicodeEncoding]::new($true, $true)
    }
    try {
        $utf8 = [Text.UTF8Encoding]::new($false, $true)
        [void]$utf8.GetString($Bytes)
        return [Text.UTF8Encoding]::new($false)
    } catch {
        return [Text.Encoding]::GetEncoding([Globalization.CultureInfo]::CurrentCulture.TextInfo.ANSICodePage)
    }
}

function Update-LoadbotProfile {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Block,
        [Parameter(Mandatory)][ValidateSet("create", "append", "replace")][string]$Action
    )
    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }

    $encoding = [Text.UTF8Encoding]::new($false)
    $text = ""
    if (Test-Path -LiteralPath $Path) {
        $bytes = [IO.File]::ReadAllBytes($Path)
        $encoding = Get-LoadbotTextEncoding $bytes
        $text = $encoding.GetString($bytes)
        if ($encoding.GetPreamble().Length -gt 0 -and $text.Length -gt 0 -and $text[0] -eq [char]0xFEFF) {
            $text = $text.Substring(1)
        }
    }

    $newline = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }
    $formattedBlock = $Block -replace "`n", $newline
    if ($Action -eq "replace") {
        $pattern = "(?ms)^$([regex]::Escape($script:StartMarker))\r?$.*?^$([regex]::Escape($script:EndMarker))\r?$"
        $newText = [regex]::Replace($text, $pattern, [Text.RegularExpressions.MatchEvaluator]{ param($match) $formattedBlock })
    } else {
        $separator = if ($text.Length -eq 0) { "" } elseif ($text.EndsWith("`n")) { $newline } else { "$newline$newline" }
        $newText = "$text$separator$formattedBlock$newline"
    }

    $temporary = Join-Path $parent (".loadbot-profile.{0}.tmp" -f [Guid]::NewGuid().ToString("N"))
    [IO.File]::WriteAllText($temporary, $newText, $encoding)
    try {
        if (Test-Path -LiteralPath $Path) {
            $timestamp = Get-Date -Format "yyyyMMddHHmmssfff"
            $backup = "$Path.loadbot-backup.$timestamp"
            [IO.File]::Replace($temporary, $Path, $backup)
            Write-Host "Backed up profile to:"
            Write-Host "  $backup"
        } else {
            [IO.File]::Move($temporary, $Path)
        }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    }
}

function Get-NormalizedLoadbotPath {
    param([Parameter(Mandatory)][string]$Path)
    $trimmed = [Environment]::ExpandEnvironmentVariables($Path.Trim().Trim('"'))
    if (-not $trimmed) { return "" }
    try { $trimmed = [IO.Path]::GetFullPath($trimmed) } catch { }
    $trimmed.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

function Test-LoadbotExpectedPathTransition {
    param(
        [AllowNull()][string]$Before,
        [AllowNull()][string]$After,
        [Parameter(Mandatory)][string[]]$AllowedEntries
    )
    $beforeEntries = @($Before -split [IO.Path]::PathSeparator | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $afterEntries = @($After -split [IO.Path]::PathSeparator | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($afterEntries.Count -lt $beforeEntries.Count -or $afterEntries.Count -gt ($beforeEntries.Count + $AllowedEntries.Count)) { return $false }
    for ($index = 0; $index -lt $beforeEntries.Count; $index++) {
        if (-not [string]::Equals(
            (Get-NormalizedLoadbotPath $beforeEntries[$index]),
            (Get-NormalizedLoadbotPath $afterEntries[$index]),
            [StringComparison]::OrdinalIgnoreCase
        )) { return $false }
    }
    for ($index = $beforeEntries.Count; $index -lt $afterEntries.Count; $index++) {
        $allowed = $false
        foreach ($entry in $AllowedEntries) {
            if ([string]::Equals(
                (Get-NormalizedLoadbotPath $afterEntries[$index]),
                (Get-NormalizedLoadbotPath $entry),
                [StringComparison]::OrdinalIgnoreCase
            )) { $allowed = $true; break }
        }
        if (-not $allowed) { return $false }
    }
    $true
}

function Test-LoadbotPathContains {
    param(
        [AllowNull()][string]$PathValue,
        [Parameter(Mandatory)][string]$Entry
    )
    $wanted = Get-NormalizedLoadbotPath $Entry
    foreach ($candidate in ($PathValue -split [IO.Path]::PathSeparator)) {
        if ([string]::IsNullOrWhiteSpace($candidate)) { continue }
        if ([string]::Equals((Get-NormalizedLoadbotPath $candidate), $wanted, [StringComparison]::OrdinalIgnoreCase)) {
            return $true
        }
    }
    $false
}

function Add-LoadbotUserPath {
    param([Parameter(Mandatory)][string]$InstallBin)
    $userPath = Get-LoadbotUserPath
    if (-not (Test-LoadbotPathContains $userPath $InstallBin)) {
        $newUserPath = if ([string]::IsNullOrEmpty($userPath)) { $InstallBin } else { "$userPath$([IO.Path]::PathSeparator)$InstallBin" }
        Set-LoadbotUserPath $newUserPath
    }
    if (-not (Test-LoadbotPathContains $env:PATH $InstallBin)) {
        $env:PATH = if ([string]::IsNullOrEmpty($env:PATH)) { $InstallBin } else { "$env:PATH$([IO.Path]::PathSeparator)$InstallBin" }
    }
}

function Invoke-LoadbotSetup {
    param([ValidateSet("cli", "gui", "all", "repair")][string]$Mode = "cli")
    $projectDir = $PSScriptRoot
    if (-not (Test-Path (Join-Path $projectDir "Cargo.toml") -PathType Leaf)) {
        throw "Cargo.toml was not found in $projectDir"
    }

    $installRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME ".cargo" }
    $installBin = Join-Path $installRoot "bin"
    $loadbotExe = Join-Path $installBin "loadbot.exe"
    $completionDir = Join-Path $installRoot "completions"
    $completionPath = Join-Path $completionDir "loadbot.ps1"
    $modePath = Join-Path $installRoot "loadbot-install-mode"
    $guiExe = Join-Path $installBin "loadbot-desktop.exe"
    $profilePath = Get-LoadbotProfilePath
    if ($Mode -eq "repair") {
        if (Test-Path -LiteralPath $modePath) {
            $modeItem = Get-Item -LiteralPath $modePath -Force
            if ($modeItem.PSIsContainer -or ($modeItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Loadbot installation record is not a normal file: $modePath"
            }
            $Mode = ([IO.File]::ReadAllText($modePath)).Trim()
            if ($Mode -notin @("cli", "gui", "all")) { throw "Invalid installation record in $modePath" }
            Write-Host "Repairing recorded $Mode installation."
        } else {
            $Mode = Resolve-LoadbotLegacyRepairMode -InstallBin $installBin -LoadbotExe $loadbotExe `
                -GuiExe $guiExe -CompletionPath $completionPath -ProfilePath $profilePath
            Write-Host "Repairing $Mode installation."
        }
        Assert-LoadbotConfigurationDirectories
    }
    $wantGui = $Mode -in @("gui", "all")
    $wantCompletion = $Mode -in @("cli", "all")
    $block = if ($wantCompletion) { Get-LoadbotManagedBlock -InstallRoot $installRoot } else { "" }
    $profilePlan = if ($wantCompletion) { Get-LoadbotProfilePlan -Path $profilePath -Block $block } else { "not configured" }
    $profileState = if ($wantCompletion) { Get-LoadbotProfileState $profilePath } else { "not inspected" }
    $userPathBefore = Get-LoadbotUserPath
    $pathPlan = if (Test-LoadbotPathContains $userPathBefore $installBin) { "unchanged" } else { "add" }
    $missing = @(Get-MissingLoadbotPrerequisites -IncludeGui:$wantGui)
    $wingetAvailable = [bool](Get-LoadbotCommand "winget")

    $packages = @()
    if ($missing -contains "git") { $packages += "Git.Git" }
    if ($missing -contains "cargo" -or $missing -contains "rustc") { $packages += "Rustlang.Rustup" }
    if ($missing -contains "node" -or $missing -contains "npm") { $packages += "OpenJS.NodeJS.LTS" }
    if ($missing -contains "webview2") { $packages += "Microsoft.EdgeWebView2Runtime" }

    Write-Host "LOADBOT SETUP PLAN"
    Write-Host "Mode: $Mode"
    Write-Host ""
    Write-Host "Prerequisites:"
    foreach ($name in @("git", "cargo", "rustc")) {
        $status = if ($missing -contains $name) { "missing" } else { "ready" }
        Write-Host ("  {0,-6} {1}" -f "$name`:", $status)
    }
    if ($wantGui) {
        Write-Host ("  {0,-6} {1}" -f "node:", $(if (Test-LoadbotNodeSupported) { "ready" } else { "missing or older than 22" }))
        Write-Host ("  {0,-6} {1}" -f "npm:", $(if (Get-LoadbotCommand "npm") { "ready" } else { "missing" }))
        Write-Host ("  WebView2: " + $(if (Test-LoadbotWebView2) { "ready" } else { "missing" }))
        Write-Host ("  MSVC build tools: " + $(if (Test-LoadbotWindowsBuildTools) { "ready" } else { "missing" }))
    }
    Write-Host ""
    Write-Host "Package manager:"
    Write-Host ("  " + $(if ($missing.Count -eq 0) { "none required" } elseif ($wingetAvailable) { "winget" } else { "unavailable" }))
    if ($packages.Count -gt 0 -and $wingetAvailable) {
        Write-Host ""
        Write-Host "Would run:"
        foreach ($package in $packages) {
            Write-Host "  winget install --id $package --exact --source winget --scope user --accept-package-agreements --accept-source-agreements"
        }
        Write-Host "Elevation required: no (user-scoped Winget installation)"
    }
    Write-Host ""
    Write-Host "Would install:"
    Write-Host "  $loadbotExe"
    if ($wantGui) { Write-Host "  $guiExe" }
    Write-Host ""
    Write-Host "Would configure:"
    Write-Host "  User PATH: $installBin ($pathPlan)"
    if ($wantCompletion) {
        Write-Host "  $profilePath ($profilePlan)"
        Write-Host "  $completionPath"
    }

    if ($missing -contains "msvc-build-tools") {
        throw "GUI setup requires Microsoft C++ Build Tools with the 'Desktop development with C++' workload. Install it from Visual Studio Installer, then rerun setup."
    }

    if ($missing.Count -gt 0 -and -not $wingetAvailable) {
        Write-Host ""
        Write-Host "Missing prerequisites: $($missing -join ', ')"
        Write-Host "Winget is unavailable. Install Git for Windows (Git.Git) and Rustup (Rustlang.Rustup) as needed, then rerun setup."
        throw "Cannot install missing prerequisites without Winget"
    }

    $needsApproval = $missing.Count -gt 0 -or $pathPlan -eq "add" -or ($wantCompletion -and $profilePlan -ne "unchanged")
    if ($needsApproval) {
        if (-not (Test-LoadbotInteractive)) {
            if ($missing.Count -gt 0) {
                Write-Host "Noninteractive setup cannot install prerequisites. Run the exact Winget command(s) shown above manually, then rerun setup."
            } else {
                Write-Host "Noninteractive setup cannot approve PATH or profile changes. Rerun in an interactive PowerShell terminal."
            }
            throw "Setup approval requires an interactive terminal"
        }
        $prompt = if ($missing.Count -gt 0) { "Install these prerequisites? [y/N]" } else { "Proceed? [y/N]" }
        $answer = Read-Host $prompt
        if ($answer -notmatch '^(?i:y|yes)$') {
            throw "Setup cancelled; no changes were made"
        }
    }

    $currentPrerequisites = @(Get-MissingLoadbotPrerequisites -IncludeGui:$wantGui)
    if (($currentPrerequisites -join "`0") -ne ($missing -join "`0")) {
        throw "Prerequisite state changed after approval; rerun setup"
    }
    if (($wantCompletion -and (Get-LoadbotProfileState $profilePath) -ne $profileState) -or (Get-LoadbotUserPath) -ne $userPathBefore) {
        throw "Profile or user PATH changed after approval; rerun setup"
    }

    if ($packages.Count -gt 0) {
        foreach ($package in $packages) {
            Invoke-LoadbotWinget @("install", "--id", $package, "--exact", "--source", "winget", "--scope", "user", "--accept-package-agreements", "--accept-source-agreements")
        }
        Sync-LoadbotProcessPath
        if ($packages -contains "Rustlang.Rustup") {
            $rustupCommand = Get-LoadbotCommand "rustup"
            $rustupExe = if ($rustupCommand) { $rustupCommand.Source } else { Join-Path $installBin "rustup.exe" }
            if (-not (Test-Path -LiteralPath $rustupExe -PathType Leaf) -and -not $rustupCommand) {
                throw "Rustup was installed, but rustup.exe was not found at $rustupExe"
            }
            Invoke-LoadbotRustup -RustupExe $rustupExe -Arguments @("toolchain", "install", "stable")
            Invoke-LoadbotRustup -RustupExe $rustupExe -Arguments @("default", "stable")
            if (-not (Test-LoadbotPathContains $env:PATH $installBin)) {
                $env:PATH = "$env:PATH$([IO.Path]::PathSeparator)$installBin"
            }
        }
        $remaining = @(Get-MissingLoadbotPrerequisites -IncludeGui:$wantGui)
        if ($remaining -contains "node") { throw "Node.js 22.12 or newer is still unavailable; install a current Node.js LTS release and rerun setup" }
        if ($remaining.Count -gt 0) { throw "Prerequisites remain missing after installation: $($remaining -join ', ')" }
        # Rustup may perform the exact approved Cargo-bin PATH addition itself.
        $userPathAfterPackages = Get-LoadbotUserPath
        $allowedPathAdditions = @($installBin)
        if ($packages -contains "Git.Git") {
            $gitCommand = Get-LoadbotCommand "git"
            if ($gitCommand -and $gitCommand.Source) { $allowedPathAdditions += Split-Path -Parent $gitCommand.Source }
        }
        if (-not (Test-LoadbotExpectedPathTransition -Before $userPathBefore -After $userPathAfterPackages -AllowedEntries $allowedPathAdditions)) {
            throw "User PATH changed unexpectedly during prerequisite installation; rerun setup"
        }
        $userPathBefore = $userPathAfterPackages
    }

    $cargoCommand = Get-LoadbotCommand "cargo"
    if (-not $cargoCommand) {
        $cargoExe = Join-Path $installBin "cargo.exe"
        if (-not (Test-Path -LiteralPath $cargoExe -PathType Leaf)) { throw "cargo was not found after prerequisite installation" }
    } else {
        $cargoExe = $cargoCommand.Source
    }

    Write-Host "Installing Loadbot from source..."
    Invoke-LoadbotCargoInstall -CargoExe $cargoExe -ProjectDir $projectDir -InstallRoot $installRoot
    if (-not (Test-Path -LiteralPath $loadbotExe -PathType Leaf)) {
        throw "Cargo completed, but $loadbotExe was not created"
    }
    Invoke-LoadbotExecutable -Executable $loadbotExe -Arguments @("--version")
    Invoke-LoadbotExecutable -Executable $loadbotExe -Arguments @("--help") | Out-Null

    if ($wantGui) {
        $npm = (Get-LoadbotCommand "npm").Source
        $guiRoot = Join-Path $projectDir "src\gui"
        if (Test-LoadbotFrontendDependencies -GuiRoot $guiRoot) {
            Write-Host "Lockfile-pinned GUI dependencies are current."
        } else {
            Write-Host "Restoring lockfile-pinned GUI dependencies..."
            Invoke-LoadbotExecutable -Executable $npm -Arguments @("--prefix", $guiRoot, "ci")
            Copy-Item -LiteralPath (Join-Path $guiRoot "package-lock.json") -Destination (Join-Path $guiRoot "node_modules\.loadbot-package-lock.json") -Force
        }
        Write-Host "Building the native Loadbot GUI..."
        Invoke-LoadbotExecutable -Executable $npm -Arguments @("--prefix", $guiRoot, "run", "desktop:build")
        $builtGui = Join-Path $guiRoot "src-tauri\target\release\loadbot-desktop.exe"
        if (-not (Test-Path -LiteralPath $builtGui -PathType Leaf)) { throw "Tauri completed, but $builtGui was not created" }
        if (Test-Path -LiteralPath $guiExe) {
            $installedGui = Get-Item -LiteralPath $guiExe -Force
            if ($installedGui.PSIsContainer -or ($installedGui.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Installed GUI path is not a normal file: $guiExe"
            }
        }
        Copy-Item -LiteralPath $builtGui -Destination $guiExe -Force
    }

    if ($wantCompletion) {
        Write-Host "Generating PowerShell completion..."
        New-Item -ItemType Directory -Force -Path $completionDir | Out-Null
        $previousComplete = $env:COMPLETE
        try {
            $env:COMPLETE = "powershell"
            $completion = Invoke-LoadbotExecutable -Executable $loadbotExe -Arguments @() -Capture
            $completion | Set-Content -LiteralPath $completionPath -Encoding utf8
        } finally {
            $env:COMPLETE = $previousComplete
        }
    }

    if (($wantCompletion -and (Get-LoadbotProfileState $profilePath) -ne $profileState) -or (Get-LoadbotUserPath) -ne $userPathBefore) {
        throw "Profile or user PATH changed while Loadbot was being installed; rerun setup"
    }
    Add-LoadbotUserPath -InstallBin $installBin
    if ($wantCompletion -and $profilePlan -ne "unchanged") {
        if ($profilePlan -eq "replace") { Write-Host "Updating the existing Loadbot managed block in $profilePath" }
        Update-LoadbotProfile -Path $profilePath -Block $block -Action $profilePlan
    }
    Set-LoadbotInstallMode -Path $modePath -Mode $Mode

    Write-Host ""
    Write-Host "Loadbot installed and verified successfully:"
    Write-Host "  $loadbotExe"
    Write-Host $(if ($wantCompletion) { "User PATH and PowerShell completion are configured. Open a new PowerShell, or reload the profile:" } else { "User PATH is configured. Open a new PowerShell." })
    if ($wantCompletion) { Write-Host "  . `"$profilePath`"" }
    Write-Host "The already-running parent process was not modified."
    $policy = Get-ExecutionPolicy
    if ($policy -in @("Restricted", "AllSigned")) {
        Write-Warning "The effective execution policy ($policy) may prevent the profile or generated completion script from loading. Setup did not change execution policy."
    }
}

function Select-LoadbotSetupMode {
    if (-not (Test-LoadbotInteractive)) {
        throw "Setup mode is required without an interactive terminal; use -Cli, -Gui, -All, or -Repair"
    }
    Write-Host "LOADBOT SETUP"
    Write-Host ""
    Write-Host "What would you like to configure?"
    Write-Host "  1. CLI only"
    Write-Host "  2. GUI only"
    Write-Host "  3. CLI + GUI"
    Write-Host "  4. Repair / verify installation"
    Write-Host "  5. Exit"
    $selection = Read-Host ">"
    switch ($selection) {
        "1" { "cli" }
        "2" { "gui" }
        "3" { "all" }
        "4" { "repair" }
        "5" { $null }
        default { throw "Invalid setup selection '$selection'" }
    }
}

if ($env:LOADBOT_SETUP_TESTING -ne "1") {
    $selected = @(@($Cli, $Gui, $All, $Repair) | Where-Object { $_ }).Count
    if ($selected -gt 1) { throw "Choose only one of -Cli, -Gui, -All, or -Repair" }
    $mode = if ($Cli) { "cli" } elseif ($Gui) { "gui" } elseif ($All) { "all" } elseif ($Repair) { "repair" } else { Select-LoadbotSetupMode }
    if ($mode) { Invoke-LoadbotSetup -Mode $mode } else { Write-Host "Setup cancelled; no changes were made." }
}
