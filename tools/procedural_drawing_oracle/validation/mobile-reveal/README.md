# Ordinary mobile first-reveal production comparison

This is Rust regression/output evidence, not a native whole-scene pixel golden.
Native class-entry admission evidence is in [entity_reveal](../../entity_reveal.md).
The [receipt](receipt.json) pins the executed Steam native identity, oracle checks,
release manifest, binary hashes, final library/Clippy logs, frame hashes and controls.

The retail XMP03T4 scenario moves MTNK1374 from (32,88) to (44,87), with seed
305419896, America versus Yuri, six starting units, ordinary shroud enabled and
FogOfWar disabled. The camera observes neutral LIMO1276 at (47,81). The ordinary
Move command, exact-step presentation clock, map/config/contract/profile bytes,
initial/final simulation fingerprints, complete observation transcripts, camera
and stock unit-atlas metadata match across each before/after pair.

| Control | Before/after result | Actual output |
|---|---|---|
| Step110, LIMO anchor unrevealed | Intentional frame mismatch only | 708 changed pixels, inclusive box (285,270)..(321,299); vehicle pixels now cross the frontier |
| Step111, LIMO anchor revealed | MATCH | Full BGRA frame identical |
| Step1, no move, shroud enabled | MATCH | Fully hidden vehicle remains hidden; full BGRA frame identical |
| Step1, no move, shroud disabled | MATCH | Fully visible frame identical |

Inspect [old110](mobile-110-before.png), [fixed110](mobile-110-after.png),
[fixed111](mobile-111-after.png), [hidden control](mobile-hidden-after.png) and
[clear control](mobile-clear-after.png). These are unchanged 800x600 GPU readbacks
from serial DX12 release runs on an RTX3050 Laptop GPU. All eight sealed bundles
validate through the existing map-observation comparer. A comparison's MATCH
means equal Rust output and checked inputs/state, not native parity.

The current release also replays the prior tree25, low-bridge243 and rock94
retail frontier witnesses. Each sealed run validates and its complete frame,
inputs, simulation fingerprints and observation transcript MATCH the already
repaired scenery release. The receipt records these three comparisons; their
unchanged images and native evidence remain with the
[scenery repair](../../../spatial_oracle/validation/scenery-reveal/README.md).

Reproduce a capture from the owned checkout with its retail config/assets:

```powershell
Remove-Item Env:RA2_DIR -ErrorAction SilentlyContinue
$env:WGPU_BACKEND='dx12'
python -B -m tools.map_observation --build-label mobile-shroud-reveal-20261007-v1 --profile E:/RA2/vera20k-building-reveal/tools/map_observation.mobile-reveal-110.example.json --contract E:/RA2/vera20k-building-reveal/src/app/diagnostics/tactical_capture/contract.v2.json --cwd E:/RA2/vera20k-building-reveal --output E:/RA2/vera20k-building-reveal/logs/mobile-reveal-replay-110
```

Use the corresponding111/hidden/clear profiles and fresh output directories.
The comparison binary is label scenery-shroud-reveal-20261007-v1, which already
contains the prior building/tree/bridge/rock corrections. Source changes are
limited to the shared mobile drawing admission owner; no simulation state, RNG,
timers, detach behavior or retail tuning keys change. Selection remains gated.

Coverage is one retail VXL vehicle at one frontier, plus three native class-entry
families and lifecycle controls. Aircraft/Infantry native admission is executed,
but their full GPU pixels, arbitrary slopes/heights, special cloak/warp/tint,
enabled-FogOfWar snapshots and20k-unit performance are not certified here.
The enabled-FogOfWar wave renderer prerequisite remains explicitly open in
[wave_admission](../../wave_admission.md).
The single [independent review](review.md) found no confirmed ordinary-material
blocker, and records an unconfirmed faint-outline risk for hidden special FX in
the existing compatibility composition path. Ordinary hidden controls do not
certify cloak/warp packed pixels; their shared native material port remains a
separate prerequisite for exhaustive visual claims.
