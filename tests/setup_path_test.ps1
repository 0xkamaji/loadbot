# Standalone regression checks; no Pester or machine/user PATH writes required.
$ErrorActionPreference = "Stop"
$previousTesting = $env:LOADBOT_SETUP_TESTING
try {
    $env:LOADBOT_SETUP_TESTING = "1"
    . (Join-Path $PSScriptRoot "..\setup.ps1")
} finally {
    $env:LOADBOT_SETUP_TESTING = $previousTesting
}

$separator = [string][IO.Path]::PathSeparator
$target = Join-Path ([IO.Path]::GetTempPath()) "loadbot path checks\bin"
$other = Join-Path ([IO.Path]::GetTempPath()) "loadbot unrelated\bin"
$cases = @(
    @{ Name = "unset PATH"; Value = $null; Expected = $false }
    @{ Name = "empty PATH"; Value = ""; Expected = $false }
    @{ Name = "whitespace PATH"; Value = "   "; Expected = $false }
    @{ Name = "only separators"; Value = "$separator$separator"; Expected = $false }
    @{ Name = "blank entries around unrelated path"; Value = "$separator$other$separator $separator"; Expected = $false }
    @{ Name = "leading empty entry"; Value = "$separator$target"; Expected = $true }
    @{ Name = "trailing empty entry without match"; Value = "$other$separator"; Expected = $false }
    @{ Name = "consecutive empty entries before match"; Value = "$other$separator$separator$target"; Expected = $true }
    @{ Name = "whitespace entry before match"; Value = " $separator$target"; Expected = $true }
    @{ Name = "case-insensitive match"; Value = $target.ToUpperInvariant(); Expected = $true }
    @{ Name = "quoted path with spaces"; Value = '"' + $target + '"'; Expected = $true }
    @{ Name = "trailing slash match"; Value = $target + [IO.Path]::DirectorySeparatorChar; Expected = $true }
)
foreach ($case in $cases) {
    $actual = Test-LoadbotPathContains -PathValue $case.Value -Entry $target
    if ($actual -isnot [bool] -or $actual -ne $case.Expected) {
        throw "$($case.Name): expected $($case.Expected), got '$actual'"
    }
}
Write-Host "Passed $($cases.Count) PowerShell PATH regression checks."
