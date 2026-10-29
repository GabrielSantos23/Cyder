# Soma a RAM de um processo e de todos os filhos (WebView2 cria vários msedgewebview2.exe).
# Uso: .\measure-ram.ps1 -Name app        (ou -Name Spotify para comparar)
param([string]$Name = "app")

$all = Get-CimInstance Win32_Process
$roots = $all | Where-Object { $_.Name -eq "$Name.exe" }
if (-not $roots) { Write-Output "processo $Name.exe não encontrado"; exit 1 }

$ids = [System.Collections.Generic.HashSet[uint32]]::new()
$queue = [System.Collections.Generic.Queue[uint32]]::new()
foreach ($r in $roots) { [void]$ids.Add($r.ProcessId); $queue.Enqueue($r.ProcessId) }
while ($queue.Count) {
    $p = $queue.Dequeue()
    foreach ($c in $all | Where-Object { $_.ParentProcessId -eq $p }) {
        if ($ids.Add($c.ProcessId)) { $queue.Enqueue($c.ProcessId) }
    }
}

$rows = foreach ($id in $ids) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($p) {
        [pscustomobject]@{ Pid = $id; Name = $p.ProcessName; WorkingSetMB = [math]::Round($p.WorkingSet64 / 1MB, 1); PrivateMB = [math]::Round($p.PrivateMemorySize64 / 1MB, 1) }
    }
}
$rows | Sort-Object PrivateMB -Descending | Format-Table -AutoSize | Out-String | Write-Output
"TOTAL  WorkingSet: {0} MB   Private: {1} MB   ({2} processos)" -f `
    [math]::Round(($rows | Measure-Object WorkingSetMB -Sum).Sum, 1),
    [math]::Round(($rows | Measure-Object PrivateMB -Sum).Sum, 1), $rows.Count
