param(
    [Parameter(Mandatory = $true)]
    [string] $Log,
    [switch] $AsJson
)

# Read-only reducer for the ignored Rust paired CPU probe's JSONL stdout.
# Values describe its declared hook bundle, NOT full-game frame time/FPS.
$ErrorActionPreference = 'Stop'
function Median([double[]] $Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { throw 'No samples' }
    $middle = [int][Math]::Floor($sorted.Count / 2)
    if (($sorted.Count % 2) -eq 0) {
        return ($sorted[$middle - 1] + $sorted[$middle]) / 2
    }
    return $sorted[$middle]
}

$records = @(Get-Content -LiteralPath $Log | ForEach-Object {
    $marker = $_.IndexOf('INTERPOLATION_PERF ')
    if ($marker -ge 0) { $_.Substring($marker + 19) | ConvertFrom-Json }
})
if ($records.Count -ne 160) { throw "Expected 160 complete probe records, got $($records.Count)" }
$summary = @($records | Group-Object { $_.case.name } | ForEach-Object {
    $group = @($_.Group)
    $off = @($group | Where-Object { -not $_.enabled })
    $on = @($group | Where-Object { $_.enabled })
    if ($off.Count -ne 8 -or $on.Count -ne 8) { throw "Incomplete pairs: $($_.Name)" }
    $deltas = @()
    $commitDeltas = @()
    $sampleDeltas = @()
    $p95Deltas = @()
    for ($pair = 0; $pair -lt 8; $pair++) {
        $a = @($off | Where-Object pair -EQ $pair)
        $b = @($on | Where-Object pair -EQ $pair)
        if ($a.Count -ne 1 -or $b.Count -ne 1 -or $a[0].final_hash -ne $b[0].final_hash) {
            throw "Missing or divergent pair: $($_.Name) / $pair"
        }
        $deltas += ($b[0].total.mean_ns - $a[0].total.mean_ns) / 1e6
        $commitDeltas += ($b[0].observe_commit.mean_ns - $a[0].observe_commit.mean_ns) / 1e6
        $sampleDeltas += ($b[0].sampling.mean_ns - $a[0].sampling.mean_ns) / 1e6
        $p95Deltas += ($b[0].total.p95_ns - $a[0].total.p95_ns) / 1e6
    }
    $case = $group[0].case
    $delta = Median $deltas
    [pscustomobject]@{
        case = $_.Name
        world = $case.world
        visible = $case.visible
        modeled_fps = 1000 / $case.render_ms
        modeled_tps = 1000 / $case.logic_ms
        bypass = $case.bypass
        point_samples = $group[0].point_samples_per_frame
        off_ms = (Median @($off | ForEach-Object { $_.total.mean_ns })) / 1e6
        on_ms = (Median @($on | ForEach-Object { $_.total.mean_ns })) / 1e6
        paired_delta_ms = $delta
        paired_delta_min_ms = ($deltas | Measure-Object -Minimum).Minimum
        paired_delta_max_ms = ($deltas | Measure-Object -Maximum).Maximum
        commit_capture_delta_ms = Median $commitDeltas
        sampling_delta_ms = Median $sampleDeltas
        off_p95_ms = (Median @($off | ForEach-Object { $_.total.p95_ns })) / 1e6
        on_p95_ms = (Median @($on | ForEach-Object { $_.total.p95_ns })) / 1e6
        paired_p95_delta_ms = Median $p95Deltas
        nominal_frame_budget_percent = $delta / $case.render_ms * 100
        mean_extra_cpu_ms_per_modeled_second = $delta * 1000 / $case.render_ms
        simulation_hash_equal = $true
        pairs = 8
        measured_frames_per_mode = ($off | Measure-Object -Property measured_frames -Sum).Sum
    }
})
if ($AsJson) {
    ConvertTo-Json -InputObject $summary -Depth 5
} else {
    $summary | Format-Table case, off_ms, on_ms, paired_delta_ms,
        commit_capture_delta_ms, on_p95_ms, nominal_frame_budget_percent -AutoSize
}
