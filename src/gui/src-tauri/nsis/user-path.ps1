param(
    [Parameter(Mandatory)]
    [ValidateSet("Add", "Remove")]
    [string]$Mode,

    [Parameter(Mandatory)]
    [string]$InstallDir
)

$ErrorActionPreference = "Stop"
$installerKeyPath = "Software\Loadbot\Installer"

function Get-NormalizedPath {
    param([AllowEmptyString()][string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) { return "" }
    $expanded = [Environment]::ExpandEnvironmentVariables($Path.Trim().Trim('"'))
    try { $expanded = [IO.Path]::GetFullPath($expanded) } catch { }
    $expanded.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
}

$normalizedInstallDir = Get-NormalizedPath $InstallDir
if (-not $normalizedInstallDir) { throw "The Loadbot installation directory is invalid" }

$environment = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
if (-not $environment) { throw "Could not open the current-user environment registry key" }

try {
    $pathValue = $environment.GetValue(
        "Path",
        $null,
        [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames
    )
    $pathKind = if ($null -eq $pathValue) {
        [Microsoft.Win32.RegistryValueKind]::ExpandString
    } else {
        $environment.GetValueKind("Path")
    }
    $pathText = if ($null -eq $pathValue) { "" } else { [string]$pathValue }
    $entries = @($pathText.Split([char]';'))

    if ($Mode -eq "Add") {
        $present = $entries | Where-Object {
            [string]::Equals((Get-NormalizedPath $_), $normalizedInstallDir, [StringComparison]::OrdinalIgnoreCase)
        }
        if (-not $present) {
            $newPath = if ([string]::IsNullOrEmpty($pathText)) { $InstallDir } else { "$pathText;$InstallDir" }
            $environment.SetValue("Path", $newPath, $pathKind)
            $marker = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($installerKeyPath)
            try { $marker.SetValue("PathEntry", $InstallDir, [Microsoft.Win32.RegistryValueKind]::String) }
            finally { $marker.Dispose() }
        }
    } else {
        $marker = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($installerKeyPath, $true)
        if ($marker) {
            try { $ownedEntry = [string]$marker.GetValue("PathEntry", "") }
            finally { $marker.Dispose() }
            if ([string]::Equals((Get-NormalizedPath $ownedEntry), $normalizedInstallDir, [StringComparison]::OrdinalIgnoreCase)) {
                $remaining = @($entries | Where-Object {
                    -not [string]::Equals((Get-NormalizedPath $_), $normalizedInstallDir, [StringComparison]::OrdinalIgnoreCase)
                })
                $environment.SetValue("Path", ($remaining -join ";"), $pathKind)
                [Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($installerKeyPath, $false)
            }
        }
    }
} finally {
    $environment.Dispose()
}

try {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class LoadbotEnvironmentBroadcast {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr SendMessageTimeout(
        IntPtr hWnd, uint message, UIntPtr wParam, string lParam,
        uint flags, uint timeout, out UIntPtr result);
}
'@
    $result = [UIntPtr]::Zero
    [void][LoadbotEnvironmentBroadcast]::SendMessageTimeout(
        [IntPtr]0xffff,
        0x001A,
        [UIntPtr]::Zero,
        "Environment",
        0x0002,
        5000,
        [ref]$result
    )
} catch {
    # PATH is already persisted; a new process will still observe it.
}
