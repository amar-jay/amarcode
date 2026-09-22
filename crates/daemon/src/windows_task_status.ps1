$ErrorActionPreference = 'Stop'
try {
    $scheduler = New-Object -ComObject 'Schedule.Service'
    $scheduler.Connect()
    $folder = $scheduler.GetFolder('\')
    try {
        $task = $folder.GetTask('Amarcode Daemon')
    } catch {
        $failure = $_.Exception
        while ($null -ne $failure.InnerException) {
            $failure = $failure.InnerException
        }
        # HRESULT_FROM_WIN32(ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND).
        # Other failures must reach
        # the caller; an access error does not mean the task needs reinstalling.
        if ($failure.HResult -in @(-2147024894, -2147024893)) {
            [Console]::Out.WriteLine('-1')
            exit 0
        }
        throw
    }
    [Console]::Out.WriteLine([int]$task.State)
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
