# Publication candidate after main integration

The user authorized submission on 2026-10-08. This candidate will be submitted
as a Draft PR for the ordinary first-reveal chain associated with #1077.
Manual play and the contributor's AI explanation declaration remain pending;
no merge is requested. [Receipt](publication.receipt.json) pins actual results.

## Integration and source identity

Base: `39289f2dbb46c0273df5877c00d2f274d924c1d2`. Validation head: `1d19a5a5c00b11954fc26a9c096672517b0602d0`.
Upstream Lightning Storm state and radar-outage storage are preserved.
Upstream schema297 and the earlier local first-reveal297 describe different
layouts: the merged first-reveal candidate now uses schema298 and rejects old
incompatible snapshots. Existing canonical query, colour-word owner and packed
source/ordering logic remain; only this snapshot conflict required manual repair.
The [previous local candidate](submission.md) and its receipt remain historical.

Compiled src/Cargo fingerprint: `8cd42c58168a984c699188e2183d9c0d2b3a2c740b8a1353a260083b77dbd06b` (1411 files).
Release label: `shroud-first-reveal-publish-20261008-v1`; executable SHA256:
`39dee985af463a45e76c4ad3bf4c98187699418f9b8933ff3fad437ed70cf31d`. Docs/receipt edits follow frozen source
validation; they are outside the compiled-code fingerprint.

## Revalidation after the conflict

- Strict retail library suite: 9827 passed, 0 failed,
  247 ignored, 211.13 seconds.
- Strict retail Clippy: exit0, 708 warnings.
- Packed GPU module: all3 tests pass on each of DX12 and Vulkan, including
  native outputs, both sRGB formats, actual4K dispatch and synthetic workload.
- Field ratchet:2466 versus2470 at the new base.
- Fresh release build and five normal-loader observations: mobile110,
  tree25, bridge243, rock94 and stock SUB Guard90. Each sealed bundle and its
  independent offline validation is VALID: four ordinary scenes use DX12;
  SUB uses Vulkan as a backend control. Actual adapter/backend recorded;
  no reduced-start-unit or startup atlas workaround was used.
  SUB uses the declared authored-map owner-row fixture, retaining stock
  weapon/rules inputs; this is a Rust integration input, not a native golden.

Two current DX12 SUB runs were INVALID: wgpu Queue::write_texture reported
Out of Memory and each child timed out without a capture. A pre-main binary
control, previously successful on this input, now failed in the same way.
All three failed runs, error logs, hashes and the historical success identity
remain in the receipt. The current Vulkan control passes with identical
executable/config/contract/profile hashes; only backend and diagnostic
RUST_BACKTRACE differ. This does not resolve the DX12 failure or rule out a
problem in the original feature. Its underlying cause remains unproved, and
DX12 SUB production validation remains a Draft follow-up.

Cargo uses the preserved local primary runner and strict retail/Steam RA2_DIR
environment recorded in the receipt. Its Windows process-observer fix is
excluded from the PR. The earlier112 Python, ordinary Vulkan palette/colour
controls and native executable replay remain historical checks, not newly
repeated commands in this run. CPU/GPU synthetic timing is not20000-unit FPS.

The initial default incremental lib-test compile ran out of memory before tests
executed. Its failed log is preserved. Successful revalidation disables
incremental compilation (`CARGO_INCREMENTAL=0`) and uses supported test-profile
environment overrides (CARGO_PROFILE_TEST_CODEGEN_UNITS=64,
CARGO_PROFILE_TEST_DEBUG=0); inputs, assertions and full test scope are
unchanged. No global system setting or repository source was changed for this
retry. See [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html).

## Evidence and coverage

Original Steam gamemd identity and executed20 selectors/6136 pixel cases/
54 ordered-parent controls/256 character/14 offset controls are unchanged and
linked in the receipt. See [native control flow](../native-controlflow/README.md),
[source dependencies](../source-dependencies/README.md) and the
[single critic](review.md). This is owner revalidation after integrating main,
not another independent review.

The current five captures are Rust integration checks, not native whole-scene
goldens or evidence for all cloak lifecycle clocks. Earlier comparisons that
reported MISMATCH are preserved as such; no comparator fields are relaxed.
SHP companion shadows, split turret/body source depth, nonzero Building+6ED
lifecycle, enabled FogOfWar Wave distortion, indirect+24C aliases, combined
tint/translucency execution, whole-scene native frames and actual20000-unit
play remain missing or unproved. Ordinary frontier coverage uses FogOfWar=false.
These limits prevent a claim that all objects and mechanisms match native.

## Reproduce the production checks

Use the committed map-observation examples: mobile-reveal-110,
scenery-tree-25, scenery-bridge-243, scenery-rock-94 and packed-sub-guard.
The first four parsed profiles equal this run's sealed inputs. SUB differs only
by absolute versus checkout-relative selected_map_file; both resolve to the
same declared authored-map fixture. Follow the [packet fixture procedure](README.md#reproduce)
for its original retail hash and exact owner-row replacement. Configure the
Steam game folder locally; preserve bases=true, unit_count=6 and the seed.

Build the recorded source with the Cargo owner and a fresh release label, then
run the existing wrapper with WGPU_BACKEND=dx12 for ordinary scenes and
WGPU_BACKEND=vulkan for the SUB control, each example's --profile,
src/app/diagnostics/tactical_capture/contract.v2.json as --contract, this
checkout as --cwd, a fresh --output directory and that --build-label.
Run `python -m tools.map_observation validate --run <output>` independently.
The [map-observation reference](../../../map_observation.md) documents the
complete CLI and environment contract. Absolute local paths in the receipt
identify retained author evidence; adapt filesystem roots when reproducing.
