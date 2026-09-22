# Exercise the production query without creating, stopping, or changing tasks.
$ErrorActionPreference = 'Stop'
$query = Get-Content -Raw (Join-Path $PSScriptRoot '../crates/daemon/src/windows_task_status.ps1')
$cases = @(
    @{ Name = 'unknown'; State = 0; Expected = '0'; Exit = 0 },
    @{ Name = 'disabled'; State = 1; Expected = '1'; Exit = 0 },
    @{ Name = 'queued'; State = 2; Expected = '2'; Exit = 0 },
    @{ Name = 'ready'; State = 3; Expected = '3'; Exit = 0 },
    @{ Name = 'running'; State = 4; Expected = '4'; Exit = 0 },
    @{ Name = 'missing file'; Failure = -2147024894; Expected = '-1'; Exit = 0 },
    @{ Name = 'missing path'; Failure = -2147024893; Expected = '-1'; Exit = 0 },
    @{ Name = 'access denied'; Failure = -2147024891; Expected = ''; Exit = 1 },
    @{ Name = 'scheduler unavailable'; ConnectFailure = $true; Expected = ''; Exit = 1 }
)
foreach ($case in $cases) {
    $fixture = @'
function New-Object {
    param([string]$ComObject)
    if ($ComObject -ne 'Schedule.Service') { throw 'Unexpected COM object' }
    $scheduler = [pscustomobject]@{}
    $scheduler | Add-Member ScriptMethod Connect {
        if ($script:connectFailure) { throw 'Scheduler unavailable' }
    }
    $scheduler | Add-Member ScriptMethod GetFolder {
        param($path)
        if ($path -ne '\') { throw 'Unexpected folder' }
        $folder = [pscustomobject]@{}
        $folder | Add-Member ScriptMethod GetTask {
            param($name)
            if ($name -ne 'Amarcode Daemon') { throw 'Unexpected task' }
            if ($script:failure) {
                throw [Runtime.InteropServices.COMException]::new('Task lookup failed', $script:failure)
            }
            return [pscustomobject]@{ State = $script:state }
        }
        return $folder
    }
    return $scheduler
}
'@
    $setup = '$script:state = ' + [int]$case.State + '; $script:failure = ' + [int]$case.Failure +
        '; $script:connectFailure = ' + [int][bool]$case.ConnectFailure + "`n"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($setup + $fixture + "`n" + $query))
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = 'powershell.exe'
    $info.Arguments = '-NoLogo -NoProfile -NonInteractive -EncodedCommand ' + $encoded
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEnd().Trim()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne $case.Exit -or $stdout -ne $case.Expected) {
        throw "Case '$($case.Name)' failed: exit=$($process.ExitCode), stdout=$stdout, stderr=$stderr"
    }
    $process.Dispose()
    Write-Output "PASS: $($case.Name)"
}
