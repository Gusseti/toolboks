<#
.SYNOPSIS
  Measures what Toolboks itself costs: CPU, memory, handles, threads and wake-ups of the host
  process (and, separately, of any add-on process it started) in three window states.

.DESCRIPTION
  Starts the given Toolboks exe with its OWN data folder (TOOLBOKS_DATA_DIR), so it has its own
  single-instance lock and never joins or touches a normal Toolboks that is already running. Use a
  preview build (`cargo build --release --features preview`): it never checks for app updates.

  States, measured in this order on one running instance:
    open       the window is shown and left alone (no mouse or keys over it)
    minimized  the window is minimized to the taskbar
    tray       the window is closed with its X (WM_CLOSE) while "Keep running in the tray" is on,
               so only the tray icon is left

  For each state: apply it, wait -WarmUp seconds, then measure for -Seconds seconds.
    CPU          change in total processor time / elapsed time; shown as % of the whole PC
                 (all logical cores) and as % of one core
    Wake-ups/s   change in the summed context-switch count of the process's threads / elapsed
                 time (Win32_PerfRawData_PerfProc_Thread); a sleeping process has close to none
    Memory       working set and private bytes, averaged over samples every second
    Handles, threads   averaged over the same samples

  Prints a markdown table and appends it to -Out when given. Stops every process it started.

