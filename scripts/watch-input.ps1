[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string] $Branch,

    [string] $InputDirectory = 'input',

    [string] $OutputDirectory = 'output',

    [ValidateRange(2, 3600)]
    [int] $PollSeconds = 10,

    [ValidateRange(1, 340)]
    [int] $MaxRuntimeMinutes = 320,

    [ValidateRange(1, 300)]
    [int] $MaximumCommandMinutes = 60
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Git {
    param(
        [Parameter(Mandatory)]
        [string[]] $Arguments
    )

    & git @Arguments
    $ExitCode = $LASTEXITCODE

    if ($ExitCode -ne 0) {
        throw (
            "git $($Arguments -join ' ') failed with " +
            "exit code $ExitCode."
        )
    }
}

function Get-GitText {
    param(
        [Parameter(Mandatory)]
        [string[]] $Arguments
    )

    $Output = @(& git @Arguments)
    $ExitCode = $LASTEXITCODE

    if ($ExitCode -ne 0) {
        throw (
            "git $($Arguments -join ' ') failed with " +
            "exit code $ExitCode."
        )
    }

    return (($Output -join [Environment]::NewLine).Trim())
}

$RepositoryRoot = Get-GitText -Arguments @(
    'rev-parse',
    '--show-toplevel'
)

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    throw 'The worker must run inside a Git repository.'
}

Set-Location -LiteralPath $RepositoryRoot

& git check-ref-format --branch $Branch *> $null

if ($LASTEXITCODE -ne 0) {
    throw "Invalid branch name: $Branch"
}

$RemoteTrackingRef = "refs/remotes/origin/$Branch"
$RemoteHeadRef = "refs/heads/$Branch"
$FetchRefSpec = (
    '+refs/heads/{0}:refs/remotes/origin/{0}' -f $Branch
)

$InputPath = [IO.Path]::GetFullPath(
    (Join-Path $RepositoryRoot $InputDirectory)
)

$OutputPath = [IO.Path]::GetFullPath(
    (Join-Path $RepositoryRoot $OutputDirectory)
)

$RepositoryPrefix = (
    [IO.Path]::GetFullPath($RepositoryRoot).TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) +
    [IO.Path]::DirectorySeparatorChar
)

foreach ($DirectoryCheck in @(
    [ordered] @{
        Name = 'InputDirectory'
        Path = $InputPath
    },
    [ordered] @{
        Name = 'OutputDirectory'
        Path = $OutputPath
    }
)) {
    if (
        -not $DirectoryCheck.Path.StartsWith(
            $RepositoryPrefix,
            [StringComparison]::OrdinalIgnoreCase
        )
    ) {
        throw (
            "$($DirectoryCheck.Name) must be inside " +
            "the repository: $($DirectoryCheck.Path)"
        )
    }
}

