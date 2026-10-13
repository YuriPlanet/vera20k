# Scenario ART ownership

RuleSet now owns the bound ArtRegistry throughout staged loading and gameplay.
Previously app/headless loaders held a second mutable registry and repeatedly
cloned it into RuleSet. Lazy authored-map constructors could bind one copy while
Start read another. The change removes that synchronization requirement; it does
not establish that a stock map previously produced different constructor effects.

## Ownership and ordering

NativeRulesProcessOwner remains the authority for type allocation and ART read
admission, with the process-fixed ARTMD.INI source. RuleSet owns the scenario's
bound projection used by simulation and presentation. These are distinct duties.
`install_art_data(ArtRegistry)` transfers ownership, applies the canonical read
receipt, projects object/terrain ART fields and rebuilds animation sequences.
The private registry has one immutable `art()` accessor and focused binding
operations; no production mutable registry accessor exists.

App authored/generated loading, the generated retail replica and headless loading
bind that same registry. The authored host extends the existing strict root
closure before consuming a native ID, then reborrows the same RuleSet for config,
Reveal and immediate Start. The now-redundant explicit ART override was removed
through load spawn, Reveal and display-layer selection. Presentation/HVA/cameo
loading derives ART from RuleSet instead of accepting a competing loader value.
Independent asset-browser and asset-diagnostic registries remain intentional.

The finalizer snapshots only immutable terrain/tiberium load parameters, once for
both Recalc sweeps. These local values are never published as runtime owners;
only ART changes during the scoped hosts. Houses receive the current RuleSet
through the callback. There is no empty-ART swap, full RuleSet/ART clone or runtime
interior-mutability mechanism. Existing strict-before-tolerant binding, eager
frame-dimension chronology, native-ID allocation, RNG draws and both sweeps are
preserved. Native projectile ART remains with its per-pass retained reader owner.

Fixture migration preserves three different contracts. Canonical fixtures consume
`install_art_data`; prior merge-then-raw fixtures use cfg(test)
`install_art_fixture`, sharing projection but bypassing native admission as before;
raw assignment fixtures use cfg(test) `replace_art_registry_for_test` without
adding projection. Frame-count and entry edits have narrow cfg(test) APIs. No
expected native golden values were changed to accommodate the migration.

## Native evidence and limits

Active retail gamemd.exe SHA256:
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Original instructions establish the ARTMD startup receiver at887180
(52D033..52D053), the live Anim read sweep679A5D..679A82 passing that receiver,
and the Anim427D00 read-admission gate. Constructor421EA0 stores one type pointer
at+C8, consumes identity/appends registry before optional RandomRate, and reaches
Unlimbo5F4EC0/Layer424CB0 before delay-zero Start424CE0. Layer and Start read that
same type pointer. Constructor defaults and427D22's failed-read exit explain why
late/unread types must not acquire orphan SHP bounds. These ordering/admission
claims rest on instruction/body/caller evidence, not a newly executed whole-game
ART load. The inspected physical loop bytes matched Ghidra's region; no full
Ghidra-export image hash was claimed.

The validation receipt (`tools/art_registry_owner.validation.json`, in git at 1b1a8b43e) records eleven
successful existing `--check` runs on 2026-09-28 (04:43:23–04:43:41 UTC), with
24 pinned ART/SHP inputs plus the retained sound inputs. The checked native producers and goldens were unchanged. The standalone Rust
`bridge_retail_anim_inputs.rs` exporter was migrated to the read-only accessor;
it is not a dependency of these native checks. Each row below states its independent coverage;
the checks do not collectively prove a joined full terrain-animation lifecycle.

| Golden checked | Recorded fixture counts | Boundary |
|---|---|---|
| `tools/rules_oracle/anim_image.json` | {'rows': 11} | Cached indexes supplied; excludes read admission, filename formatting and physical IO. |
| `tools/rules_oracle/bridge_anim_lists.json` | {'rows': 24} | Allocation/order/list truncation only; no ART reads. |
| `tools/rules_oracle/bridge_anim_inputs.json` | {'asymmetric_flag_controls': 2, 'production_export_checks': 2, 'rows': 24} | Full original constructor/ART/image readers with physical bytes at IO seam; no global read-sweep timing. |
| `tools/rules_oracle/bridge_child_sound.json` | {'anim_reports': 3, 'rows': 2} | Report/native sound fields/release-vs-stop; not full constructor, playback waveform/device or full RNG parity. |
| `tools/spatial_oracle/anim_layer_rules.json` | {'rows': 19} | Native Layer reader cases; not full constructor/sweep. |
| `tools/spatial_oracle/display_anim_owner.json` | {'rows': 24} | Display/attach/detach/expiry with actual vtables; initial object state supplied, no constructor admission. |
| `tools/spatial_oracle/anim_bouncer_launch.json` | {'ctor': 158, 'debris_loop': 9} | Full constructor/RandomRate/bounce arithmetic; Mark/Submit/Start are callbacks; Start recorded, not executed. |
| `tools/spatial_oracle/bridge_debris_producer.json` | {'death_loop': 12, 'explosion_pool': 4, 'metallic_pool': 15, 'retail_anim_types': 19, 'rows': 266, 'selected_metallic_indices': 15} | Fallout/constructors/immediate metallic Start with separately native-checked ART fields; Mark/Display seams; not full flight/render. |
| `tools/spatial_oracle/anim_middle.json` | {'rows': 298} | Middle/smudge placement/RNG; supplied frame dimensions/object virtuals/constructor seams; no joined Reveal/Start. |
| `tools/anim_oracle/boundary.json` | {'native': 86, 'bounds_columns': 5, 'bounds': 6, 'columns': 12, 'rows': 13312} | Downstream boundary/reset/continuation cases; no constructor/timer/full AI/ART loader. |
| `tools/spatial_oracle/terrain_recalc.json` | {'cases': 10} | Retained TMP/level/overlay states explicitly excluding terrain animations; terrain prerequisite only. |

