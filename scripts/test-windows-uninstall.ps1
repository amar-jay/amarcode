# Compile the real hook into an isolated NSIS uninstaller and exercise it.
# All instance data and executables below are fixtures; no real tasks are used.
param([string]$MakeNsis)
$ErrorActionPreference = 'Stop'
if (-not $MakeNsis) {
    $compiler = Get-Command makensis.exe -ErrorAction SilentlyContinue
    if ($compiler) { $MakeNsis = $compiler.Source }
    else { $MakeNsis = Join-Path ${env:ProgramFiles(x86)} 'NSIS/makensis.exe' }
}
if (-not (Test-Path $MakeNsis)) { throw 'Install NSIS before running this test' }
$root = Join-Path ([IO.Path]::GetTempPath()) ('amarcode-uninstall-' + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $installer = Join-Path $root 'fixture-installer.exe'
    $hook = (Resolve-Path (Join-Path $PSScriptRoot '../crates/application/src-tauri/windows/installer-hooks.nsh')).Path
    $template = Join-Path $PSScriptRoot 'fixtures/uninstall-preserves-daemon.nsi'
    & $MakeNsis /V2 "/DFIXTURE_OUT=$installer" "/DHOOK_FILE=$hook" $template
    if ($LASTEXITCODE -ne 0) { throw 'NSIS fixture compilation failed' }
    foreach ($case in @(
        @{ Name = 'normal'; Delete = 0; Update = $false },
        @{ Name = 'delete checkbox selected'; Delete = 1; Update = $false },
        @{ Name = 'application update'; Delete = 0; Update = $true },
        @{ Name = 'application update with delete selected'; Delete = 1; Update = $true }
    )) {
        $caseRoot = Join-Path $root $case.Name
        $desktop = Join-Path $caseRoot 'desktop'
        $shared = Join-Path $caseRoot 'local-app-data/com.amarcode.desktop'
        $roaming = Join-Path $caseRoot 'roaming-app-data/com.amarcode.desktop'
        New-Item -ItemType Directory -Path $desktop, "$shared/data", "$shared/daemon/0.7.3", $roaming -Force | Out-Null
        $owned = @{
            "$shared/data/workspace.sqlite3" = 'conversation database'
            "$shared/data/workspace.sqlite3-wal" = 'pending WAL data'
            "$shared/daemon/0.7.3/amarcode-daemon.exe" = 'independent managed binary'
            "$shared/daemon/current-release.json" = 'verified current release'
            "$shared/access-token" = 'private token'
            "$shared/remote.json" = 'enabled remote access'
            "$shared/connection.json" = 'saved desktop connection'
            "$roaming/preferences" = 'desktop preferences'
        }
        foreach ($path in $owned.Keys) { [IO.File]::WriteAllText($path, $owned[$path]) }
        [IO.File]::WriteAllText((Join-Path $desktop 'amarcode.exe'), 'desktop application')
        [IO.File]::WriteAllText((Join-Path $desktop 'amarcode-bootstrap.exe'), 'bundled bootstrap')
        $process = Start-Process $installer -ArgumentList "/S /D=$desktop" -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Fixture installation failed: $($case.Name)" }
        $update = if ($case.Update) { '/UPDATE' } else { '' }
        $process = Start-Process (Join-Path $desktop 'uninstall.exe') -ArgumentList "/S /DELETE_APP_DATA=$($case.Delete) $update _?=$desktop" -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "Fixture uninstall failed: $($case.Name)" }
        foreach ($path in $owned.Keys) {
            if (-not (Test-Path $path) -or [IO.File]::ReadAllText($path) -cne $owned[$path]) {
                throw "Shared state changed in case '$($case.Name)': $path"
            }
        }
        if ((Test-Path (Join-Path $desktop 'amarcode.exe')) -or (Test-Path (Join-Path $desktop 'amarcode-bootstrap.exe'))) {
            throw "Desktop files were not removed: $($case.Name)"
        }
        if ([IO.File]::ReadAllText((Join-Path $desktop 'hook-result.txt')) -ne '0') {
            throw "Tauri data deletion remained enabled: $($case.Name)"
        }
        Write-Output "PASS: $($case.Name) preserves the independent daemon and instance data"
    }
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
