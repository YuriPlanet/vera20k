# Original IFV hit through bridge effects and Bullet retirement

The separate `ifv_bridge_impact.py/json` companion preserves
the earlier five-case launch and full-impact evidence unchanged. Independent
`--check` passes for three controls, each with 17 full original Bullet AI visits.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
VERA20K_BRIDGE_ANIM_ASSETS=/tmp/bridge-debris-input-assets/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_bridge_impact.py --check
```

Seed 31 gives native bridge admission draw 340 in [1,1500], rejecting damage 25.
Seed 39 gives 7 and admits the hit: the damaged control changes the anchor state
9 to 15 and neighboring states 9 to 13/14. The collapse control first primes
state 15 with original `HighBridgeBody(0x576BA0)` and then executes the same full
guided launch/flight/impact. This priming is not a preceding full weapon hit.

The collapse reaches four original `Fallout(0x47DD70)` calls, native rim and
perpendicular/setter/notification calls, graph disconnection `0x584E50`, and raw
connectivity rebuilding `0x56C510`. Original screen, radar and dirty-cell paths
execute. Retained outputs include all 225 supplied cell scalars, the active
bridge record, three graph levels, thirteen raw movement rows, radar coordinates
and dirty rectangle count. These are native outputs, not Rust expectations.

Runtime animation constructors publish TWLT026 ID95, DBRIS2LG ID96, TWLT036 ID97,
TWLT050 ID98 and TWLT070 ID99. Only after the bridge body returns does native
`SelectAnim(0x48A4F0)` observe the changed water state and select **H2O_EXP3**,
which receives ID100. The rejected and merely damaged controls select XGRYSML2.
The prior missing-water dependency is closed by executing original SplashList
constructor `0x6665D5..0x666604`, reader `0x66C184..0x66C287` and Wake reader
`0x66D847..0x66D894` on the physical layered INIs. All reached effect ART readers
execute with the frozen physical SHPs.

Full native AreaDamage, retirement, pending drain and Bullet destructor complete;
the pending/Bullet counts are zero afterward. The six collapse animations remain
retained. This companion deliberately stops before scheduling post-impact Anim
AI; its empty `anim_frames` is not an animation completion claim. The earlier
full-impact baseline separately executes the single XGRYSML2 lifetime to removal.

Supplied GameActive `0xA8E9A0=1` admits each original Anim Unlimbo. Native
coordinate commits and Display submissions execute, and retained effect locations
now equal their actual constructor placements; DBRIS2LG's Bounce pose is native.
The joined events decode selector XYZ separately: collapse retains Bullet Z104,
while AreaDamage and the H2O_EXP3 constructor receive Z520. Selection observes the
retained coordinate. This replaces the earlier temporary fixture's rejected Anim
admission and zero locations, while preserving the supplied Bullet admission.

Boundaries required to interpret this evidence:

- Physical stock fixture `tools/spatial_oracle/bridge_rim_stock_inputs.json`
  supplies a 225-cell crop translated by [-102,-120]. Map dimensions remain
  [136,140]. Native direct cell queries execute, but a native map load or valid
  translated map enumeration is not demonstrated.
- The retained hierarchy is explicitly supplied: three levels use zone 1 through
  y=20 and zone 2 after it; a single active high-bridge record connects [10,14]
  and [10,32]. Native vector constructors and initial edge publication execute.
  Base classifiers are class 0 inside the crop and class 7 outside. This is a
  bounded executable retained-state fixture, not a stock hierarchy export.
- Actual OverlayType constructors form the first 28 physical dense identities;
  full readers load BRIDGE2/GEM01. Their SHPs are absent and image IO returns
  missing. Native damage flags and land types are retained; overlay rendering
  is outside coverage.
- Native Map vector, Tactical dirty list and Radar constructor prefixes establish
  their containers. Full Tactical constructor executes with verified WINMM
  `timeGetTime` supplied as zero. Viewport 800x600, camera zero, original known
  projection constant and options GameSpeed 4 are fixture inputs.
- Source object/FLH, Bullet Unlimbo/Display admission, single-shot scheduling and
  partial type-pool construction are inherited supplied boundaries. Bullet ID94
  is that bounded setup's actual constructor identity, not whole-match startup.
  Each control independently seeds Scenario RNG. Whole Rules Process chronology
  and its pool/order are not claimed by this fixture.
- All original effect/math/RNG instructions execute. Shared transport boundaries
  supply physical archive bytes, allocation/free/TLS and verified OS atomics/RTTI
  probes. The script adapts only Python fixture setup statements in memory; it
  never patches original executable instructions.

The companion metadata pins the saved payload.
`--check` independently executes without writing; `--write` regenerates. Shared
harness imports use only repository files. Both asset roots may be relocated;
the input payload retains physical hashes. The stock crop path is repository
relative, so run from the checkout root.

## Rust composition status

The [current IFV ledger](ifv_launch.md) records passing 22 AreaDamage receipt
controls, 48 selector controls and the ordinary retail flight/collapse/restore
witness on the joined candidate. The final focused candidate also passes
`native_ifv_bridge_impact_orders_live_selection_debris_ids_and_rng` for all
3 controls (0.31 s). It compares all 225 cells, constructor order/placement,
Bounce values and Scenario RNG. The priming return is checked against the
independent `bridge_rim_body.json`: original state 9→15 returns 0. The test
supplies the final Bullet/impact handoff coordinates; it does not additionally
execute the full native launch/map-loading/retained-graph producer boundary in
Rust or schedule the post-impact animations.

[Six isolated-tail controls](ifv_isolated_tail.md) independently execute the
AreaDamage 0/1/2 to selector/WeaponNullifyAnim branch. Both original `--check`
and the final focused Rust composition pass all 6. In the EMEffect control,
selection consumes its Scenario draw before isolation replaces the selected
effect with IRONFX. This is separate from the physical retail HE bridge rows
above, where EMEffect is false. [Trail impact](ifv_trail_impact.md) separately
establishes physical Bullet destruction as the detach point. The [IFV ledger](ifv_launch.md)
records the passing full retail suite, release map load and visible save-restoration
checks, with their separate binary identities.
