# Campaign maps in the arena baseline

`tools/autoplay_campaign.py` plays the campaign maps shipped inside
`mapsmd03.mix` (extracted here as `yr_campaigns/*.map`) through the same
`autoplay-match` harness as the arena baseline. It answers one question: how
does AutoPlay behave on this ground with these houses? It does not answer
whether a campaign stage can be completed.

## What a row is, and is not

`autoplay-match` loads a map through `headless_scenario::battle_session_with_seats`
and `ScenarioGameMode::Skirmish`, so a campaign map enters as terrain and
houses only: the stage's triggers, objectives, briefing and `[TeamTypes]` /
`[ScriptTypes]` do not run. A row therefore measures AutoPlay's opening,
economy, expansion and assault behaviour on campaign ground, never mission
progress. Two consequences a reader should keep in mind:

- The map's own `[Basic]`/`[Houses]`/pre-placed forces still load, so the local
  player may start from that stage's small starting force instead of the retail
  Battle opening. `opening_matches` is `False` on every campaign row by design:
  the manifest records `expected_opening = None`, and the constant is not
  retuned to make a campaign row pass.
- A `timeout` row is not a failure of the harness. On `all01umd.map` the local
  house starts with 7 infantry and the hard computer reaches 83 units, so the
  match runs the full 200,000-tick cap. That is the honest result for a mission
  terrain standing in for an arena.

## Manifest

`autoplay/benchmarks/campaigns.json` holds the 14 maps and their case seeds
(30000–30013), so campaign seeds never collide with the historical suite's
20000+. The local house is `america` for an `all*` map and `russia` for a
`sov*` map, with the opponent the other side; that is the run's fixed
assignment, not a claim about the mission's own houses. Each file's sha256 and
size are recorded next to the case list so a changed map is visible without
re-extracting the MIX.

Regenerate after re-extracting `mapsmd03.mix`:

```
python -m tools.autoplay_campaign manifest \
  --ra2-dir "I:/SteamLibrary/steamapps/common/Command & Conquer Red Alert II"
```

## Run and report

```
python -m tools.autoplay_campaign run --label V52-campaign \
  --run-dir target/autoplay-results/v52-campaign \
  --binary <path to autoplay-match.exe> \
  --ra2-dir "I:/SteamLibrary/steamapps/common/Command & Conquer Red Alert II" --jobs 14

python -m tools.autoplay_campaign report \
  --run-dir target/autoplay-results/v52-campaign [--before <earlier campaign run>]
```

`run` is resumable: receipts under `results/<seed>.json` are re-verified
against the run identity, so an interrupted run continues with the same
`--run-dir`. `report --before` refuses a comparison whose manifest, config,
retail inputs, jobs or benchmark runner hash differ.

## Relationship to the arena baseline

The per-case machinery is `tools/autoplay_benchmark.py`'s: `run_case`,
`load_receipts`, `export_csv` and the CSV columns are shared, and the campaign
tool imports them rather than copying them. That module's manifest and
`expected_opening` stay untouched, because the V52 acceptance flow pins its
hash; the campaign suite keeps its own manifest and its own identity fields,
plus `benchmark_runner_sha256` so a report says which arena runner produced the
cases.