$InputGitPath = (
    [IO.Path]::GetRelativePath($RepositoryRoot, $InputPath).
        Replace('\', '/')
)

$OutputGitPath = (
    [IO.Path]::GetRelativePath($RepositoryRoot, $OutputPath).
        Replace('\', '/')
)

$PowerShellExecutable = (
    Get-Command pwsh -ErrorAction Stop
).Source

$Deadline = [DateTimeOffset]::UtcNow.AddMinutes(
    $MaxRuntimeMinutes
)

# Reserve time inside the watcher for repository restoration, result writing,
# pushing, and normal shutdown. The workflow itself has additional time for
# replacement dispatch and the actions/cache post-step.
$ShutdownReserveSeconds = 300

git config user.name 'github-actions[bot]'
git config user.email `
    '41898282+github-actions[bot]@users.noreply.github.com'

function Fetch-WatchedBranch {
    Invoke-Git -Arguments @(
        'fetch',
        '--no-tags',
        'origin',
        $FetchRefSpec
    )
}

function Assert-WatchedBranchCheckout {
    $CurrentBranch = Get-GitText -Arguments @(
        'branch',
        '--show-current'
    )

    if ($CurrentBranch -ne $Branch) {
        throw (
            "Expected checked-out branch '$Branch', " +
            "but found '$CurrentBranch'."
        )
    }
}

function Initialize-WatchedBranch {
    Fetch-WatchedBranch

    Invoke-Git -Arguments @(
        'checkout',
        '--force',
        '-B',
        $Branch,
        $RemoteTrackingRef
    )

    Invoke-Git -Arguments @(
        'reset',
        '--hard',
        $RemoteTrackingRef
    )

    Assert-WatchedBranchCheckout
}

function Sync-Repository {
    Assert-WatchedBranchCheckout
    Fetch-WatchedBranch

    $LocalCommit = Get-GitText -Arguments @(
        'rev-parse',
        'HEAD'
    )

    $RemoteCommit = Get-GitText -Arguments @(
        'rev-parse',
        $RemoteTrackingRef
    )

    if ($LocalCommit -eq $RemoteCommit) {
        return
    }

    Write-Host (
        "Branch $Branch changed: " +
        "$LocalCommit -> $RemoteCommit"
    )

    $Status = Get-GitText -Arguments @(
        'status',
        '--porcelain'
    )

    if (-not [string]::IsNullOrWhiteSpace($Status)) {
        throw (
            "Cannot update $Branch because the worker " +
            'repository contains local changes.'
        )
    }

    # Rebase only against the explicitly fetched monitored branch.
    Invoke-Git -Arguments @(
        'rebase',
        $RemoteTrackingRef
    )
}

function Stop-AnyGitOperation {
    # These commands are best-effort cleanup. A non-zero result usually means
    # that the corresponding operation was not active.
    & git rebase --abort *> $null
    & git merge --abort *> $null
    & git cherry-pick --abort *> $null
    & git revert --abort *> $null
}

function Restore-RepositoryAfterCommand {
    # Preserve ignored build directories such as target/.
    # Remove tracked and untracked changes created by the command.
    Stop-AnyGitOperation
    Fetch-WatchedBranch

    Invoke-Git -Arguments @(
        'checkout',
        '--force',
        '-B',
        $Branch,
        $RemoteTrackingRef
    )

    Invoke-Git -Arguments @(
        'reset',
        '--hard',
        $RemoteTrackingRef
    )

    Invoke-Git -Arguments @(
        'clean',
        '-fd'
    )

    Assert-WatchedBranchCheckout
}

function Push-Output {
    param(
        [Parameter(Mandatory)]
        [string] $TaskName,

        [Parameter(Mandatory)]
        [string] $CommandHash
    )

    Assert-WatchedBranchCheckout

    Invoke-Git -Arguments @(
        'add',
        '--',
        $OutputGitPath
    )

    & git diff --cached --quiet --exit-code
    $DiffExitCode = $LASTEXITCODE

    if ($DiffExitCode -eq 0) {
        Write-Host 'No output changes to commit.'
        return
    }

    if ($DiffExitCode -ne 1) {
        throw (
            'Could not inspect staged output changes; ' +
            "git diff exited with $DiffExitCode."
        )
    }

    $ShortHash = $CommandHash.Substring(0, 16)

    Invoke-Git -Arguments @(
        'commit',
        '-m',
        "automation: save result for $TaskName [$ShortHash]"
    )

    for ($Attempt = 1; $Attempt -le 10; $Attempt++) {
        try {
            Fetch-WatchedBranch

            Invoke-Git -Arguments @(
                'rebase',
                $RemoteTrackingRef
            )

            # The destination is always the exact branch assigned to
            # this worker. No configured upstream is used.
            Invoke-Git -Arguments @(
                'push',
                'origin',
                "HEAD:$RemoteHeadRef"
            )

            Write-Host (
                "Output pushed to $Branch for $TaskName."
            )
            return
        }
        catch {
            $Failure = $_.Exception.Message

            # Preserve the local result commit before another retry.
            & git rebase --abort *> $null

            if ($Attempt -ge 10) {
                throw (
                    "Could not push output to $Branch after " +
                    "10 attempts. Last error: $Failure"
                )
            }

            Write-Warning (
                "Push attempt $Attempt for $Branch failed: " +
                $Failure
            )

            Start-Sleep -Seconds (
                [Math]::Min(5 * $Attempt, 30)
            )
        }
    }
}

function Invoke-CommandFile {
    param(
        [Parameter(Mandatory)]
        [System.IO.FileInfo] $CommandFile,

        [Parameter(Mandatory)]
        [int] $TimeoutSeconds
    )

    $Started = [DateTimeOffset]::UtcNow
    $ExitCode = 125
    $Status = 'launcher_error'
    $TimedOut = $false
    $StandardOutput = ''
    $StandardError = ''
    $Process = $null
    $StdoutTask = $null
    $StderrTask = $null

    try {
        $StartInfo = (
            [System.Diagnostics.ProcessStartInfo]::new()
        )

        $StartInfo.FileName = $PowerShellExecutable
        $StartInfo.WorkingDirectory = $RepositoryRoot
        $StartInfo.UseShellExecute = $false
        $StartInfo.CreateNoWindow = $true
        $StartInfo.RedirectStandardOutput = $true
        $StartInfo.RedirectStandardError = $true

        [void] $StartInfo.ArgumentList.Add('-NoLogo')
        [void] $StartInfo.ArgumentList.Add('-NoProfile')
        [void] $StartInfo.ArgumentList.Add('-NonInteractive')
        [void] $StartInfo.ArgumentList.Add('-ExecutionPolicy')
        [void] $StartInfo.ArgumentList.Add('Bypass')
        [void] $StartInfo.ArgumentList.Add('-File')
        [void] $StartInfo.ArgumentList.Add(
            $CommandFile.FullName
        )

        $Process = [System.Diagnostics.Process]::new()
        $Process.StartInfo = $StartInfo

        if (-not $Process.Start()) {
            throw 'Could not start the PowerShell child process.'
        }

        # Asynchronous reads prevent stdout/stderr pipe deadlocks.
        $StdoutTask = $Process.StandardOutput.ReadToEndAsync()
        $StderrTask = $Process.StandardError.ReadToEndAsync()

        $Completed = $Process.WaitForExit(
            $TimeoutSeconds * 1000
        )

        if (-not $Completed) {
            $TimedOut = $true
            $Status = 'timed_out'
            $ExitCode = 124

            try {
                $Process.Kill($true)
                $Process.WaitForExit()
            }
            catch {
                $StandardError += (
                    "Could not terminate command process tree: " +
                    $_.Exception.Message
                )
            }
        }
        else {
            $ExitCode = $Process.ExitCode

            if ($ExitCode -eq 0) {
                $Status = 'success'
            }
            else {
                $Status = 'command_failed'
            }
        }

        if ($null -ne $StdoutTask) {
            $StandardOutput = (
                $StdoutTask.GetAwaiter().GetResult()
            )
        }

        if ($null -ne $StderrTask) {
            $CapturedError = (
                $StderrTask.GetAwaiter().GetResult()
            )

            if (
                $StandardError.Length -gt 0 -and
                $CapturedError.Length -gt 0
            ) {
                $StandardError += [Environment]::NewLine
            }

            $StandardError += $CapturedError
        }
    }
    catch {
        $Status = 'launcher_error'
        $ExitCode = 125

        if ($StandardError.Length -gt 0) {
            $StandardError += [Environment]::NewLine
        }

        $StandardError += ($_ | Out-String)
    }
    finally {
        if ($null -ne $Process) {
            $Process.Dispose()
        }
    }

    $Finished = [DateTimeOffset]::UtcNow

    return [ordered] @{
        status = $Status
        exit_code = $ExitCode
        timed_out = $TimedOut
        started_utc = $Started.ToString('o')
        finished_utc = $Finished.ToString('o')
        duration_seconds = [Math]::Round(
            ($Finished - $Started).TotalSeconds,
            3
        )
        stdout = $StandardOutput
        stderr = $StandardError
    }
}

Initialize-WatchedBranch

New-Item `
    -ItemType Directory `
    -Path $InputPath `
    -Force |
    Out-Null

New-Item `
    -ItemType Directory `
    -Path $OutputPath `
    -Force |
    Out-Null

Write-Host "Monitoring branch: $Branch"
Write-Host "Remote source ref: refs/heads/$Branch"
Write-Host "Remote push ref: $RemoteHeadRef"
Write-Host "Input directory: $InputGitPath"
Write-Host "Output directory: $OutputGitPath"
Write-Host "Worker deadline: $($Deadline.ToString('o'))"

:WorkerLoop while (
    [DateTimeOffset]::UtcNow -lt $Deadline
) {
    try {
        Sync-Repository
    }
    catch {
        Write-Warning (
            "Repository synchronization for $Branch failed: " +
            $_.Exception.Message
        )

        Start-Sleep -Seconds $PollSeconds
        continue
    }

    if (
        Test-Path `
            -LiteralPath (
                Join-Path `
                    $RepositoryRoot `
                    '.continuous-worker.stop'
            )
    ) {
        Write-Host "Stop file detected on branch $Branch."
        break
    }

    New-Item `
        -ItemType Directory `
        -Path $InputPath `
        -Force |
        Out-Null

    New-Item `
        -ItemType Directory `
        -Path $OutputPath `
        -Force |
        Out-Null

    $CommandFiles = @(
        Get-ChildItem `
            -LiteralPath $InputPath `
            -File `
            -Filter '*.ps1' `
            -Recurse |
        Sort-Object FullName
    )

    $ProcessedSomething = $false

    foreach ($CommandFile in $CommandFiles) {
        $Hash = (
            Get-FileHash `
                -LiteralPath $CommandFile.FullName `
                -Algorithm SHA256
        ).Hash.ToLowerInvariant()

        # Result names are hash-only, so identical command contents are
        # executed at most once even if the file is renamed.
        $ResultFileName = "$Hash.json"
        $LogFileName = "$Hash.log"

        $ResultPath = Join-Path `
            $OutputPath `
            $ResultFileName

        $LogPath = Join-Path `
            $OutputPath `
            $LogFileName

        if (Test-Path -LiteralPath $ResultPath) {
            continue
        }

        $RemainingSeconds = [Math]::Floor(
            (
                $Deadline -
                [DateTimeOffset]::UtcNow
            ).TotalSeconds -
            $ShutdownReserveSeconds
        )

        if ($RemainingSeconds -lt 30) {
            Write-Host 'Rollover deadline reached.'
            break WorkerLoop
        }

        $CommandTimeoutSeconds = [Math]::Min(
            $MaximumCommandMinutes * 60,
            $RemainingSeconds
        )

        $CommandRelativePath = (
            [IO.Path]::GetRelativePath(
                $RepositoryRoot,
                $CommandFile.FullName
            ).Replace('\', '/')
        )

        $SourceCommit = Get-GitText -Arguments @(
            'rev-parse',
            'HEAD'
        )

        Write-Host (
            "Executing $CommandRelativePath from " +
            "$SourceCommit; timeout: " +
            "$CommandTimeoutSeconds seconds."
        )

        $CommandResult = Invoke-CommandFile `
            -CommandFile $CommandFile `
            -TimeoutSeconds $CommandTimeoutSeconds

        try {
            Restore-RepositoryAfterCommand
        }
        catch {
            # A repository restoration failure is an infrastructure failure,
            # not an ordinary command failure. Do not risk pushing command-
            # created commits or pushing from the wrong local branch.
            throw (
                "Repository restoration failed after " +
                "$CommandRelativePath: " +
                $_.Exception.Message
            )
        }

        New-Item `
            -ItemType Directory `
            -Path $OutputPath `
            -Force |
            Out-Null

        $Log = @(
            '=== STANDARD OUTPUT ==='
            $CommandResult.stdout
            ''
            '=== STANDARD ERROR ==='
            $CommandResult.stderr
        ) -join [Environment]::NewLine

        [IO.File]::WriteAllText(
            $LogPath,
            $Log,
            [Text.UTF8Encoding]::new($false)
        )

        $Metadata = [ordered] @{
            command_file = $CommandFile.Name
            command_path = $CommandRelativePath
            command_sha256 = $Hash
            source_commit = $SourceCommit
            status = $CommandResult.status
            exit_code = $CommandResult.exit_code
            timed_out = $CommandResult.timed_out
            started_utc = $CommandResult.started_utc
            finished_utc = $CommandResult.finished_utc
            duration_seconds = (
                $CommandResult.duration_seconds
            )
            log_file = $LogFileName
            branch = $Branch
            repository = $env:GITHUB_REPOSITORY
            workflow_run_id = $env:GITHUB_RUN_ID
            workflow_run_attempt = (
                $env:GITHUB_RUN_ATTEMPT
            )
            runner_name = $env:RUNNER_NAME
        }

        [IO.File]::WriteAllText(
            $ResultPath,
            ($Metadata | ConvertTo-Json -Depth 5),
            [Text.UTF8Encoding]::new($false)
        )

        # Push failures are worker failures. Ordinary command non-zero exits
        # and timeouts have already been recorded and still reach this step.
        Push-Output `
            -TaskName $CommandRelativePath `
            -CommandHash $Hash

        $ProcessedSomething = $true

        # Re-fetch and rescan after every command. This prevents processing
        # stale FileInfo objects if the branch changed during execution.
        break
    }

    if (-not $ProcessedSomething) {
        Start-Sleep -Seconds $PollSeconds
    }
}

Write-Host (
    "Continuous worker for $Branch is ending normally " +
    'for rollover or stop.'
)

exit 0
