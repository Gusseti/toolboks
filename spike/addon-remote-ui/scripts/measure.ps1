# Usage: measure.ps1 -Name idle -HostArgs "--secs 32" [-Shot out.png]
param([string]$Name, [string]$HostArgs = "", [string]$Shot = "", [int]$Warm = 8, [int]$Window = 15)
$root = Split-Path $PSScriptRoot -Parent
$exeDir = Join-Path $root "target\release"
$log = Join-Path $root "scripts\out-$Name.log"
$total = $Warm + $Window + 6
$argl = "--addon `"$exeDir\spike-addon.exe`" --secs $total $HostArgs"
$p = Start-Process -FilePath "$exeDir\spike-host.exe" -ArgumentList $argl -RedirectStandardOutput $log -PassThru -WindowStyle Normal
Start-Sleep -Seconds 2
$line = (Get-Content $log | Where-Object { $_ -match "addon pid" } | Select-Object -First 1)
$apid = [int]($line -replace "[^0-9]", "")
Start-Sleep -Seconds ($Warm - 2)
if ($Shot) {
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  $b = New-Object System.Drawing.Bitmap 1600, 900
  $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen(0, 0, 0, 0, $b.Size); $b.Save($Shot)
}
$a0 = (Get-Process -Id $apid).TotalProcessorTime; $h0 = (Get-Process -Id $p.Id).TotalProcessorTime
$t0 = Get-Date
Start-Sleep -Seconds $Window
$a1p = Get-Process -Id $apid; $h1p = Get-Process -Id $p.Id; $a1 = $a1p.TotalProcessorTime; $h1 = $h1p.TotalProcessorTime
$dt = ((Get-Date) - $t0).TotalSeconds
function pct($x0, $x1) { [math]::Round(($x1 - $x0).TotalSeconds / $dt * 100, 2) }
$pws = $null; try { $c = (Get-Counter "\Process(spike-addon*)\Working Set - Private").CounterSamples | Select-Object -First 1; $pws = [math]::Round($c.CookedValue/1MB,1) } catch {}
# private working set via perf counter is slow to set up; use PrivateMemorySize64 (commit) and WorkingSet64
"RESULT $Name addon_pid=$apid addon_ws_mb=$([math]::Round($a1p.WorkingSet64/1MB,1)) addon_privws_mb=$pws addon_private_mb=$([math]::Round($a1p.PrivateMemorySize64/1MB,1)) addon_cpu_pct=$(pct $a0 $a1) host_ws_mb=$([math]::Round($h1p.WorkingSet64/1MB,1)) host_private_mb=$([math]::Round($h1p.PrivateMemorySize64/1MB,1)) host_cpu_pct=$(pct $h0 $h1) threads=$($a1p.Threads.Count) handles=$($a1p.HandleCount)"
$p.WaitForExit(20000) | Out-Null
Get-Content $log | Select-String "STATS" | Select-Object -Last 3
