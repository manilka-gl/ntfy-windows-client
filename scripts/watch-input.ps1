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

function New-WorkerTemporaryDirectory {
    param(
        [Parameter(Mandatory)]
        [string] $Prefix
    )

    $TemporaryRoot = $env:RUNNER_TEMP

    if ([string]::IsNullOrWhiteSpace($TemporaryRoot)) {
        $TemporaryRoot = [IO.Path]::GetTempPath()
    }

    $Path = Join-Path `
        $TemporaryRoot `
        ($Prefix + [Guid]::NewGuid().ToString('N'))

    New-Item -ItemType Directory -Path $Path -Force | Out-Null
    return $Path
}

function Assert-PathInsideRepository {
    param(
        [Parameter(Mandatory)]
        [string] $Name,

        [Parameter(Mandatory)]
        [string] $Path,

        [Parameter(Mandatory)]
        [string] $RepositoryPrefix
    )

    if (
        -not $Path.StartsWith(
            $RepositoryPrefix,
            [StringComparison]::OrdinalIgnoreCase
        )
    ) {
        throw "$Name must be inside the repository: $Path"
    }
}

function Copy-FileWithParentDirectory {
    param(
        [Parameter(Mandatory)]
        [string] $Source,

        [Parameter(Mandatory)]
        [string] $Destination
    )

    $Parent = Split-Path -Parent $Destination

    if (-not [string]::IsNullOrWhiteSpace($Parent)) {
        New-Item -ItemType Directory -Path $Parent -Force |
            Out-Null
    }

    Copy-Item `
        -LiteralPath $Source `
        -Destination $Destination `
        -Force
}

function Get-DirectoryFileState {
    param(
        [Parameter(Mandatory)]
        [string] $Directory
    )

    $State = @{}

    if (-not (Test-Path -LiteralPath $Directory)) {
        return $State
    }

    $Files = @(
        Get-ChildItem `
            -LiteralPath $Directory `
            -File `
            -Force `
            -Recurse
    )

    foreach ($File in $Files) {
        $RelativePath = (
            [IO.Path]::GetRelativePath(
                $Directory,
                $File.FullName
            ).Replace('\', '/')
        )

        $Hash = (
            Get-FileHash `
                -LiteralPath $File.FullName `
                -Algorithm SHA256
        ).Hash.ToLowerInvariant()

        $State[$RelativePath] = $Hash
    }

    return $State
}

function New-OutputDelta {
    param(
        [Parameter(Mandatory)]
        [hashtable] $Before,

        [Parameter(Mandatory)]
        [hashtable] $After,

        [Parameter(Mandatory)]
        [string] $OutputPath
    )

    $StagePath = New-WorkerTemporaryDirectory `
        -Prefix 'continuous-worker-output-'

    $ChangedPaths = [System.Collections.Generic.List[string]]::new()
    $DeletedPaths = [System.Collections.Generic.List[string]]::new()

    foreach ($RelativePath in $After.Keys) {
        $Changed = (
            -not $Before.ContainsKey($RelativePath) -or
            $Before[$RelativePath] -ne $After[$RelativePath]
        )

        if (-not $Changed) {
            continue
        }

        $ChangedPaths.Add($RelativePath)

        $Source = Join-Path `
            $OutputPath `
            ($RelativePath.Replace('/', '\'))

        $Destination = Join-Path `
            $StagePath `
            ($RelativePath.Replace('/', '\'))

        Copy-FileWithParentDirectory `
            -Source $Source `
            -Destination $Destination
    }

    foreach ($RelativePath in $Before.Keys) {
        if (-not $After.ContainsKey($RelativePath)) {
            $DeletedPaths.Add($RelativePath)
        }
    }

    return [pscustomobject] @{
        StagePath = $StagePath
        ChangedPaths = $ChangedPaths.ToArray()
        DeletedPaths = $DeletedPaths.ToArray()
    }
}

function Apply-OutputDelta {
    param(
        [Parameter(Mandatory)]
        [pscustomobject] $Delta,

        [Parameter(Mandatory)]
        [string] $OutputPath,

        [Parameter(Mandatory)]
        [string] $OutputPrefix
    )

    New-Item -ItemType Directory -Path $OutputPath -Force |
        Out-Null

    foreach ($RelativePath in $Delta.DeletedPaths) {
        $Destination = [IO.Path]::GetFullPath(
            (Join-Path $OutputPath $RelativePath)
        )

        if (
            -not $Destination.StartsWith(
                $OutputPrefix,
                [StringComparison]::OrdinalIgnoreCase
            )
        ) {
            throw "Invalid output deletion path: $RelativePath"
        }

        if (Test-Path -LiteralPath $Destination) {
            Remove-Item -LiteralPath $Destination -Force
        }
    }

    foreach ($RelativePath in $Delta.ChangedPaths) {
        $Source = Join-Path `
            $Delta.StagePath `
            ($RelativePath.Replace('/', '\'))

        $Destination = [IO.Path]::GetFullPath(
            (Join-Path $OutputPath $RelativePath)
        )

        if (
            -not $Destination.StartsWith(
                $OutputPrefix,
                [StringComparison]::OrdinalIgnoreCase
            )
        ) {
            throw "Invalid output copy path: $RelativePath"
        }

        if (
            Test-Path `
                -LiteralPath $Destination `
                -PathType Container
        ) {
            Remove-Item `
                -LiteralPath $Destination `
                -Recurse `
                -Force
        }

        Copy-FileWithParentDirectory `
            -Source $Source `
            -Destination $Destination
    }
}

function Remove-TemporaryPath {
    param(
        [AllowNull()]
        [string] $Path
    )

    if (
        -not [string]::IsNullOrWhiteSpace($Path) -and
        (Test-Path -LiteralPath $Path)
    ) {
        Remove-Item `
            -LiteralPath $Path `
            -Recurse `
            -Force `
            -ErrorAction SilentlyContinue
    }
}

function Write-Utf8Bytes {
    param(
        [Parameter(Mandatory)]
        [IO.Stream] $Stream,

        [Parameter(Mandatory)]
        [string] $Text
    )

    $Bytes = [Text.UTF8Encoding]::new($false).GetBytes($Text)
    $Stream.Write($Bytes, 0, $Bytes.Length)
}

function Write-CombinedLog {
    param(
        [Parameter(Mandatory)]
        [string] $Destination,

        [Parameter(Mandatory)]
        [string] $StandardOutputPath,

        [Parameter(Mandatory)]
        [string] $StandardErrorPath
    )

    $Parent = Split-Path -Parent $Destination
    New-Item -ItemType Directory -Path $Parent -Force |
        Out-Null

    $DestinationStream = [IO.File]::Open(
        $Destination,
        [IO.FileMode]::Create,
        [IO.FileAccess]::Write,
        [IO.FileShare]::None
    )

    try {
        Write-Utf8Bytes `
            -Stream $DestinationStream `
            -Text (
                '=== STANDARD OUTPUT ===' +
                [Environment]::NewLine
            )

        if (Test-Path -LiteralPath $StandardOutputPath) {
            $SourceStream = [IO.File]::OpenRead(
                $StandardOutputPath
            )

            try {
                $SourceStream.CopyTo($DestinationStream)
            }
            finally {
                $SourceStream.Dispose()
            }
        }

        Write-Utf8Bytes `
            -Stream $DestinationStream `
            -Text (
                [Environment]::NewLine +
                [Environment]::NewLine +
                '=== STANDARD ERROR ===' +
                [Environment]::NewLine
            )

        if (Test-Path -LiteralPath $StandardErrorPath) {
            $SourceStream = [IO.File]::OpenRead(
                $StandardErrorPath
            )

            try {
                $SourceStream.CopyTo($DestinationStream)
            }
            finally {
                $SourceStream.Dispose()
            }
        }
    }
    finally {
        $DestinationStream.Dispose()
    }
}

$RepositoryRoot = Get-GitText -Arguments @(
    'rev-parse',
    '--show-toplevel'
)

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    throw 'The worker must run inside a Git repository.'
}

$RepositoryRoot = [IO.Path]::GetFullPath($RepositoryRoot)
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
    $RepositoryRoot.TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) +
    [IO.Path]::DirectorySeparatorChar
)

Assert-PathInsideRepository `
    -Name 'InputDirectory' `
    -Path $InputPath `
    -RepositoryPrefix $RepositoryPrefix

Assert-PathInsideRepository `
    -Name 'OutputDirectory' `
    -Path $OutputPath `
    -RepositoryPrefix $RepositoryPrefix

$OutputPrefix = (
    $OutputPath.TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) +
    [IO.Path]::DirectorySeparatorChar
)

$InputGitPath = [IO.Path]::GetRelativePath(
    $RepositoryRoot,
    $InputPath
).Replace('\', '/')

$OutputGitPath = [IO.Path]::GetRelativePath(
    $RepositoryRoot,
    $OutputPath
).Replace('\', '/')

$PowerShellExecutable = (
    Get-Command pwsh -ErrorAction Stop
).Source

$Deadline = [DateTimeOffset]::UtcNow.AddMinutes(
    $MaxRuntimeMinutes
)

# Time reserved inside the watcher for result creation, pushing, and shutdown.
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

function Stop-AnyGitOperation {
    # Input commands are intentionally unrestricted. These best-effort aborts
    # only return the checkout to a usable state after a command finishes.
    & git rebase --abort *> $null
    & git merge --abort *> $null
    & git cherry-pick --abort *> $null
    & git revert --abort *> $null
    & git am --abort *> $null
}

function Set-WatchedBranchToCommit {
    param(
        [Parameter(Mandatory)]
        [string] $Commit
    )

    Stop-AnyGitOperation

    # This resets tracked files and the index. It does not perform a broad
    # untracked/ignored-file cleanup, so target/ and Cargo build state remain.
    Invoke-Git -Arguments @(
        'checkout',
        '--force',
        '-B',
        $Branch,
        $Commit
    )

    Invoke-Git -Arguments @(
        'reset',
        '--hard',
        $Commit
    )

    Assert-WatchedBranchCheckout
}

function Initialize-WatchedBranch {
    Fetch-WatchedBranch
    Set-WatchedBranchToCommit -Commit $RemoteTrackingRef
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

    Set-WatchedBranchToCommit -Commit $RemoteTrackingRef
}

function Test-CommittedStopFile {
    & git cat-file -e 'HEAD:.continuous-worker.stop' 2>$null
    return ($LASTEXITCODE -eq 0)
}

function Test-CommittedResult {
    param(
        [Parameter(Mandatory)]
        [string] $ResultGitPath
    )

    $Reference = "HEAD:$ResultGitPath"
    & git cat-file -e $Reference 2>$null
    return ($LASTEXITCODE -eq 0)
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
    $Process = $null
    $StandardOutputStream = $null
    $StandardErrorStream = $null
    $StandardOutputTask = $null
    $StandardErrorTask = $null
    $InfrastructureError = ''

    $CaptureDirectory = New-WorkerTemporaryDirectory `
        -Prefix 'continuous-worker-capture-'

    $StandardOutputPath = Join-Path `
        $CaptureDirectory `
        'stdout.txt'

    $StandardErrorPath = Join-Path `
        $CaptureDirectory `
        'stderr.txt'

    try {
        $StartInfo = [System.Diagnostics.ProcessStartInfo]::new()
        $StartInfo.FileName = $PowerShellExecutable
        $StartInfo.WorkingDirectory = $RepositoryRoot
        $StartInfo.UseShellExecute = $false
        $StartInfo.CreateNoWindow = $true
        $StartInfo.RedirectStandardOutput = $true
        $StartInfo.RedirectStandardError = $true

        # Commands receive the normal job environment, including GH_TOKEN and
        # checkout credentials. No branch-access restriction is applied.
        $StartInfo.Environment['WORKER_BRANCH'] = $Branch
        $StartInfo.Environment['WORKER_OUTPUT_DIRECTORY'] = (
            $OutputPath
        )

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

        $StandardOutputStream = [IO.File]::Open(
            $StandardOutputPath,
            [IO.FileMode]::Create,
            [IO.FileAccess]::Write,
            [IO.FileShare]::Read
        )

        $StandardErrorStream = [IO.File]::Open(
            $StandardErrorPath,
            [IO.FileMode]::Create,
            [IO.FileAccess]::Write,
            [IO.FileShare]::Read
        )

        $StandardOutputTask = (
            $Process.StandardOutput.BaseStream.CopyToAsync(
                $StandardOutputStream
            )
        )

        $StandardErrorTask = (
            $Process.StandardError.BaseStream.CopyToAsync(
                $StandardErrorStream
            )
        )

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
                $InfrastructureError += (
                    "Could not terminate command process tree: " +
                    $_.Exception.Message
                )
            }
        }
        else {
            $Process.WaitForExit()
            $ExitCode = $Process.ExitCode

            if ($ExitCode -eq 0) {
                $Status = 'success'
            }
            else {
                $Status = 'command_failed'
            }
        }

        $StandardOutputTask.GetAwaiter().GetResult()
        $StandardErrorTask.GetAwaiter().GetResult()
    }
    catch {
        $Status = 'launcher_error'
        $ExitCode = 125

        if ($InfrastructureError.Length -gt 0) {
            $InfrastructureError += [Environment]::NewLine
        }

        $InfrastructureError += ($_ | Out-String)
    }
    finally {
        if (
            $null -ne $StandardOutputTask -and
            -not $StandardOutputTask.IsCompleted
        ) {
            try {
                $StandardOutputTask.GetAwaiter().GetResult()
            }
            catch {
                if ($InfrastructureError.Length -gt 0) {
                    $InfrastructureError += [Environment]::NewLine
                }

                $InfrastructureError += (
                    "stdout capture failed: " +
                    $_.Exception.Message
                )
            }
        }

        if (
            $null -ne $StandardErrorTask -and
            -not $StandardErrorTask.IsCompleted
        ) {
            try {
                $StandardErrorTask.GetAwaiter().GetResult()
            }
            catch {
                if ($InfrastructureError.Length -gt 0) {
                    $InfrastructureError += [Environment]::NewLine
                }

                $InfrastructureError += (
                    "stderr capture failed: " +
                    $_.Exception.Message
                )
            }
        }

        if ($null -ne $StandardOutputStream) {
            $StandardOutputStream.Dispose()
        }

        if ($null -ne $StandardErrorStream) {
            $StandardErrorStream.Dispose()
        }

        if ($null -ne $Process) {
            $Process.Dispose()
        }
    }

    if (-not (Test-Path -LiteralPath $StandardOutputPath)) {
        [IO.File]::WriteAllBytes(
            $StandardOutputPath,
            [byte[]]::new(0)
        )
    }

    if (-not (Test-Path -LiteralPath $StandardErrorPath)) {
        [IO.File]::WriteAllBytes(
            $StandardErrorPath,
            [byte[]]::new(0)
        )
    }

    if ($InfrastructureError.Length -gt 0) {
        [IO.File]::AppendAllText(
            $StandardErrorPath,
            (
                [Environment]::NewLine +
                $InfrastructureError
            ),
            [Text.UTF8Encoding]::new($false)
        )
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
        capture_directory = $CaptureDirectory
        stdout_path = $StandardOutputPath
        stderr_path = $StandardErrorPath
    }
}

function Push-Output {
    param(
        [Parameter(Mandatory)]
        [string] $TaskName,

        [Parameter(Mandatory)]
        [string] $CommandHash
    )

    Assert-WatchedBranchCheckout

    # Force-add permits executable and other artifact types even when a
    # repository ignore rule normally excludes them. Nothing outside output/
    # is staged by this worker.
    Invoke-Git -Arguments @(
        'add',
        '-A',
        '-f',
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
        Fetch-WatchedBranch

        & git rebase $RemoteTrackingRef
        $RebaseExitCode = $LASTEXITCODE

        if ($RebaseExitCode -ne 0) {
            & git rebase --abort *> $null
            throw (
                "Could not rebase output for $TaskName onto " +
                "$Branch. A tracked or untracked path conflicts " +
                'with a newer remote commit.'
            )
        }

        # The watcher pushes only its assigned branch. Input commands remain
        # unrestricted and may perform their own Git or GitHub operations.
        & git push origin "HEAD:$RemoteHeadRef"
        $PushExitCode = $LASTEXITCODE

        if ($PushExitCode -eq 0) {
            Write-Host (
                "Output pushed to $Branch for $TaskName."
            )
            return
        }

        if ($Attempt -ge 10) {
            throw (
                "Could not push output to $Branch after " +
                '10 attempts.'
            )
        }

        Write-Warning (
            "Push attempt $Attempt for $Branch failed."
        )

        Start-Sleep -Seconds (
            [Math]::Min(5 * $Attempt, 30)
        )
    }
}

Initialize-WatchedBranch

New-Item -ItemType Directory -Path $InputPath -Force |
    Out-Null

New-Item -ItemType Directory -Path $OutputPath -Force |
    Out-Null

Write-Host "Monitoring branch: $Branch"
Write-Host "Fetch source: refs/heads/$Branch"
Write-Host "Push destination: $RemoteHeadRef"
Write-Host "Input directory: $InputGitPath"
Write-Host "Output directory: $OutputGitPath"
Write-Host 'No broad untracked/ignored-file cleanup is performed.'
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

    if (Test-CommittedStopFile) {
        Write-Host "Committed stop file detected on $Branch."
        break
    }

    New-Item -ItemType Directory -Path $InputPath -Force |
        Out-Null

    New-Item -ItemType Directory -Path $OutputPath -Force |
        Out-Null

    $CommandFiles = @(
        Get-ChildItem `
            -LiteralPath $InputPath `
            -File `
            -Filter '*.ps1' `
            -Force `
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

        $ResultFileName = "$Hash.json"
        $LogFileName = "$Hash.log"

        $ResultGitPath = (
            "$OutputGitPath/$ResultFileName"
        )

        if (
            Test-CommittedResult `
                -ResultGitPath $ResultGitPath
        ) {
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

        $OutputBefore = Get-DirectoryFileState `
            -Directory $OutputPath

        Write-Host (
            "Executing $CommandRelativePath from " +
            "$SourceCommit; timeout: " +
            "$CommandTimeoutSeconds seconds."
        )

        $CommandResult = $null
        $OutputDelta = $null

        try {
            $CommandResult = Invoke-CommandFile `
                -CommandFile $CommandFile `
                -TimeoutSeconds $CommandTimeoutSeconds

            $OutputAfter = Get-DirectoryFileState `
                -Directory $OutputPath

            $OutputDelta = New-OutputDelta `
                -Before $OutputBefore `
                -After $OutputAfter `
                -OutputPath $OutputPath

            # Revert tracked command side effects while preserving target/,
            # Cargo downloads, ignored files, and unrelated untracked files.
            Set-WatchedBranchToCommit -Commit $SourceCommit

            Apply-OutputDelta `
                -Delta $OutputDelta `
                -OutputPath $OutputPath `
                -OutputPrefix $OutputPrefix

            $ResultPath = Join-Path `
                $OutputPath `
                $ResultFileName

            $LogPath = Join-Path `
                $OutputPath `
                $LogFileName

            Write-CombinedLog `
                -Destination $LogPath `
                -StandardOutputPath $CommandResult.stdout_path `
                -StandardErrorPath $CommandResult.stderr_path

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

            # Non-zero command exits and command timeouts are normal recorded
            # results. Only worker/repository failures fail the worker.
            Push-Output `
                -TaskName $CommandRelativePath `
                -CommandHash $Hash

            $ProcessedSomething = $true
        }
        finally {
            if ($null -ne $CommandResult) {
                Remove-TemporaryPath `
                    -Path $CommandResult.capture_directory
            }

            if ($null -ne $OutputDelta) {
                Remove-TemporaryPath `
                    -Path $OutputDelta.StagePath
            }
        }

        # Re-fetch and rescan after each command so command files changed while
        # it was running are never processed from stale FileInfo objects.
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