.EXAMPLE
  scripts/measure-host-overhead.ps1 -Exe D:/CodexBuilds/issue48/before/toolboks.exe `
      -DataDir D:/CodexBuilds/issue48/data-before -Label before -Out D:/CodexBuilds/issue48/before.md

.EXAMPLE
  # With one add-on: its page open in the "open" state (install it into -DataDir first)
  scripts/measure-host-overhead.ps1 -Exe ... -DataDir ... -Label "before + RM" -Open "Resource Monitor"
#>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$DataDir,
    [string]$Label = 'host',
    [string[]]$States = @('open', 'minimized', 'tray'),
    [int]$WarmUp = 15,
    [int]$Seconds = 60,
    [string]$Open = '',
    [string]$Out = '',
    # The app keeps its data in this folder inside -DataDir ("Toolboks" for a normal build).
    [string]$AppFolder = 'Toolboks Preview'
)
$ErrorActionPreference = 'Stop'

Add-Type -Namespace TbMeasure -Name Win -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool ShowWindow(System.IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")] public static extern bool PostMessage(System.IntPtr hWnd, uint msg, System.IntPtr w, System.IntPtr l);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool IsIconic(System.IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
[DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr hWnd, out RECT r);
public struct POINT { public int X; public int Y; }
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
'@

$cores = [Environment]::ProcessorCount

# Context switches so far, per thread id (threads that end lose their count, so a difference is
# taken per thread that is still there at the end: a slight undercount, never negative).
function Get-ContextSwitches([int]$ProcessId) {
    $map = @{}
    foreach ($t in Get-CimInstance -ClassName Win32_PerfRawData_PerfProc_Thread -Filter "IDProcess=$ProcessId" -ErrorAction SilentlyContinue) {
        $map[[int]$t.IDThread] = [uint64]$t.ContextSwitchesPersec
    }
    $map
}

function Get-SwitchesSince($Before, $After) {
    $sum = 0.0
    foreach ($id in $After.Keys) {
        $was = if ($Before.ContainsKey($id)) { $Before[$id] } else { 0 }
        if ($After[$id] -ge $was) { $sum += $After[$id] - $was }
    }
    $sum
}

# True when the mouse pointer is over the window (input there wakes the app and spoils "idle").
function Test-CursorOver([IntPtr]$Window) {
    $p = New-Object TbMeasure.Win+POINT; $r = New-Object TbMeasure.Win+RECT
    if (-not [TbMeasure.Win]::IsWindowVisible($Window) -or [TbMeasure.Win]::IsIconic($Window)) { return $false }
    [void][TbMeasure.Win]::GetCursorPos([ref]$p); [void][TbMeasure.Win]::GetWindowRect($Window, [ref]$r)
    $p.X -ge $r.Left -and $p.X -lt $r.Right -and $p.Y -ge $r.Top -and $p.Y -lt $r.Bottom
}

function Get-Children([int]$ProcessId) {
    @(Get-CimInstance -ClassName Win32_Process -Filter "ParentProcessId=$ProcessId" -ErrorAction SilentlyContinue |
            ForEach-Object { $_.ProcessId })
}

# Measures every given process over the same window; returns one result object per process.
function Measure-Processes([int[]]$Ids, [int]$Duration, [IntPtr]$Window) {
    $script:cursorSamples = 0; $script:cursorOver = 0
    $start = @{}
    foreach ($id in $Ids) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p) {
            $start[$id] = [pscustomobject]@{
                Process = $p; Name = $p.ProcessName; Cpu = $p.TotalProcessorTime
                Switches = Get-ContextSwitches $id; Samples = New-Object System.Collections.ArrayList
            }
        }
    }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt $Duration) {
        $script:cursorSamples++
        if (Test-CursorOver $Window) { $script:cursorOver++ }
        foreach ($entry in $start.Values) {
            $entry.Process.Refresh()
            if (-not $entry.Process.HasExited) {
                [void]$entry.Samples.Add([pscustomobject]@{
                        Ws = $entry.Process.WorkingSet64; Private = $entry.Process.PrivateMemorySize64
                        Handles = $entry.Process.HandleCount; Threads = $entry.Process.Threads.Count
                    })
            }
        }
        $left = $Duration - $clock.Elapsed.TotalSeconds
        if ($left -gt 0) { Start-Sleep -Milliseconds ([int](1000 * [Math]::Min(1, $left))) }
    }
    $elapsed = $clock.Elapsed.TotalSeconds
    foreach ($id in $start.Keys) {
        $entry = $start[$id]
        $entry.Process.Refresh()
        if ($entry.Process.HasExited -or $entry.Samples.Count -eq 0) { continue }
        $cpuSeconds = ($entry.Process.TotalProcessorTime - $entry.Cpu).TotalSeconds
        $switches = Get-SwitchesSince $entry.Switches (Get-ContextSwitches $id)
        $avg = { param($name) ($entry.Samples | Measure-Object -Property $name -Average).Average }
        [pscustomobject]@{
            Name       = $entry.Name
            CpuAll     = 100.0 * $cpuSeconds / ($elapsed * $cores)
            CpuCore    = 100.0 * $cpuSeconds / $elapsed
            WakeUps    = $switches / $elapsed
            WsMb       = (& $avg 'Ws') / 1MB
            PrivateMb  = (& $avg 'Private') / 1MB
            Handles    = & $avg 'Handles'
            Threads    = & $avg 'Threads'
            Elapsed    = $elapsed
        }
    }
}

# --- a fresh data folder: Home page, keep-in-tray on, no add-ons unless installed beforehand ---
# (first start done and the old-version migration marked done, so nothing is offered or installed)
$appData = Join-Path $DataDir $AppFolder
New-Item -ItemType Directory -Force -Path $appData | Out-Null
$config = Join-Path $appData 'config.json'
if (-not (Test-Path $config)) {
    Set-Content -Path $config -Encoding ascii -Value '{"first_run_complete":true,"keep_in_tray":true,"platform_migrated":true}'
}

$env:TOOLBOKS_DATA_DIR = (Resolve-Path $DataDir).Path
$arguments = @()
if ($Open) { $arguments = @('--open', "`"$Open`"") }
$started = if ($arguments.Count) { Start-Process -FilePath $Exe -ArgumentList $arguments -PassThru } else { Start-Process -FilePath $Exe -PassThru }
$hostId = $started.Id
$seen = New-Object System.Collections.Generic.HashSet[int]
try {
    $window = [IntPtr]::Zero
    for ($i = 0; $i -lt 60 -and $window -eq [IntPtr]::Zero; $i++) {
        Start-Sleep -Milliseconds 500
        $started.Refresh()
        $window = $started.MainWindowHandle
    }
    if ($window -eq [IntPtr]::Zero) { throw "Toolboks (pid $hostId) showed no window within 30 s" }

    $rows = @()
    foreach ($state in $States) {
        switch ($state) {
            'open' { [void][TbMeasure.Win]::ShowWindow($window, 9) }        # SW_RESTORE
            'minimized' { [void][TbMeasure.Win]::ShowWindow($window, 6) }   # SW_MINIMIZE
            'tray' { [void][TbMeasure.Win]::PostMessage($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) } # WM_CLOSE
            default { throw "unknown state $state" }
        }
        Start-Sleep -Seconds $WarmUp
        $visible = [TbMeasure.Win]::IsWindowVisible($window)
        $iconic = [TbMeasure.Win]::IsIconic($window)
        $shown = if (-not $visible) { 'hidden' } elseif ($iconic) { 'minimized' } else { 'visible' }
        $children = Get-Children $hostId
        foreach ($c in $children) { [void]$seen.Add($c) }
        Write-Host "[$Label] $state (window $shown, add-on processes: $($children.Count)): measuring $Seconds s"
        foreach ($r in Measure-Processes (@($hostId) + $children) $Seconds $window) {
            $who = if ($r.Name -eq $started.ProcessName) { 'host' } else { "add-on ($($r.Name))" }
            $over = '{0:N0}%' -f (100.0 * $script:cursorOver / [Math]::Max(1, $script:cursorSamples))
            $rows += [pscustomobject]@{ State = $state; Window = $shown; Who = $who; R = $r; Over = $over }
        }
    }
}
finally {
    foreach ($c in Get-Children $hostId) { [void]$seen.Add($c) }
    Stop-Process -Id $hostId -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 3
    foreach ($c in $seen) { Stop-Process -Id $c -Force -ErrorAction SilentlyContinue }
}

$machine = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
$lines = @(
    '',
    "### $Label ($(Get-Date -Format 'yyyy-MM-dd HH:mm'), $machine, $cores logical cores, ${WarmUp} s warm-up, ${Seconds} s per state)",
    '',
    '| State | Window | Process | CPU % of PC | CPU % of one core | Wake-ups/s | Working set MB | Private MB | Handles | Threads | Pointer over window |',
    '|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|'
)
foreach ($row in $rows) {
    $r = $row.R
    $lines += ('| {0} | {1} | {2} | {3:N3} | {4:N2} | {5:N1} | {6:N1} | {7:N1} | {8:N0} | {9:N0} | {10} |' -f
        $row.State, $row.Window, $row.Who, $r.CpuAll, $r.CpuCore, $r.WakeUps, $r.WsMb, $r.PrivateMb, $r.Handles, $r.Threads, $row.Over)
}
$text = $lines -join "`n"
Write-Output $text
if ($Out) { Add-Content -Path $Out -Value $text -Encoding utf8 }
