# Amostra a RAM total (privada) do app + filhos a cada 2 s até aparecer "done" em marks.txt.
# Saída: %TEMP%\cyder-spike\ram.csv  (unix_ms,private_mb,processos)
param([string]$Name = "app", [int]$TimeoutSec = 900, [string]$Dir = "cyder-spike")

$dir = Join-Path $env:TEMP $Dir
New-Item -ItemType Directory -Force $dir | Out-Null
$csv = Join-Path $dir "ram.csv"
"unix_ms,private_mb,processes" | Set-Content -Encoding utf8 $csv
$end = (Get-Date).AddSeconds($TimeoutSec)

while ((Get-Date) -lt $end) {
    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, PrivatePageCount
    $ids = [System.Collections.Generic.HashSet[uint32]]::new()
    $queue = [System.Collections.Generic.Queue[uint32]]::new()
    foreach ($r in $all | Where-Object { $_.Name -eq "$Name.exe" }) { [void]$ids.Add($r.ProcessId); $queue.Enqueue($r.ProcessId) }
    while ($queue.Count) {
        $p = $queue.Dequeue()
        foreach ($c in $all | Where-Object { $_.ParentProcessId -eq $p }) { if ($ids.Add($c.ProcessId)) { $queue.Enqueue($c.ProcessId) } }
    }
    $sum = ($all | Where-Object { $ids.Contains($_.ProcessId) } | Measure-Object PrivatePageCount -Sum).Sum
    $ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    "$ms,$([math]::Round($sum / 1MB, 1)),$($ids.Count)" | Add-Content -Encoding utf8 $csv
    $marks = Join-Path $dir "marks.txt"
    if ((Test-Path $marks) -and (Select-String -Path $marks -Pattern " done$" -Quiet)) { break }
    Start-Sleep -Seconds 2
}
