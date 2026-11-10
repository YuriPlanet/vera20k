# Temporary: prints what runs and what the logs say while the update test's
# second step runs, until step2.done appears.
$seen = @{}
function Show-New($path, $tag) {
  if (-not (Test-Path -LiteralPath $path)) { return }
  try {
    $fs = [System.IO.File]::Open($path, 'Open', 'Read', 'ReadWrite,Delete')
    $sr = New-Object System.IO.StreamReader($fs)
    $text = $sr.ReadToEnd(); $sr.Close(); $fs.Close()
  } catch { "  [$tag] cannot read: $_"; return }
  $lines = $text -split "`r?`n"
  $from = if ($seen.ContainsKey($path)) { $seen[$path] } else { 0 }
  if ($lines.Count -lt $from) { $from = 0 }
  for ($k = $from; $k -lt $lines.Count; $k++) { if ($lines[$k]) { "  [$tag] $($lines[$k])" } }
  $seen[$path] = [Math]::Max(0, $lines.Count - 1)
}
$names = 'vera20k|setup|\.tmp|python|bash|Runner|hosted|provjobd|explorer|Terminal|OpenConsole|powershell|pwsh|taskkill|shutdown|logoff|winlogon|LogonUI|userinit|wininit|msiexec|WerFault'
$last = ''
for ($i = 0; $i -lt 400; $i++) {
  $procs = (Get-CimInstance Win32_Process | Where-Object { $_.Name -match $names } | Sort-Object ProcessId |
    ForEach-Object { "$($_.ProcessId)<-$($_.ParentProcessId) $($_.Name)" }) -join ' | '
  "=== $(Get-Date -Format HH:mm:ss.fff)"
  if ($procs -ne $last) { "  procs: $procs"; $last = $procs }
  Get-ChildItem (Join-Path $env:LOCALAPPDATA 'VERA20k\update\*.log') -ErrorAction SilentlyContinue |
    ForEach-Object { Show-New $_.FullName 'setup' }
  Get-ChildItem (Join-Path $env:TEMP 'vera20k-update-test-*\Test Player\VERA\logs\ra2.log') -ErrorAction SilentlyContinue |
    ForEach-Object { Show-New $_.FullName 'game' }
  if (Test-Path step2.done) { break }
  Start-Sleep -Milliseconds 300
}
