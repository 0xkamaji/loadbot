$scriptPath = Join-Path $PSScriptRoot "..\setup.ps1"
$env:LOADBOT_SETUP_TESTING = "1"
. $scriptPath

Describe "Loadbot PowerShell setup" {
    BeforeEach {
        $script:testRoot = Join-Path $TestDrive "home with spaces"
        $script:project = Split-Path -Parent $scriptPath
        $script:profile = Join-Path $testRoot "Documents\PowerShell\Microsoft.PowerShell_profile.ps1"
        $script:installRoot = Join-Path $testRoot ".cargo"
        $script:installBin = Join-Path $installRoot "bin"
        $script:loadbot = Join-Path $installBin "loadbot.exe"
        $env:CARGO_HOME = $installRoot
        $env:PATH = "C:\Windows\System32"
        New-Item -ItemType Directory -Force $installBin | Out-Null
        Set-Content -LiteralPath $loadbot -Value "fake"
        Mock Get-LoadbotProfilePath { $script:profile }
        Mock Get-LoadbotUserPath { "C:\Existing" }
        Mock Get-LoadbotMachinePath { "C:\Windows\System32" }
        Mock Set-LoadbotUserPath { }
        Mock Test-LoadbotInteractive { $true }
        Mock Read-Host { "y" }
        Mock Get-ExecutionPolicy { "RemoteSigned" }
        Mock Invoke-LoadbotCargoInstall { }
        Mock Invoke-LoadbotExecutable {
            if ($Capture) { "Register-ArgumentCompleter -Native -CommandName loadbot -ScriptBlock {}" }
        }
        Mock Get-LoadbotCommand {
            param($Name)
            if ($Name -in @("git", "cargo", "rustc", "winget")) {
                [pscustomobject]@{ Source = "C:\fake\$Name.exe" }
            }
        }
    }

    It "recognizes ready prerequisites and verifies the absolute executable" {
        Invoke-LoadbotSetup
        Assert-MockCalled Invoke-LoadbotCargoInstall -Times 1 -ParameterFilter { $InstallRoot -eq $script:installRoot }
        Assert-MockCalled Invoke-LoadbotExecutable -Times 1 -ParameterFilter { $Executable -eq $script:loadbot -and $Arguments[0] -eq "--version" }
        Assert-MockCalled Invoke-LoadbotExecutable -Times 1 -ParameterFilter { $Executable -eq $script:loadbot -and $Arguments[0] -eq "--help" }
    }

    It "proposes only Git.Git when Git is missing and requires approval" {
        Mock Get-LoadbotCommand {
            param($Name)
            if ($Name -in @("cargo", "rustc", "winget")) { [pscustomobject]@{ Source = "C:\fake\$Name.exe" } }
            elseif ($Name -eq "git" -and $script:gitInstalled) { [pscustomobject]@{ Source = "C:\fake\git.exe" } }
        }
        Mock Invoke-LoadbotWinget { $script:gitInstalled = $true }
        Invoke-LoadbotSetup
        Assert-MockCalled Invoke-LoadbotWinget -Times 1 -ParameterFilter {
            $Arguments -contains "Git.Git" -and $Arguments -contains "--exact" -and
                $Arguments -contains "--source" -and $Arguments -contains "winget" -and
                $Arguments -contains "--scope" -and $Arguments -contains "user"
        }
        Assert-MockCalled Read-Host -Times 1 -ParameterFilter { $prompt -eq "Install these prerequisites? [y/N]" }
    }

    It "installs Rustlang.Rustup once and initializes stable for missing Cargo and rustc" {
        $rustup = Join-Path $testRoot ".cargo\bin\rustup.exe"
        Set-Content $rustup fake
        Mock Get-LoadbotCommand {
            param($Name)
            if ($Name -in @("git", "winget")) { [pscustomobject]@{ Source = "C:\fake\$Name.exe" } }
            if ($Name -in @("cargo", "rustc") -and $script:rustInstalled) { [pscustomobject]@{ Source = "C:\fake\$Name.exe" } }
        }
        Mock Invoke-LoadbotWinget { $script:rustInstalled = $true }
        Mock Invoke-LoadbotRustup { }
        Invoke-LoadbotSetup
        Assert-MockCalled Invoke-LoadbotWinget -Times 1 -ParameterFilter { $Arguments -contains "Rustlang.Rustup" }
        Assert-MockCalled Invoke-LoadbotRustup -Times 1 -ParameterFilter { $Arguments -contains "install" -and $Arguments -contains "stable" }
        Assert-MockCalled Invoke-LoadbotRustup -Times 1 -ParameterFilter { $Arguments -contains "default" -and $Arguments -contains "stable" }
    }

    It "declines without invoking Winget, Cargo, PATH, or profile changes" {
        Mock Get-LoadbotCommand { param($Name) if ($Name -eq "winget") { [pscustomobject]@{ Source = "winget.exe" } } }
        Mock Read-Host { "n" }
        { Invoke-LoadbotSetup } | Should -Throw "*cancelled*"
        Assert-MockCalled Invoke-LoadbotWinget -Times 0
        Assert-MockCalled Invoke-LoadbotCargoInstall -Times 0
        Assert-MockCalled Set-LoadbotUserPath -Times 0
        Test-Path $profile | Should -BeFalse
    }

    It "fails safely when Winget is unavailable" {
        Mock Get-LoadbotCommand { $null }
        { Invoke-LoadbotSetup } | Should -Throw "*without Winget*"
        Assert-MockCalled Invoke-LoadbotWinget -Times 0
    }

    It "stops when Winget fails" {
        Mock Get-LoadbotCommand { param($Name) if ($Name -eq "winget") { [pscustomobject]@{ Source = "winget.exe" } } }
        Mock Invoke-LoadbotWinget { throw "winget failed" }
        { Invoke-LoadbotSetup } | Should -Throw "winget failed"
        Assert-MockCalled Invoke-LoadbotCargoInstall -Times 0
    }

    It "adds user PATH once case-insensitively and never requests Machine scope" {
        Mock Get-LoadbotUserPath { "C:\Existing;$($script:installBin.ToUpperInvariant())\" }
        Add-LoadbotUserPath $script:installBin
        Assert-MockCalled Set-LoadbotUserPath -Times 0
        Test-LoadbotPathContains $env:PATH $script:installBin | Should -BeTrue
        (Get-Content -Raw $scriptPath) | Should -Not -Match 'SetEnvironmentVariable\([^\r\n]+"Machine"'
    }

    It "preserves unrelated user PATH entries when adding Cargo bin" {
        $script:setPath = $null
        Mock Get-LoadbotUserPath { "C:\One;C:\Two" }
        Mock Set-LoadbotUserPath { param($Value) $script:setPath = $Value }
        Add-LoadbotUserPath $script:installBin
        Assert-MockCalled Set-LoadbotUserPath -Times 1
        $script:setPath | Should -Match ([regex]::Escape("C:\One;C:\Two"))
        Test-LoadbotPathContains $script:setPath $script:installBin | Should -BeTrue
    }

    It "preserves process-only PATH entries while refreshing persistent PATH" {
        $env:PATH = "C:\SessionOnly"
        Sync-LoadbotProcessPath
        Test-LoadbotPathContains $env:PATH "C:\SessionOnly" | Should -BeTrue
        Test-LoadbotPathContains $env:PATH "C:\Existing" | Should -BeTrue
        Test-LoadbotPathContains $env:PATH "C:\Windows\System32" | Should -BeTrue
    }

    It "accepts only the planned Cargo-bin user PATH transition" {
        Test-LoadbotExpectedPathTransition "C:\One;C:\Two" "C:\One;C:\Two;$script:installBin" @($script:installBin) | Should -BeTrue
        Test-LoadbotExpectedPathTransition "C:\One" "C:\Changed;$script:installBin" @($script:installBin) | Should -BeFalse
    }

    It "preserves an existing profile, backs it up, and is idempotent" {
        New-Item -ItemType Directory -Force (Split-Path $profile) | Out-Null
        Set-Content -LiteralPath $profile -Value "# existing"
        $block = Get-LoadbotManagedBlock
        (Get-LoadbotProfilePlan $profile $block) | Should -Be "append"
        Update-LoadbotProfile $profile $block append
        Get-Content -Raw $profile | Should -Match "# existing"
        @(Get-ChildItem "$profile.loadbot-backup.*").Count | Should -Be 1
        (Get-LoadbotProfilePlan $profile $block) | Should -Be "unchanged"
    }

    It "replaces only an existing managed block" {
        New-Item -ItemType Directory -Force (Split-Path $profile) | Out-Null
        Set-Content $profile "before`n# >>> loadbot >>>`nold`n# <<< loadbot <<<`nafter"
        $block = Get-LoadbotManagedBlock
        (Get-LoadbotProfilePlan $profile $block) | Should -Be "replace"
        Update-LoadbotProfile $profile $block replace
        $updated = Get-Content -Raw $profile
        $updated | Should -Match "before"
        $updated | Should -Match "after"
        ([regex]::Matches($updated, [regex]::Escape("# >>> loadbot >>>"))).Count | Should -Be 1
    }

    It "preserves UTF-16 profile encoding" {
        New-Item -ItemType Directory -Force (Split-Path $profile) | Out-Null
        [IO.File]::WriteAllText($profile, "# existing", [Text.UnicodeEncoding]::new($false, $true))
        Update-LoadbotProfile $profile (Get-LoadbotManagedBlock) append
        $bytes = [IO.File]::ReadAllBytes($profile)
        $bytes[0] | Should -Be 0xFF
        $bytes[1] | Should -Be 0xFE
    }

    It "uses a custom Cargo home in the completion block" {
        $customRoot = Join-Path $testRoot "custom cargo"
        Get-LoadbotManagedBlock $customRoot | Should -Match ([regex]::Escape($customRoot))
    }

    It "refuses malformed and duplicate managed markers" {
        New-Item -ItemType Directory -Force (Split-Path $profile) | Out-Null
        Set-Content $profile "# >>> loadbot >>>"
        { Get-LoadbotProfilePlan $profile (Get-LoadbotManagedBlock) } | Should -Throw "*Malformed*"
        Set-Content $profile "# >>> loadbot >>>`n# <<< loadbot <<<`n# >>> loadbot >>>`n# <<< loadbot <<<"
        { Get-LoadbotProfilePlan $profile (Get-LoadbotManagedBlock) } | Should -Throw "*duplicate*"
    }

    It "refuses a reparse-point profile" -Skip:(-not $IsWindows) {
        New-Item -ItemType Directory -Force (Split-Path $profile) | Out-Null
        $target = Join-Path $testRoot "target.ps1"
        Set-Content $target untouched
        New-Item -ItemType SymbolicLink -Path $profile -Target $target | Out-Null
        { Get-LoadbotProfilePlan $profile (Get-LoadbotManagedBlock) } | Should -Throw "*reparse-point*"
    }

    It "writes completion configuration and never changes execution policy" {
        Mock Set-ExecutionPolicy { }
        Invoke-LoadbotSetup
        Get-Content -Raw (Join-Path $installRoot "completions\loadbot.ps1") | Should -Match "Register-ArgumentCompleter"
        Get-Content -Raw $profile | Should -Match ([regex]::Escape('$LoadbotCompletion = Join-Path $HOME ".cargo\completions\loadbot.ps1"'))
        Assert-MockCalled Get-ExecutionPolicy -Times 1
        Assert-MockCalled Set-ExecutionPolicy -Times 0
    }

    It "refuses noninteractive prerequisite installation before any mutation" {
        Mock Get-LoadbotCommand { param($Name) if ($Name -eq "winget") { [pscustomobject]@{ Source = "winget.exe" } } }
        Mock Test-LoadbotInteractive { $false }
        { Invoke-LoadbotSetup } | Should -Throw "*interactive terminal*"
        Assert-MockCalled Invoke-LoadbotWinget -Times 0
        Assert-MockCalled Invoke-LoadbotCargoInstall -Times 0
        Assert-MockCalled Set-LoadbotUserPath -Times 0
    }

    It "stops profile and PATH configuration when Cargo fails" {
        Mock Invoke-LoadbotCargoInstall { throw "Cargo failed" }
        { Invoke-LoadbotSetup } | Should -Throw "Cargo failed"
        Assert-MockCalled Set-LoadbotUserPath -Times 0
        Test-Path $profile | Should -BeFalse
    }
}
