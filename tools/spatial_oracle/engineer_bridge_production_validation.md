# Engineer object-hut production validation

The ordinary local Engineer object-hut path now uses full Map587410 geometry,
terminal Repair/NoRepair decisions, Infantry controllability and Capture ingress.
The existing Walk/PerCell2 repair owner remains responsible for rebuilding,
navigation publication and Engineer UnInit. Input only queries; the delayed event
does not repeat the producer's bridge-state decision.

The required Building Foundation dependency shares native474DA0 and the
461225..46125D Image-then-type-ID ART composition. ProcessedRulesLayers retains
the field across reached passes; RuleSet receives that result. ART metadata keeps
exact section identities and one derived folded visual lookup. RULES Foundation
does not supply geometry. Affected synthetic fixtures now declare explicit ART;
native goldens, assertions and the global replay baseline were not rebaselined.

The single independent critic found a competing Foundation writer in ART
installation. A focused regression reproduced the error: a constructor-only LATE
type changed from1x1 to4x4. That writer was removed; only reached rules bodies
execute461225. The regression covers reached, retained and constructor-only
types through installation, including a changed metadata cache. The80 native
rows also run through RuleSet projection, installation and the Undeploy consumer.

The saved receipt (`tools/spatial_oracle/engineer_bridge_production_validation.json`, in git at 1b1a8b43e) records the actual
checks, physical inputs, output hashes and preserved test/release identities at
commit `9f7bdb6cfdde18c58271732a7af71d9adccfd0d5` including main940 and the committed bridge candidate.
Both label manifests retain its complete source hash and Git status. This
documentation-only receipt was refreshed after final validation. The outputs are Rust observations,
not native goldens.

Validation on 2026-09-30 with retail INIs and `VERA20K_REQUIRE_RETAIL_INI=1`:

- Full library suite: 9,497 passed, 0 failed, 218 ignored, 64.69 seconds.
- Library clippy: exit0, 759 warnings. Simulation field ratchet: 2645/2645.
- Concrete/wood input witnesses: 2 passed; default simulation/snapshot witnesses:
  2 passed; GPU witnesses: 2 passed. Native caller and query `--check` both passed.
- Preserved release built with `--locked`; the normal loader opened stock AnyTown
  `XMP03T4.MAP`, advanced one 22ms diagnostic step and rendered an 800x600 hidden
  window. Map-observation v4 returned VALID, child exit0, no input violations.
  The receipt pins the consumed map bytes, executable, config, profile, contract,
  resident atlas, presentation clock, frame and final state.

Reproduce from the checkout with retail INIs/assets configured. All Cargo commands
use `python -m tools.cargo_run --`; all tests use `--lib`:

```sh
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- test -p vera20k --lib
VERA20K_REQUIRE_RETAIL_INI=1 python -m tools.cargo_run -- clippy -p vera20k --lib
python -m tools.spatial_oracle.engineer_bridge_cursor_caller --check
python -m tools.spatial_oracle.bridge_repair_query --check
python -m tools.cargo_run -- test -p vera20k --lib hut_input_reaches_engineer_entry_and_publication -- --ignored
python -m tools.cargo_run -- test -p vera20k --lib damage_repair_and_restore_publish_navigation -- --ignored
python -m tools.cargo_run -- test -p vera20k --lib ordinary_bridge_gpu_tests:: -- --ignored
```

For physical exports, set `VERA20K_BRIDGE_INPUT_EXPORT` to an existing directory;
GPU exports use `VERA20K_ANYTOWN_RENDER_OUTPUT` and
`VERA20K_SHRAPNEL_RENDER_OUTPUT`, with `RA2_DIR` pointing at retail assets.
The saved release profile is retained in the map-observation bundle identified by
the receipt. See [map observation](../map_observation.md) to run that sealed profile
with a freshly built preserved release label and a new exclusive output directory.

[Native caller evidence](engineer_bridge_cursor_caller.md) establishes bounded
action/click/display/control and Foundation results; [native query evidence](bridge_repair_query.md)
establishes full587410 on the supplied geometry corpus. Original selection ordering
rests on saved instruction evidence. Input witnesses compose production owners
from supplied target identity through queue/drain, Walk/PerCell2 repair and cleanup.
They exclude OS delivery, picking, finish_order presentation effects, native event
bytes and native whole-world cadence. GPU witnesses draw authored bridge rows
separately on clear targets; underlying terrain and original composited frames are
excluded. The release observation proves loader/render integration, not a click.

Cell-route behavior, native event transport/save import, omitted upstream selection
branches and the exhaustive whole-bridge audit remain open. This packet does not
certify the whole mechanism beyond its stated common-path coverage.
