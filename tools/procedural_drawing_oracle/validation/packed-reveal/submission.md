# Historical local submission candidate

This report pins local candidate `760a6f75fe914b57d40fd964b1e56df3f041b22c`
before publication authorization and the later main integration. See the
[publication report](publication.md) for current results and submission scope.
At the time of this historical preparation, no issue comment, push, fork, PR
or merge had been published; publication and the AI declaration were pending.

## Scope and branch

`feature/shroud-first-reveal` starts from fetched upstream
`8cec184345ab38e23ddf6f039be9424416cb3730`. Five relevant commits were
cherry-picked from the preserved `feature/building-shroud-reveal` branch. The
old branch remains at `0849cd778622e725110ed867b9285f6fd4fde9e2`; its historical
checks and captures are retained in [after-critic.receipt.json](after-critic.receipt.json).
The [current receipt](submission.receipt.json) records this candidate separately.

The chain covers ordinary buildings, trees, rocks, bridges and mobile objects
being admitted before their anchor is explored. Existing shroud shading reveals
their pixels. Screen selection keeps its own discovery gate. The required
canonical visual-character query, live Rules stage input, packed destination
composition and VXL raw-cloak shadow gate stay with their existing owners.

Integration preserves upstream curtain PaletteLight/colour-word ownership,
the sinking-row shader ABI, Rocket locomotor state and Chronosphere state.
Snapshot schema 297 rejects incompatible older positional payloads. Translucent
tinted-leaf omission rests on instruction reading; combined tint/translucency
was not executed by the plain-leaf native corpus. Ordinary colour-word OR and
palette-table GPU checks were rerun against their unchanged upstream owner.

Excluded old commits cover Windows Cargo process waiting, startup atlas
optimization and a mapped-MIX deletion fixture. Only the standard wgpu 27
`WGPU_BACKEND` override and adapter logging are retained from the local runtime
setup. No retail INIs, extracted maps, assets, configuration or executable is
included in the proposed Git diff.

## Final validation

All tests use `--lib`, strict retail INIs and one Cargo job. The final environment
sets `VERA20K_REQUIRE_RETAIL_INI=1` and `RA2_DIR` to the official Steam folder.

- Full library suite: **9819 passed, 0 failed, 246 ignored**, 192.27 seconds.
- Clippy: exit 0, **710 warnings**; warnings remain visible in the saved log.
- Simulation field ratchet: **2466 versus 2470** at upstream base 8cec184345.
- Map-observation Python checks: **112 passed**.
- Packed GPU module: **3 passed each on Vulkan and DX12**, including native
  pixel/replay outputs in both sRGB formats, the 4K regression and workload.
- Ordinary palette tables and colour-word OR: **1 passed each on Vulkan**.
  These helpers use PRIMARY; no DX12 coverage is claimed for them.
- Release build: succeeded in 9m02s, label
  `shroud-first-reveal-submit-20261008-v1`. Exact binary, manifest and compiled
  source hashes are in the receipt; documents were added after source freeze.

The initial full run omitted `RA2_DIR`: 9817 passed and two native-table checks
failed. Both focused checks passed when the official Steam math tables were
loaded, followed by the successful full suite. No code was changed to suppress
these failures. Both failed and corrected logs remain pinned in the receipt.

## Release production observations

Five current-candidate DX12 release observations are **VALID**, and independent
offline validation of each sealed bundle is also VALID: mobile110, tree25,
bridge243, rock94 and stock SUB Guard90. These use the existing normal loader
and profiles, including Bases=true and UnitCount=6; no startup atlas workaround
or reduced-start-unit variant was needed. Each capture records the actual RTX
3050 Laptop adapter and Dx12 backend. The map source, exact step clocks, frame
hash, observation transcript and executable identity are pinned in the receipt.

The stock SUB fixture changes only the owner of the selected retail map unit;
its [original/authored map provenance](receipt.json) remains preserved. Stock
SubTorpedo has DecloakToFire=no. These are authored Rust integration scenarios,
not proof of native whole-scene or cloak lifecycle behavior.

Comparing each bundle with the old corrected candidate reports **MISMATCH**,
with no errors and exactly two differences: initial and final deterministic
state hashes. Full frame bytes, exact clocks and observation fields match.
Upstream changed hash inputs (including kamikaze, house superweapon state and
Rocket ownership); the comparison does not assert identical simulation across
different revisions. The raw MISMATCH results are retained without relaxing the
comparer, removing fields or claiming an unqualified MATCH.

On this machine, upstream's Windows process observer waited on an exited rustc
record with zero threads and handles. Validation used the preserved primary
checkout's existing runner module through
`PYTHONPATH=E:/RA2/vera20k python -P -m tools.cargo_run -- <cargo arguments>`.
The runner still owns the current checkout's shared build lock/cache. Its exact
path/hash are recorded; the workaround is excluded from this submission. No
other process was killed. Normal portable usage remains
`python -m tools.cargo_run -- <cargo arguments>`.

## Evidence and remaining work

Native identity is official Steam gamemd.exe SHA256
`3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600`.
The preserved [independent executable replay](../native-controlflow/translucent-replay-check.receipt.json)
covers 20 selectors, 6136 pixel cases, 54 ordered parent controls, 256 character
and 14 offset controls. Those are bounded native controls, not whole-scene
geometry or cloak clock proof. Native [source/shadow evidence](../source-dependencies/README.md)
and [control-flow evidence](../native-controlflow/README.md) remain separately
identified. The current Rust source still consumes those exact native goldens.

One fresh read-only packed critic reviewed the prior implementation and found
the 4K dispatch defect; the owner fixed it and replayed both backends. This
integration is owner validation after that single [review](review.md).

SHP companion shadows, split turret/body native source depth, nonzero
Building+6ED lifecycle, enabled FogOfWar Wave distortion and indirect Techno+24C
alias writers remain missing or unproved. Full-scene native captures and normal
20000-unit performance are also outside this proof. The exhaustive original
parity task remains open; the ordinary frontier uses FogOfWar=false.