All checks ran normal Python, not optimized Python. Exact commands, logs,
input SHA256 values and corpus identities are in the receipt. Reinspect the
original addresses above with the indexed [native inspector](native_inspect.md),
e.g. `python -m tools.native_inspect disasm 0x679A5D --bytes 39`.

## Pre-existing residuals

- Existing checked native corpora do not execute the complete live read sweep
  with same-pass allocations followed by late Techno allocations and another
  Rules Process pass. Rust admission tests remain useful, but their hardcoded
  1975/1070 trace counts/hashes lack a recovered indexed native producer.
- No checked joined native terrain Recalc -> constructor -> Reveal/Start ->
  scalar-delete/recreate corpus was found. The navigation oracle records waterfall
  constructor boundaries but supplies the constructor return; terrain_recalc
  deliberately excludes animations. Do not turn this ownership change into a
  claim of complete native animation parity.
- A tolerantly bound modded root with a missing chained SHP can fail a later
  strict authored binding. This prior failure policy is preserved.
- Native Start clears a prior report when the new Report is absent; the existing
  Rust sounding-to-silent Next transition can retain old sound. Fresh constructor
  ownership does not fix that independent transition residual ([#797](https://github.com/YuriPlanet/vera20k/issues/797)).


## Shared headless source selection

The retail waterfall regression exposed a prerequisite migration gap: headless
loading called `map_file::load_from_path` directly and could not find
`XMP03T4.MAP` inside the retail archives. It now uses
`map::source::load_map_by_name_or_path_with_assets`, the same loose/MIX source
owner as the app. Source priority and parsing stay in that owner; no independent
lookup or fallback policy was added. Dustbowl/Hills loose wrappers retain their
existing selection. This closes this headless map-source gap, not all load
sequencing duplication tracked in #593.

## Reproduction and validation

The synthetic
`sim::world::authored_load_host::tests::lazy_authored_binding_reaches_constructor_display_sound_and_snapshot_owner`
regression reaches the actual host with an initially unbound root, then checks
loaded bounds, native ID, display membership, start sound, RNG, config hash,
snapshot validation/restore, strict missing-SHP failure before ID consumption,
and a fresh reload. Its explicit Seed(0) and no Scenario draws allow full state
hash equality across the native-style snapshot RNG reset. It is an ownership
regression, not a hand-authored native golden.

Run the retail construction checks explicitly (normally ignored):

```sh
VERA20K_REQUIRE_RETAIL_INI=1 VERA20K_REQUIRE_RETAIL_ASSETS=1 \
python -m tools.cargo_run -- test -p vera20k --lib \
  headless_scenario::retail_construction_tests::retail_ -- --ignored --nocapture
```

Dustbowl checks unread D, repeated bound config hashes, construction and 30
headless steps. Anytown requires live terrain Anims and verifies their scheduler
membership, loaded SHP bounds, cell-drawer flag and display against the final
canonical ART. The checked run contains two surviving terrain Anims. Its native
navigation fixture observes constructor boundaries but substitutes their body;
that count is an observed Rust result, not a new native lifetime golden.

The eight-configuration generated-map retail check and the generated launch
lifecycle test are explicitly executed.

The final receipt also retains strict retail library/clippy results, explicitly
executed generated-map checks, labeled release builds, 300-step Hills/Dustbowl
headless comparisons and exact 30-step game captures for those maps and Anytown.
[Map observation](map_observation.md) owns capture validation and full-frame
comparison; no pixel masks or tolerances are used. These before/after results
establish bounded behavior preservation, not native rendering parity.

The independent critic rechecked the native corpora and retained comparisons,
finding no blocking defect. Its unused crane ART argument was removed along with
the adjacent unused finalizer frame flag; seven building-animation and 112 miner
checks passed after that cleanup. Incoming AI PR #796 merged cleanly. The final
combined candidate passed 9,612 library tests (215 ignored) and clippy, reproduced
all eleven native checks, and matched fresh paired current-main headless runs and
all three full-frame captures. See `latest_integrated_validation` in the receipt.
No second critic pass or whole-codebase completion is claimed.
