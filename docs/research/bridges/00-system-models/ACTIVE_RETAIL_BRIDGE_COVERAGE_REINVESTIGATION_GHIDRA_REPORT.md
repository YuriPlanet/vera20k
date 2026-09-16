# Active-Retail Bridge Coverage Re-investigation — Ghidra Report

**Date:** 2026-08-28
**Investigation mode:** coverage-map
**GSI ownership hypotheses:** GSI-04.12, GSI-04.13, GSI-04.14, GSI-04.15, plus every active cross-system consumer discovered below
**Native target:** active retail `gamemd.exe`, image base `0x00400000`
**Retail-data target:** Yuri's Revenge rules/art/theater data, shipped and loose retail map payloads, and the retail random-map setup path
**Secondary navigation source:** `C:\Users\enok\Documents\OpenTS` (lead generation only; never parity authority)
**Rust snapshot inspected:** `origin/main` at `0a6e6742`
**Confidence:** mixed by row; each row states its evidence and unresolved work
**Active in YR:** yes, with explicit content-conditional and evidence-backed exclusion boundaries below
**Coverage-map status:** **FROZEN after tenth-pass zero-add**; 27 mechanism rows and 31 explicit open questions

## 1. Verdict

The existing four GSI rows do not bound the bridge system. Active retail bridge behavior crosses map/RMG loading, terrain and overlay state, dual occupancy, navigation, locomotion, spawning and landing, projectile/effect collision, damage, collapse, repair, triggers, rendering, radar/audio, input, AI/order consumers, and deterministic persistence.

All four named rows remain open.

- **GSI-04.12 remains open:** the current Rust model has a substantial high-bridge foundation, but native entry conditions, exact A* and zone details, several locomotor transitions, spawn/landing/teleport ordering, projectile/effect bridge-plane collision, rendering details, and cross-system consumers are partial, missing, or unchecked.
- **GSI-04.13 remains open:** retail low/water bridges are flat Road overlays traversed by the ordinary ground path, not TubeClass paths. Native procedural endpoint expansion and active random-map low-deck/CABHUT generation are absent from Rust, and collapse/repair terrain mutation is incomplete.
- **GSI-04.14 remains open:** the dispatcher and major state machines exist, but ignored edge/restoration cases, debris/explosion details, Trigger Event 31, exact hut-selection branches, and several presentation/state ordering questions remain.
- **GSI-04.15 remains open:** TubeClass is a separate active executable mechanism. Explicit `[Tubes]` data is absent from the 385 scanned retail map payloads but is accepted by active retail map loading, so it is content-conditional rather than TS-only or compiled dormant. Rust has an explicit tube foundation, but currently invents non-native endpoints from automatic same-cell shells and lacks native hierarchy integration.

The most consequential corrections are:

1. The source and older documents' claim that the whole random-map generator is dormant is false. The retail random-map dialog creates `RandMap.Sed`, calls the generator, and map types 3/4 reach the low-bridge placement pass.
2. Low/water bridges and TubeClass tunnels are separate systems. Stock `LOBRDG*`/`LOBRDB*` cells finish as Road and return before the `LandType == 10` automatic tube-shell branch.
3. The RMG function named `BuildRiverBridge @ 0x0059E740` stamps waterfall iso-tile families and never writes low-bridge overlays or high-bridge flags. It is active map-generation terrain, but it is not a runtime bridge-topology mechanism.
4. `BulletClass__AI @ 0x004666E0` and `BounceClass__Update @ 0x00439B00` contain active bridge-plane collision logic not represented in the prior bridge gap synthesis.
5. `RepairBridgeOrRestoreRamp_Low @ 0x00570050` returns immediately when its primary 5x5 overlay scan finds a low bridge. Its pavement/flood-fill/level-restore tail is only the no-overlay fallback, contradicting an existing ignored Rust test's broader wording.

No bridge behavior may be implemented from OpenTS alone. Each OpenTS correspondence below is either paired with active-retail evidence or retained as an unresolved lead.

## 2. Authority and prior-report disposition

Authority order for this investigation is:

1. active retail `gamemd.exe` control flow and dataflow;
2. active Yuri's Revenge retail INI/assets/maps;
3. current Rust source and tests;
4. repository research documents, with later verified corrections taking precedence;
5. OpenTS as a function/mechanism locator only.

The prior bridge corpus is useful but not a closed scope. In particular:

- [BRIDGE_PARITY_GAP_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_PARITY_GAP_SYSTEM_MODEL_SYNTHESIS.md) is superseded as a completeness claim. It does not include the active RMG low-deck path, low-bridge/TubeClass split, projectile/bounce collision, or the full cross-consumer surface.
- [BRIDGE_PARITY_IMPLEMENTATION_CONTRACT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/BRIDGE_PARITY_IMPLEMENTATION_CONTRACT.md) remains a useful dated requirement set, but its residual list is not exhaustive and some source statuses have changed.
- [LOW_BRIDGE_TUBECLASS_DOC_VERIFICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/LOW_BRIDGE_TUBECLASS_DOC_VERIFICATION.md) correctly warned that constructor shells and parsed tubes are distinct; the new retail/overlay evidence now proves stock low bridges do not use either path for ordinary traversal.
- `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` statements that `ComputeBridgeZones` fills tube exits or that one TubeClass represents each low bridge are wrong.
- old rendering claims that normal under-deck occlusion is driven by `CellClass+0x10E` are stale; later reports prove the normal scanline/depth path and map-load literal values.
- old `TooBigToFitUnderBridge` movement-gate claims are stale; the verified active consumer is sprite/shadow split rendering.
- old paradrop “bridge target abort” claims are stale; active code attempts replacement and retains the original target when replacement fails.
- [WHAT_ACTION_BRIDGE_CELLS_CURSOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/07-cross-system-consumers/WHAT_ACTION_BRIDGE_CELLS_CURSOR_GHIDRA_REPORT.md) and [PIER_BRIDGE_WATER_CLASSIFICATION_RESWARM_20260527.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PIER_BRIDGE_WATER_CLASSIFICATION_RESWARM_20260527.md) are stale where they classify stock low Road overlays as TubeClass/tunnel cells.

## 3. Active-retail activation and data evidence

### 3.1 Retail random-map bridge path is active

- `RandomMapSetupDialog__Proc @ 0x00596300` is reachable from the retail skirmish setup and calls `RandomMapGenerator__Generate @ 0x00598960` from its generate/accept flow.
- `ChooseMap__AcceptRandomMapSetup @ 0x005E8590` writes/accepts `RandMap.Sed` and adds it to the selectable map list.
- `RandomMapGenerator__Generate @ 0x00598960` calls `RandomMapGenerator__BridgeAndConnectorPass @ 0x0058EF10` for map types 3 and 4.
- `RmgRegion__CarveConnectorsOrBridges @ 0x005905D0` sends water-class regions through `RandomMapGenerator__PlaceLowBridgeDeck @ 0x0058F2C0`.
- the retail installation contains `RandMap.Sed`; presence is corroborative, while the executable call chain is the activation proof.

Rust reaches its generator through `app/shell_random_map.rs` → `map/rmg/build.rs` → `map/rmg/pipeline.rs` → `map/rmg/emit.rs`. The active source comments describing the generator as dormant are therefore false for both retail gamemd and VERA.

### 3.2 Low/water bridge data

- retail `rulesmd.ini` defines `LOBRDG01..28`, `LOBRDGE1..4`, `LOBRDB01..28`, and `LOBRDGB1..4`.
- the normal body/end families use `Land=Road`; the ordinary entries also use `NoUseTileLandType=true`.
- `OverlayClass::Mark @ 0x005FC570` recognizes low-bridge endpoint ranges, stamps three-cell endpoint data, searches for the opposing endpoint, fills body rows with randomized variants, and recalculates every stamped cell.
- `CellClass::RecalcAttributes @ 0x0047D2B0` applies the overlay's Road land and returns through the Road/`NoUseTileLandType` path before automatic TubeClass creation.

The active traversal model is therefore a flat ordinary ground surface. It does not use elevated `OnBridge` occupancy and does not use TubeMovement.

### 3.3 TubeClass retail boundary

- `MapClass::ReadTubesINI @ 0x007283C0` is called from full scenario initialization and parses entry, direction, exit, path steps, and count.
- no `[Tubes]` section exists in the 385 scanned retail map payloads.
- `TubeClass::Constructor @ 0x00727FD0` also creates automatic same-cell, zero-step shells for qualifying final `LandType == 10` terrain.
- a same-cell shell cannot satisfy `ComputeBridgeZones`' strict current/exit order or safely enter a movement producer that divides by path length.

This makes explicit tube traversal **content-conditional/custom-map active** and automatic shell traversal **structurally excluded**. The automatic shell predicate itself remains in scope until the active YR theater tile corpus is classified.

## 4. Coverage ledger

Status meanings: **VERIFIED** is sufficient native/retail evidence for the stated requirement; **OPEN** means a material question remains and blocks implementation/pass; **EXCLUDED** has an evidence-backed active-retail boundary.

| ID | Mechanism / owner hypothesis | Active-retail evidence | Current Rust state | Status / next proof |
|---|---|---|---|---|
| BR-M01 | Rules, theater, overlay and asset inputs | `DestroyableBridges`, `BridgeStrength`, `BridgeExplosions`, `RepairBridgeSound`, `BridgeRepairHut`, all ten high bridge-piece keys, low overlays; theater loader reports | broad parsing exists but omits `BridgeBottomLeft1/2` and `BridgeBottomRight1/2`; exact C_SHADOW/RAILBRDG ownership still needs reconciliation | **OPEN** |
| BR-M02 | Active RMG low-deck/CABHUT emission | `0x0058EF10`, `0x005905D0`, `0x0058F2C0`, `0x005902C0`, `0x005904B0`; map type 3/4 retail UI call chain | `carve_driver` returns for water regions; `bridge_deck` only has seed/validators; no deck/end/CABHUT emission | **OPEN** — exhaustive placer/stamp/RNG contract and implementation |
| BR-M03 | RMG waterfall crossing named “bridge” | `BuildRiverBridge @ 0x0059E740` writes tile/subtile/level and RMG region tags, uses waterfall bases, never writes overlay `+0x44` or bridge flags | active implementation exists but has zero surviving characterization fixture | **EXCLUDED from runtime bridge topology**, retained under RMG terrain parity |
| BR-M04 | High-bridge map-load stamp and overlay-data order | OverlayPack high anchors call SetBridgeDirection; OverlayDataPack later overwrites `+0x11E`; exact local slots and dummy fallback verified | strong foundation exists | **OPEN** — modeled flag mask and remaining active consumers/writers |
| BR-M05 | Low-bridge procedural endpoint/body stamp | `OverlayClass::Mark @ 0x005FC570`; retail endpoint/body ranges and Road data | no equivalent endpoint expansion found | **OPEN** |
| BR-M06 | Raw cell facts and mutable terrain | independent flags `0x80/100/200/400/800/1000/10000/40000`, tile/subtile/level/overlay/state/anchor relations; raw `0x1000` also gates PixelFX sparkle emission | many facts exist, but live mask/comments, mutable projection and render-consumer binding are incomplete | **OPEN** |
| BR-M07 | High dual occupancy and persistent `OnBridge` | `+0xE4/+0xE8`, `+0x124/+0x128`, owner metadata; remove-old/write-state/add-new | production occupancy exists; dead duplicate shadow owner and unverified handoffs remain | **OPEN** |
| BR-M08 | Map spawn, Unlimbo, production, paradrop/landing | numeric `High`/`atoi`; `Unlimbo @ 0x005F5940`; parachute and jumpjet reports | map Units/Infantry partly covered; generic spawn, reveal/landing plane state incomplete | **OPEN** |
| BR-M09 | High runtime entry and A* | `CheckBridgeTraversal @ 0x004D9C60`, two-pass CanEnter, dual visited arrays, marker/flank costs, `UpdateBridgePassability @ 0x0042ACF0` and peer fallback `0x0042B080` | broad layered A* exists; skip predicate, structural-vs-walkable tests, exact peer-path marker overlay and hierarchy assertions remain | **OPEN** |
| BR-M10 | Bridge records, zones, redirects and hierarchy | `ComputeBridgeZones @ 0x0056D6E0`, `ResolvePathCoord @ 0x00583180`, zone helpers and hierarchy writers | high records exist; flattened/unchecked redirects and missing tube hierarchy pairs remain | **OPEN** |
| BR-M11 | Low Road traversal | Road/NoUseTileLandType early recalc and ordinary ground entry; low collapse changes overlays/land state | basic flat ground model exists | **OPEN** — procedural load, named retail intact/damaged/destroyed trace, mutation ordering |
| BR-M12 | Explicit tubes and direction-8 movement | `[Tubes]` parser, TubeClass, A* sentinel 8, Drive/Walk producers, Unit/Infantry consumers | explicit tube parser/movement exists; endpoint record semantics, costs, hierarchy and naming are wrong/partial | **OPEN** |
| BR-M13 | Locomotor height and crossing state | Drive/Walk own the ground transition writes; `FootClass::ShouldBeOnBridge @ 0x004DDC40` wraps destination/zone query `0x005F6A70`; Fly height setter `0x005F5FA0`; Ship, Hover, Jumpjet, landing and Teleport reports | transition foundation exists; destination reachability, exact constants, teleport/landing and several whitelists/latches remain | **OPEN** |
| BR-M14 | Projectile/effect bridge-plane collision | `BulletClass__AI @ 0x004666E0` checks current/previous structural cells and deck-plane crossing; `Inviso` initialization copies target `OnBridge` at `0x006FF0B0` | `projectile_collides_at`/`projectile_bridge_crossing` already implement the physical crossing and must be preserved; invisible-shot ownership often bypasses a persistent BulletClass and remains unaudited | **OPEN** |
| BR-M15 | Bounce/debris bridge-plane collision | `BounceClass__Update @ 0x00439B00` checks current/previous structural cells, deck top/underside and occupant plane; reached by active anim/effect paths | no complete Rust bridge-aware bounce owner identified | **OPEN** |
| BR-M16 | Direct/AoE/superweapon damage admission | `Apply_area_damage @ 0x00489280` tests the original impact cell only; state-machine bridge admission is strict `impact_z > ground+2` and inclusive `<= ground+5`, followed by tile-family selection; active SW impact-Z reports | dispatcher exists, but its Z gate is ground ±1 and its high/low discriminator is `deck_level >= 4`; cell force-fire and cross-consumer clauses remain incomplete | **OPEN** |
| BR-M17 | High/low damage and collapse state machines | high/low direct and state-machine families, walkers, zone invalidation, exact edge restamp; low EW bridgehead slots 0..2 call high ramp helpers | major state machines exist; low EW family dispatch and ignored edge/restamp/terrain cases remain wrong | **OPEN** |
| BR-M18 | Collapse fallout, objects, debris, RNG, triggers | `CellClass::BlowUpBridge @ 0x0047DD70` runs synchronously inside each destroy primitive; linked-list C4 damage, DropIn, DBRIS Bouncer debris/explosions, event `0x1F` | fallout is batched after walkers, ground kills bypass native damage/lifecycle, DBRIS is inert/misses a draw, and `notify_bridge_span_collapse` is a no-op | **OPEN** |
| BR-M19 | CABHUT C4, attached bomb, death, repair, cursor and outputs | hut timer/death plus `BombClass::Detonate @ 0x00438720`; engineer Y-major discriminator then X-major inner first-hit scan; overlay repair or no-overlay pavement/height/flood-fill fallback; cursor record geometry; observer/tag outputs, sound/EVA/radar | broad timer/repair foundation exists; attached-bomb entry, scan distinction, record branch, observer/event output and fallback restoration remain absent or approximate | **OPEN** |
| BR-M20 | High/low rendering, occlusion, shadows, railings, PixelFX, action lines and radar | terrain bundle before objects, body state/depth, C_SHADOW tables, TooBig split and adjacent-deck probe `0x00703E70`, raw-`0x1000` sparkle suppression, selected/enemy action-line endpoint bridge Z `0x004DC060`/`0x004DC340`, radar colors/dirties | broad rendering exists; sparkle gate incorrectly uses `bridge_walkable`; selected lines use generalized deck state instead of raw `0x100`, Psychic Sensor enemy lines are absent, and railing/alpha/split/effect ordering remain open | **OPEN** |
| BR-M21 | Targeting, orders, placement, AI, triggers and superweapon consumers | tactical inverse `0x006D6590`; generic target layer gate `0x006F7CA0`; Mirage bridge-plane neighbor scan `0x007465B0`; Parasite/Spawner/arcing gates; MCV refusal; factory/unload/undeploy relayers; rally Z `0x006DA9D0`; trigger helpers under `0x006DD8B0`; paradrop/landing/cursor; no proactive bridge AI found | tactical inverse search exists but its exhausted-search fallback is wrongly clamped; generic target filtering and Mirage scan are missing; remaining owners are scattered/partial or ignored | **OPEN** |
| BR-M22 | Persistence, checksum and deterministic rebuild | native saves overlay `+0x44`/data `+0x11E` and rebuilds flags/pointers/zones; Tube CRC `0x00728630` feeds endpoints, direction, all 100 path words and length; Scenario RNG save/load behavior differs from MapGen/Main | Rust serializes/hashes a broader derived bridge graph; exact native pack reconstruction, RNG projection, new effect state and replay ordering are not closed | **OPEN** |
| BR-M23 | Occupant scatter, crusher dispatch and stuck-unit safety | `CellClass::Scatter_Objects @ 0x00481670` selects `+0xE4/+0xE8`, snapshots up to ten objects in linked-list order and dispatches Scatter; `UnitClass::Scatter @ 0x00743A50` carries `OnBridge` into nearby-cell search; `UnitClass::PerCellProcess @ 0x00739EC0` exempts correctly layered structural-deck units from blocked-cell self-damage | selected-layer bump/crush support is partial; periodic scatter uses ground-only occupancy, blocker scatter substitutes direct eight-neighbor motion, and blocked movement has no native stuck-damage transaction/exemption | **OPEN** |
| BR-M24 | Resource/terrain placement bridge-mask consumers | `CellClass::CanPlaceTiberium @ 0x004838E0` rejects raw `0x500` and is reached by active `TIBTRE01..03` `SpawnsTiberium=yes` terrain AI | `resolved_cell_accepts_tiberium` already rejects structural `0x100` and destroyed/inactive `0x400`; preserve and validate its raw-fact binding | **OPEN** — exact consumer validation/critic pass, plus remaining raw-mask consumers under OQ-01 |
| BR-M25 | Post-A* bridge-height-aware path smoothing | successful `AStar_main_loop @ 0x00429A90` always calls corner smoothing `0x0042B210` then straight-segment optimization `0x0042B7F0`; validators `0x0042B420`/`0x0042BE20` carry effective level and retain `cell level + 4` only across the exact structural-deck transition | live Rust `smooth_path` receives only boolean walkability, carries no effective Z/structural fact, and omits the second native optimization pass | **OPEN** |
| BR-M26 | Bridge-selected AoE Rocker secondary effect | `Apply_area_damage @ 0x00489280` selects deck `+0xE8` or ground `+0xE4`; active `Warhead+0x14E Rocker` reuses that same plane throughout its 7x7 scan and virtual rocking dispatch; retail `V3WH` has `Rocker=yes` | ordinary Rust splash preserves a selected plane, but production never consumes `warhead.rocker`; `apply_rocker_impulse` is test-only | **OPEN** |
| BR-M27 | `CellSpread` bridge-object tolerance filter | `Apply_area_damage @ 0x00489280` enables tolerance mode when `CellSpread > 0.5`; a qualifying non-limbo bridge/above-ground object within strict distance `< 0x55` sets the local flag used by final bridge/`OnBridge` candidate suppression; retail `V3WH`, `V3EWH` and `DMISLWH` activate it | selected-plane Rust splash exists but has no `CellSpread > 0.5`, `<0x55` tolerance flag or final suppression | **OPEN** |

## 5. Verified mechanism requirements

### 5.1 High-bridge facts and occupancy

- Preserve raw bridge facts independently. Do not replace the flag word with one `has_bridge` boolean.
- `0x100` is structural deck coverage; `0x200` is a transition/bridgehead subset; `0x400` is the destroyed/inactive endpoint-fallback state; `0x800` orientation polarity is set for the direction-0/N-S family; `0x40000` is a transient path-cost marker.
- cell effective height and object `OnBridge` state are distinct contracts.
- object list selection uses persistent `OnBridge`; occupation-bit selection uses the native height/structural predicates and is intentionally asymmetric between mark and clear.
- normal crossing order is remove using old state, move/update coordinates, compute exact new bridge state and height, then add using the new state.
- do not use `FootClass::ShouldBeOnBridge @ 0x004DDC40` as the transition writer. It queries the NavCom destination (or explicit tube exit), applies the Foot `+0x684` gate, and feeds zone/reachability and per-cell trigger consumers. Drive/Walk own the verified ground-movement writes to `OnBridge`.
- active Fly height updates call `FootClass::Set_Height_On_Bridge @ 0x005F5FA0`, which adds the independently initialized 416-lepton deck offset when `OnBridge` is set and brackets a marked object's Z write with remove/put.

### 5.2 Low/water bridges

- Low bridges remain at ground level and use Road passability. Do not route them through high-bridge occupancy, bridge height, or TubeMovement.
- Map loading must reproduce the endpoint-to-endpoint procedural overlay expansion and its random body variant order before final cell recalculation.
- Damage and repair mutate the three-cell overlay strip and recalculate it; final destruction, not first damage, rebuilds connectivity.

### 5.3 TubeClass

- Keep cell tube index, final land type, explicit TubeClass data, and bridge overlay identity as separate facts.
- `IsTubeCell` requires a valid tube index plus `LandType == 10`; `GetTube` only bounds-checks the index.
- parsed tubes own entry, exit, direction, path buffer and path length.
- constructor-created automatic shells have entry==exit and length zero; do not synthesize a long endpoint by joining adjacent shells.
- direction 8 bypasses ordinary neighbor movement and uses the stored exit; zero-length shells must never enter visible TubeMovement.
- low `BridgeRecordKind` naming in Rust must be changed to a tube/tunnel meaning.

### 5.4 Damage and repair

- preserve the four dispatcher paths and their call/RNG order.
- outer `DestroyableBridges` and warhead gates precede BridgeStrength admission; Ion behavior is a special admission case, not a different mutation machine.
- the state-machine admission gate tests only the original `Apply_area_damage` impact cell. It requires `impact_z > ground+2` and `impact_z <= ground+5`, then chooses high/low through native tile-family predicates rather than inferred deck height.
- collapse fallout is synchronous inside each per-cell destroy primitive. Native linked-list order applies force-kill `ReceiveDamage` with the C4 warhead to ground occupants, relayers deck occupants with DropIn, enqueues/updates the cell, and performs optional explosion/debris work before the next bridge damage gate. Do not batch it after all walkers.
- retail `MetallicDebris` resolves to active `DBRIS*` Bouncer AnimTypes. Construction still consumes the `RandomRanged(1,1)` RandomRate draw; bounce physics later applies each type's damage/radius/warhead and expire animation.
- low repair's ordinary primary match returns after low overlay repair. The pavement/flood-fill/level tail is the no-overlay fallback only.
- engineer repair selection is X-major then Y-minor over the 5x5 square and stops on the first matching branch. The no-overlay path restores pavement/tile/level state, runs flood-fill/validation work, and can recurse into adjacent repair handling.
- `NotifyBridgeSpanCollapse @ 0x00575EE0` conditionally delivers numeric event `0x1F` only to tagged cells over its normalized four-cells-per-step, endpoint-exclusive span footprint. It is distinct from the `FootClass::PerCellProcess` event `0x18` reachability path. Never broadcast it globally over Rust's destroyed-cell set.
- `Apply_area_damage @ 0x00489280` keeps its initial deck/ground occupant-list selection for the active `Rocker` secondary-effect branch. Its 7x7 scan must rock only occupants from the chosen bridge plane; it is not a plane-agnostic visual embellishment and retail `V3WH` activates it.
- `Apply_area_damage` also owns a separate `CellSpread > 0.5` tolerance mode. Qualifying non-limbo bridge/above-ground objects at strict distance `< 0x55` set a local flag that participates in final bridge/`OnBridge` damage suppression. Do not conflate this with selected-plane enumeration or Rocker; Rocker-negative retail `DMISLWH` activates the tolerance path.

### 5.5 Rendering and physical collision

- high bridge terrain body/shadow/railing draw before objects, but correct under/over ordering also depends on object Z/depth and split-blit rules.
- the recovered railing emitter tables are tied to `C_SHADOW.SHP`; a separate `RAILBRDG` visual owner must not be substituted without native proof.
- `TooBigToFitUnderBridge` is a render split/depth consumer, not a movement blocker.
- bullets and bounce/effect objects collide with the bridge plane when the old or new cell is structural and the step crosses the deck Z. Upward hits use the underside adjustment where native does.

### 5.6 Occupant reactions and stuck-unit safety

- `CellClass::Scatter_Objects @ 0x00481670` chooses the ground or deck linked list from the requested bridge plane, snapshots no more than ten occupants in linked-list order, and only then dispatches each object's Scatter behavior. Do not merge this with a ground-only periodic convenience pass.
- `UnitClass::Scatter @ 0x00743A50` forwards the unit's persistent `OnBridge` state into `Find_Nearby_Passable_Cell`; its directional candidate branch rejects structural `0x100` cells. Preserve per-class/native search semantics rather than substituting a direct eight-neighbor move.
- `UnitClass::PerCellProcess @ 0x00739EC0` suppresses its stopped-and-blocked explosion/C4 self-damage path when the unit is `OnBridge` and the current cell is structural, unless the separate sinking branch applies. The exemption is an active safety consumer of both facts, not permission to omit the surrounding stuck transaction.

### 5.7 Target acquisition and tactical inverse fallback

- `TechnoClass::Evaluate_Candidate @ 0x006F7CA0` resolves both cells and rejects a candidate for bridge-layer mismatch only when both cells carry structural bit `0x100` and attacker/target `OnBridge` differs. Its ordinary `Greatest_Threat` and `Scan_Cell_For_Target` callers make this a common acquisition rule, distinct from later weapon-specific Parasite, Spawner and arcing fire checks.
- `TacticalClass` inverse `0x006D6590` keeps the existing orientation/cardinal search, strict threshold and 180-attempt cap. If all attempts fail, it returns the original packed coordinate with signed-short components; negative/off-map values are not clamped to zero at this boundary.

### 5.8 Resource placement and bridge-adjusted action lines

- `CellClass::CanPlaceTiberium @ 0x004838E0` refuses cells with either raw structural `0x100` or destroyed/inactive `0x400`. Retail `TIBTRE01..03` terrain AI actively reaches it through `SpawnsTiberium=yes`. Preserve Rust's existing exact `0x500` refusal and do not substitute current walkability or generalized deck state.
- selected action lines `TechnoClass::DrawActionLines @ 0x004DC060` and Psychic Sensor enemy lines `TechnoClass::DrawRadarActionLines @ 0x004DC340` set movement endpoint Z to ground height plus the bridge offset when the endpoint cell has raw structural bit `0x100`. This presentation correction remains active after tactical target selection and is distinct from factory rally-line Z.

### 5.9 Post-A* bridge-height-aware smoothing

- every successful `AStar_main_loop @ 0x00429A90` result runs corner smoothing and then straight-segment optimization. Both validators carry the previous effective level into their cell-entry probes.
- after a candidate step, the effective level remains `cell level + 4` only when `previous effective level - cell level == 4` and raw structural bit `0x100` is set; otherwise it drops to the cell's base level. This prevents either optimization pass from accepting a geometric shortcut on the wrong bridge plane.
- smoothing is downstream of A* success but still part of movement correctness. A boolean-only visibility/walkability closure cannot represent the native contract, and omitting the second pass is not a harmless performance difference.

## 6. Open Questions Log

Every unresolved item below keeps its mechanism open. “Different system context” is not a parity waiver for this goal; such items are scheduled into the owning mechanism before implementation or pass.

1. **OQ-01 — complete consumer census:** Which OpenTS bridge readers/writers still exist in active YR after bullet, bounce, particle, wave, deformation, placement, AI/team, trigger, superweapon and tactical leads are checked? **OPEN.**
2. **OQ-02 — runtime constants:** What exact initialized values/writers feed every Drive/Ship/Walk/Hover/Jumpjet/Teleport bridge Z threshold and render/debris constant? **OPEN.**
3. **OQ-03 — automatic tubes:** Which active YR theater TMP cells finish as `LandType == 10`, and which ordinary retail flow, if any, observes the automatic shell? **OPEN.**
4. **OQ-04 — RMG low placer:** Resolve every predicate, tile-base identity, rectangle, overlay byte, CABHUT position, attempt/rejection, RNG site and emitted section order in `0x0058F2C0`. **OPEN.**
5. **OQ-05 — low overlay Mark RNG:** Resolve exact endpoint scan order, opposite-end termination and body-variant RNG order at `0x005FC570`. **OPEN.**
6. **OQ-06 — traversal call placement:** Does active Unit/Infantry entry call `CheckBridgeTraversal` unconditionally at every ordinary step, and exactly which Rust skip predicate is non-native? **OPEN.**
7. **OQ-07 — A* tie and hierarchy:** Close cost/tie insertion order, hierarchy bridge exemption and the concrete stock trigger for two-pass list/bit divergence. **OPEN.**
8. **OQ-08 — tube hierarchy pairs:** Close exact entry/exit/path-step pair registration and ordering in `MapClass::RegisterBridgeOrTubeHierarchyPairs @ 0x00582D70`. **OPEN.**
9. **OQ-09 — path marker peers:** `PathfinderClass::UpdateBridgePassability @ 0x0042ACF0` is active in Drive/Walk A*, uses facing-selected ground/deck peers, replays Unit/Infantry queued paths including direction 8, toggles `0x40000`, scans a 5x5 occupation square, and applies the 4x destination cost. Exact Rust delta, duplicate-visit parity, urgency state and tube-invalid edge fixtures remain **OPEN.**
10. **OQ-10 — `ShouldBeOnBridge`:** The Foot override is a gated destination/zone-reachability query, not an `OnBridge` writer. Verified consumers include locomotion AI, Walk movement recheck, and per-cell Event `0x18` processing. Exact `Can_Reach_Zone` inputs, remaining callers and Rust trigger/zone delta remain **OPEN.**
11. **OQ-11 — locomotor reachability:** Prove exact stock activation for Hover/Ship tube entry, Hover Push/Shove, Jumpjet states 4/5, and each air/landing bridge branch. **OPEN.**
12. **OQ-12 — teleport/landing state:** Close final Z, OnBridge, object-list, occupation-byte and render-layer write order for successful and refused placement. **OPEN.**
13. **OQ-13 — projectile/effect families:** Physical BulletClass bridge-plane crossing is already present in Rust and must be preserved. DBRIS/Anim Bouncer activation, Parasite cross-deck FireError `0x006FCCAE`, Spawner bridge-adjacency gate `0x006FC612`, and arcing target-cell ceiling `0x006F74D7` are active. Close `Inviso` target-layer propagation plus remaining homing, particle, wave, anim and top/underside variants against stock bindings. **OPEN.**
14. **OQ-14 — edge restamp:** High `0x00576200` and low `0x00570AE0` exact tile-family/flag-transition walks, backward SetBridgeDirection, overlay clear, event, radar and recursion are active; replace the Rust 8-neighbor/orphan heuristic with mechanism-specific contracts and fixtures. Exact per-mode mutation ledger remains **OPEN.**
15. **OQ-15 — collapse debris:** Stock DBRIS Bouncer animation activation, RandomRate 1..1 draw and damage/radius/HE ownership are proved. Close exact construction/AI/bounce-result ordering, nonmetal/voxel branches, RNG instance and animation-delay integration in Rust. **OPEN.**
16. **OQ-16 — repair selection/restoration:** The outer low/high discriminator is Y-major, but each repair entry's first-hit 5x5 scan is X-major; Rust incorrectly reuses one Y-major scan. The low primary-return/no-overlay-fallback split is proved. Close overlay-vs-record precedence, high fallback terms, exact pavement/flood-fill/tile/level mutation, native Logic-vector multi-engineer ordering and dirty/zone ordering. **OPEN.**
17. **OQ-17 — rendering assets:** Reconcile `C_SHADOW.SHP`, any independent `RAILBRDG` use, shadow encoding, split-blit gates, effect layer and scanline ordering. **OPEN.**
18. **OQ-18 — cross-system consumers:** The tactical inverse, generic target layer gate, Mirage neighbor scan, MCV refusal, rally-line Z, generic factory/unload/undeploy handoffs and trigger action helpers are active; AI has no proactive bridge opcode/priority. Preserve the implemented 180-attempt orientation-aware inverse search but replace its zero-clamped failure result with the native original packed signed coordinate; add the missing `Evaluate_Candidate` and Mirage plane selection. Verify crates, remaining transport/passenger paths, deformation, scripted waypoint effects and superweapon effect-Z leads, and bind each active path to the shared layer authority. **OPEN.**
19. **OQ-19 — persistence:** Native overlay/data persistence, Tube save/load/CRC and derived rebuild are bounded, while Rust snapshots/hash a broader graph. Enumerate object/cell/hut/effect fields, exact pack reconstruction, Scenario-vs-MapGen/Main RNG behavior and reconcile conflicting liveness claims for `FootClass::ComputeChecksum @ 0x004DBAD0`. **OPEN.**
20. **OQ-20 — retail fixtures:** Named movement fixtures are selected: Lost Lake and Killer for intact low crossings; Bay of Pigs and Hills for intact high deck/under-span/dual-plane/AttackMove cases; Deadman's Ridge for a high collapse gap; Shrapnel Mountain for a destroyed low bridge. Collapse/repair/CABHUT state transitions, active RMG output, explicit custom `[Tubes]`, DBRIS bounce and trigger-event fixtures still need sealed inputs/actions. **PARTIALLY RESOLVED; OPEN.**
21. **OQ-21 — GSI-04.15 activity label:** Is any automatic shell reached by shipped retail content? Explicit `[Tubes]` is already classified content-conditional, not dormant. **PARTIALLY RESOLVED; tile-corpus part OPEN.**
22. **OQ-22 — RMG “river bridge” ownership:** Does `0x0059E740` ever lead indirectly to runtime bridge overlay/flag creation? Direct subtree writes and callees show no. **RESOLVED: no; topology exclusion.**
23. **OQ-23 — low repair tail:** Does every successful low repair execute pavement/level restore? **RESOLVED: no; only the no-overlay fallback tail does.**
24. **OQ-24 — TubeClass low bridge ownership:** Do stock low overlays create or consume tubes? **RESOLVED: no; Road early return and separate predicates prove the split.**
25. **OQ-25 — scatter/crusher and stuck dispatch:** Close every active `CellClass::Scatter_Objects`, crusher/per-cell and locomotor caller, the ten-object snapshot/order, per-class Scatter/FNPC arguments, directional structural-cell refusal, and the stopped-blocked damage/repath ordering. Bind periodic and blocker-triggered Rust paths to the same selected-layer authority without inventing extra neighbor motion. **OPEN.**
26. **OQ-26 — target acquisition layer gate:** Close the full return/control-flow ordering around `TechnoClass::Evaluate_Candidate @ 0x006F7CA0`, all ordinary acquisition callers and weapon overrides, then prove fixtures for deck-vs-under-span refusal and either-cell-nonstructural admission. **OPEN.**
27. **OQ-27 — raw-mask terrain/resource consumers:** TIBTRE ore placement's exact `0x500` refusal is present in Rust and must be preserved. Exhaust the remaining active retail vegetation, ore/gem spread/growth and map-mutation callers for structural/destroyed bridge-mask reads so the positive match is not mistaken for complete consumer coverage. **OPEN.**
28. **OQ-28 — action-line endpoint height:** Close the selected and Psychic Sensor enemy action-line activation gates, movement-vs-attack endpoint choice, endpoint raw-`0x100` test, exact bridge Z offset, collapsed/repair transitions and Rust render-command ownership. **OPEN.**
29. **OQ-29 — post-A* smoothing:** Close both passes' exact iteration, corner/segment candidate order, effective-level transition, `Can_Enter_Cell` arguments, mutation/tie behavior and endpoint handling. Prove deck, under-span, ramp, collapse-gap and no-bridge negative fixtures against live movement callers. **OPEN.**
30. **OQ-30 — Rocker secondary effect:** Close the `Rocker` admission/order relative to ordinary splash, exact 7x7 enumeration, distance/impulse calculation, selected-plane linked-list traversal, virtual dispatch and deterministic effect state. Prove V3WH deck-vs-under-span fixtures plus a `Rocker=no` negative case. **OPEN.**
31. **OQ-31 — CellSpread bridge tolerance:** Close the qualifying-object class/height/limbo predicates, strict `<0x55` distance basis, each repeated flag writer in the CellSpread scan, final bridge/`OnBridge` suppression predicate and ordering relative to Verses, damage, Rocker and state-machine bridge admission. Prove V3WH/V3EWH and Rocker-negative DMISLWH fixtures. **OPEN.**

## 7. OpenTS correspondence ledger

These are navigation leads only. “Verified” here means a corresponding active-retail native mechanism has independently been found; it does not bless OpenTS code as an implementation source.

| OpenTS lead | Native disposition |
|---|---|
| `code/overlay.cpp` low endpoint stamping | independently verified at `OverlayClass::Mark @ 0x005FC570` |
| `code/astar.cpp`, `cell.cpp`, `drive.cpp`, `walk.cpp`, `hover.cpp` | high dual-layer and explicit tube mechanisms independently verified; exact residuals remain open |
| `code/map.cpp` damage/repair/zone families | independently verified corresponding gamemd families; OpenTS names help navigate only |
| `code/bullet.cpp` bridge-plane crossing | independently verified in `BulletClass__AI @ 0x004666E0`; Rust physical crossing already matches at the known owner, while Inviso ownership remains open |
| `code/bounce.cpp` bridge-plane crossing | independently verified in `BounceClass__Update @ 0x00439B00` |
| `code/mapgen.cpp` water-region bridge placement | independently verified at `0x005905D0`/`0x0058F2C0` |
| `code/tube.cpp`/`tube.h` | independently verified parser, constructor and movement consumers |
| `code/particle.cpp`, `partsys.cpp`, `wave.cpp`, `anim.cpp` | active DBRIS Anim/Bouncer path independently verified; other particle/wave/anim leads remain unresolved |
| `code/display.cpp`, `tactical.cpp`, `foot.cpp`, `team.cpp`, `techno.cpp` | rally-line Z, placement, factory/unload relayers, bridge fire gates and trigger consumers independently found; proactive bridge AI excluded, remaining waypoint/transport leads open |
| `code/smartdeform.cpp`, `super.cpp`, `ion.cpp`, `ionblast.cpp` | unresolved deformation and effect-Z leads |

Theater-data cross-check also proves that OpenTS's `TrainBridgeSet` is TS-only for this target: active YR parses `BridgeSet`, `WoodBridgeSet`, and ten named bridge-piece keys, but contains no `TrainBridgeSet` key string. `RAILBRDG` therefore cannot be treated as train topology.

## 8. Evidence-backed exclusions and negative facts

- TS Mech, DropPod and subterranean locomotor bindings with no stock YR type binding are excluded unless a separate active YR caller is proved.
- `TooBigToFitUnderBridge` must not become a movement/passability gate.
- automatic same-cell TubeClass shells must not be joined into synthesized low-bridge spans or direction-8 routes.
- low Road overlays must not receive high-bridge height, `OnBridge`, dual occupancy or TubeMovement.
- `BuildRiverBridge @ 0x0059E740` is not a bridge topology builder despite its inherited label; it is waterfall terrain shaping.
- `RandomMapGenerator__RunMapType34Block_Unreferenced @ 0x005A1E10` has no callers and is excluded.
- OpenTS `TrainBridgeSet` and its parallel rail-bridge damage/repair walkers are TS-only. `RAILBRDG1/2` remain active visual assets and must stay under render evidence, not train topology.
- OpenTS Mech, DropPod and subterranean bindings are compiled inactive for stock YR. Meteor behavior is default-off (`Meteorites=no`), and BounceClass's LaserFence collision arm has no stock YR `LaserFence=` building binding.
- the proactive AI surface contains no bridge-specific attack/repair/destroy opcode, CABHUT priority or special route policy; ordinary target/orders and common path costs remain in scope.
- `CellClass__HasBridgeOverlay @ 0x004865D0` and the `0x0057A0C0`/`0x0057A430`/`0x0057A320`/`0x0057ACF0` cluster are shore/water/RMG finalization predicates despite inherited bridge-like labels.
- retail `wbrdge01/02` WaterBridge TMP terrain bytes resolve to ordinary Rough ground across the available theaters; their asset name does not make them runtime bridge topology, a low Road overlay, or TubeClass content.
- `FUN_006E61F0` is generic TagType destroyed-event category bookkeeping, not a bridge-linked-cell registry. The actual event `0x1F` footprint and trigger-action helpers remain active separately.
- refinery dock “`0x16` bridge” terminology is a false positive: `0x16` belongs to a radio/timer docking handshake, not physical bridge state.
- retail `CASANF04` and `CASANF09` through `CASANF14` use Golden Gate/Golden Bridge display names but are ordinary `BuildingType` scenery with normal building strength, armor, placement and debris ownership. They are not CABHUTs, bridge overlays, TubeClass records, or structural-cell bridge topology.
- bridges do not have continuous per-cell hit points. Weapon damage uses the Scenario `SpecialFlags` `DestroyableBridges` bit `0x8000`, BridgeStrength admission and discrete state ladders; `[CombatDamage] DestroyableBridges=yes` is not the live owner.
- `DestroyableBridges` gates weapon damage, not CABHUT C4 or attached-bomb collapse. Hut immunity/ownership/capture and `MultiEngineer` do not suppress the verified repair/collapse transactions, and hut placement does not repair automatically.
- native collapse force-damages ground-list objects and DropIns deck-list objects; it does not force-kill or drown the deck list.
- `BridgeVoxelMax` does not gate BlowUpBridge metallic debris.
- `UnregisterBridgeRepairHut @ 0x00577920` is a stale label for TagClass/global destroyed-event registry cleanup.
- `RecalcBridgeShroudFlags @ 0x00578100` is a misnamed whole-map shroud-edge routine; no bridge-specific shroud recomputation or per-layer shroud model is required.
- no dedicated bridge network packet exists; ordinary deterministic simulation state and RNG carry collapse/repair results.
- `g_IsMapEditor` suppresses BlowUpBridge fallout; this is an editor boundary, not an ordinary gameplay branch.
- explicit sentinel-less malformed tube rows may crash native; Rust's safer rejection is a deliberate robustness divergence, not a retail parity requirement, provided valid-row behavior and deterministic state are exact.
- editor-only placement/writing UI remains excluded; runtime placement, deployment, production exit and unload paths remain active cross-consumers.
- no per-layer shroud model is introduced; active bridge consumers use the ordinary visibility model unless a verified caller proves otherwise.
- cold BSS zero is not evidence that runtime constants are zero or dormant.

## 9. Fresh native spot-checks

All checks were read-only and targeted program `gamemd.exe` explicitly. No Ghidra metadata was mutated.

1. `get_current_program_info`: retail `gamemd.exe`, PE x86, image base `0x00400000`.
2. `ObjectClass::ShouldBeOnBridge @ 0x005F6A70`: destination height query, three-level threshold and destination structural predicate are live; the full reachability input remains an implementation blocker.
3. `MapClass::RegisterBridgeOrTubeHierarchyPairs @ 0x00582D70`: high/wood and tube branches are distinct and register temporary hierarchy pairs.
4. `MapClass::UpdateBridgeEdgeTiles_High @ 0x00576200`: bounded forward search, backwalk, flag/state/overlay mutation, radar dirty and recursion are active.
5. `MapClass::RepairBridgeOrRestoreRamp_Low @ 0x00570050`: primary low-overlay match repairs and returns; only no-match enters the restoration tail.
6. `CellClass::BlowUpBridge @ 0x0047DD70`: occupant fallout and conditional bridge-explosion work are active.
7. `UnitClass::Draw_Sprite_With_BridgeFudge @ 0x0073B140`: TooBig/bridge-body split rendering is active.
8. `RandomMapGenerator__BuildRiverBridge @ 0x0059E740`: caller/callee and write audit confirms waterfall terrain, no bridge overlay/flag writes.
9. `BulletClass__AI @ 0x004666E0`: active structural-current/previous-cell deck-plane crossing in both guided and ballistic paths.
10. `BounceClass__Update @ 0x00439B00`: active structural-current/previous-cell deck-top/underside collision.
11. `OverlayClass::Mark @ 0x005FC570`, `RecalcAttributes @ 0x0047D2B0`, `ComputeBridgeZones @ 0x0056D6E0`, `TubeClass::Constructor @ 0x00727FD0`, `ReadTubesINI @ 0x007283C0`: independent low-overlay/TubeClass split.
12. `PathfinderClass::UpdateBridgePassability @ 0x0042ACF0` and `FindNearbyBridgePeer @ 0x0042B080`: active search-scoped `0x40000` marker overlay, peer queued-path replay, direction-8 tube exit handling, 5x5 ground-occupation marking and exact 4x cost consumer.
13. `FootClass::ShouldBeOnBridge @ 0x004DDC40` and `ObjectClass::ShouldBeOnBridge @ 0x005F6A70`: destination/zone query ownership and active reachability/trigger consumers; not the ground transition write.
14. `FootClass::Set_Height_On_Bridge @ 0x005F5FA0`: active Fly-path height setter using the 416-lepton offset and marked-object remove/put bracket.
15. `TechnoClass::CountAdjacentBridgeDeckTiles @ 0x00703E70`: active render-edge probe returns only 0/1/2 and feeds the TooBig split decision.
16. `TubeClass::Compute_CRC @ 0x00728630`: fixed endpoint/direction/100-word-path/length checksum order; `MapClass::WriteTubesINI @ 0x00728280` is scenario-writer/editor-only and does not establish ordinary skirmish traversal activity.
17. `Read_Theater_TileSets_INI @ 0x00545150`: all ten active bridge-piece globals map to `BridgeTopLeft1/2`, `BridgeTopRight1/2`, `BridgeBottomLeft1/2`, `BridgeBottomRight1/2`, and `BridgeMiddle1/2`; current Rust omits the four bottom keys.
18. `Apply_area_damage @ 0x00489280`: only the original impact cell enters bridge admission; exact high/low state-machine Z window is `(ground+2, ground+5]`, followed by tile-family predicates.
19. `ProcessBridgeDamageStateMachine_Low @ 0x00571490`: EW bridgehead slots 0..2 route into high damage-ramp helpers; a uniform low-helper dispatch is wrong.
20. `UpdateBridgeEdgeTiles_High @ 0x00576200` and `_Low @ 0x00570AE0`: exact family/transition walkers, backward stamp, overlay clear, event, radar and recursive continuation; no native 8-neighbor orphan heuristic.
21. `RepairBridgeOrRestoreRamp_Low @ 0x00570050` and `_High @ 0x00573540`: X-major first-hit 5x5 selection; primary low overlay match returns, while the no-overlay path owns pavement/level/flood-fill/validation fallback.
22. `AnimClass` constructor/AI `0x00421EA0`/`0x00423AC0`, bounce-result `0x00423930`, and `BounceClass::Update @ 0x00439B00`: stock MetallicDebris `DBRIS*` types are active damaging Bouncer animations, not inert presentation records.
23. `BombClass::Detonate @ 0x00438720`: after ordinary attached-bomb warhead/animation work, a `BridgeRepairHut` enters the same bridge-collapse path independently of `DestroyableBridges` and immunity; Rust lacks this entry.
24. `InfantryClass::PerCellProcess @ 0x00519630`: CABHUT engineer output includes observer callbacks and engineer-attached event `0x30`; repair sound/EVA and consumption do not require cells to have changed.
25. `RallyLineRenderer @ 0x006DA9D0` and `TriggerAction__Execute @ 0x006DD8B0`: active bridge-adjusted rally Z and content-conditional bridge-damage action helpers extend scope beyond movement and collapse owners.
26. `NotifyBridgeSpanCollapse @ 0x00575EE0`: tagged-cell, four-wide, endpoint-exclusive event-`0x1F` footprint; distinct from per-cell event `0x18` reachability handling.
27. `CellClass::Scatter_Objects @ 0x00481670` and `UnitClass::Scatter @ 0x00743A50`: active ground/deck occupant-list selection, bounded linked-list snapshot, per-object dispatch and bridge-aware nearby-passable-cell search; current Rust's ground-only/direct-neighbor paths are not equivalent.
28. `UnitClass::PerCellProcess @ 0x00739EC0`: the stopped-blocked C4-warhead self-damage path has an active `(not OnBridge) OR (not structural deck)` admission clause, preserving units correctly layered on an intact structural bridge cell.
29. `TechnoClass::Evaluate_Candidate @ 0x006F7CA0`, `Greatest_Threat` callsites `0x006F92AC`/`0x006F9C2B`/`0x006F9D76`, and `Scan_Cell_For_Target @ 0x006F8C00`: ordinary target acquisition applies the both-structural-cells plus unequal-`OnBridge` rejection; Rust currently has no equivalent gate.
30. `TacticalClass` inverse `0x006D6590`: after more than `0xB3` failed bridge-correction attempts, native returns the original packed signed-short coordinate rather than clamping negative/off-map components to zero.
31. `CellClass::CanPlaceTiberium @ 0x004838E0`: active TIBTRE-spawned ore placement rejects raw structural/destroyed bridge mask `0x500`; Rust already carries the same two-bit refusal and must preserve it.
32. `TechnoClass::DrawActionLines @ 0x004DC060` and tactical draw caller `0x006D4735..0x006D4750`: selected movement action-line endpoint Z uses ground height plus the bridge offset when raw structural bit `0x100` is set.
33. `TechnoClass::DrawRadarActionLines @ 0x004DC340` and caller `0x006D478E`: Psychic Sensor enemy action lines use the same structural endpoint correction; retail `NAPSIS` supplies the active `PsychicDetectionRadius=15` binding.
34. `AStar_main_loop @ 0x00429A90`, `Path_smooth_corners @ 0x0042B210`, `Path_optimize_straight_segments @ 0x0042B7F0`, `Path_smooth_single_segment @ 0x0042B420`, and `Path_Reroute_Straight_Line @ 0x0042BE20`: both unconditional post-success optimizers carry structural-deck effective height into `Can_Enter_Cell`; Rust's boolean-only single smoother does not.
35. `Apply_area_damage @ 0x00489280` Rocker branch `0x00489B84..0x00489E3E`: the active 7x7 secondary-effect scan reuses the initial bridge-selected `+0xE8`/`+0xE4` occupant plane and dispatches the rocking virtual; retail `V3WH` binds `Rocker=yes`.
36. `Apply_area_damage @ 0x00489280` tolerance branches `0x00489347..0x00489372`, `0x0048947F..0x004894A4` and repeated CellSpread writers through `0x004899D4`: `CellSpread > 0.5` plus a qualifying object at strict distance `<0x55` controls final bridge/`OnBridge` suppression independently of Rocker.

## 10. Adversarial questions

1. **Could “bridge” labels alone have pulled RMG waterfalls and TubeClass shells into runtime bridge scope?** Yes; direct field writes and retail land data disprove both conflations.
2. **Could the absence of `[Tubes]` in stock maps justify dropping GSI-04.15?** No. The active executable parser and movement path accept valid custom-map content, and the user explicitly requires the row. The absence changes activity classification and fixture choice, not the need for an exact supported path.
3. **Could current broad Rust coverage make ignored/unchecked residuals harmless?** No. Several residuals affect common collapse, repair, landing, targeting and generated-map paths; the goal explicitly keeps any such mechanism open.
4. **Could OpenTS be ported directly because it exposes readable functions?** No. It mixes inherited TS behavior, reconstruction choices and later documentation. Every material rule must be rebound to active YR native/data evidence.
5. **Could a single central bridge service replace the distributed owners?** Not without architecture drift. Facts, runtime mutation, pathing, movement, combat, rendering and persistence already have distinct repository owners and native responsibilities.

## 11. Zero-add pass

The first discovery pass was **not** zero-add. Searching OpenTS after reading the existing bridge corpus added at least these material candidates:

- bullet bridge-plane collision;
- bounce/effect bridge-plane collision;
- active RMG low-deck and repair-hut emission;
- low-overlay procedural endpoint expansion;
- particle/wave/animation collision and landing leads;
- placement, AI/team/order, deformation and superweapon effect-Z leads.

The second discovery pass also failed zero-add: it added the CABHUT attached-bomb entry, engineer observer/event outputs, rally-line Z adjustment, trigger-action bridge-damage helpers, the active DBRIS Anim/Bounce runtime, and the exact theater bottom-piece inputs. Those are now represented in BR-M01, BR-M15, BR-M18, BR-M19 and BR-M21.

The fresh third-pass omission critic also failed zero-add. It added the tactical pixel-to-cell inverse, Mirage disguise neighbor acquisition by bridge plane, and raw-`0x1000` PixelFX suppression, plus the WaterBridge TMP, generic tag-registry and refinery-radio exclusions. Those are now represented in BR-M06, BR-M20 and BR-M21.

The fourth read-only omission critic also failed zero-add. It added bridge-aware scatter/crusher dispatch, the structural-deck exemption in stopped-blocked unit self-damage, and the Golden Gate-named ordinary-building exclusion. These are now represented in BR-M23 and the negative-fact ledger.

The fifth read-only omission critic also failed zero-add. It added the ordinary `Evaluate_Candidate` deck/under-span target rejection and the signed, unclamped tactical-inverse exhausted-search fallback. Both are now explicit under BR-M21 and OQ-18/OQ-26.

The sixth read-only omission critic also failed zero-add. It added the active TIBTRE ore-placement `0x500` consumer and selected/Psychic Sensor action-line endpoint bridge-height consumers. These are now represented by BR-M20, BR-M24 and OQ-27/OQ-28.

The seventh read-only omission critic also failed zero-add. It added the two-pass post-A* bridge-height-aware smoothing pipeline. This is now represented by BR-M25 and OQ-29.

The eighth read-only omission critic also failed zero-add. It added the active AoE Rocker branch's reuse of the selected bridge occupant plane. This is now represented by BR-M26 and OQ-30.

The ninth read-only omission critic also failed zero-add. It added the independent `CellSpread > 0.5` bridge-object tolerance flag and final damage suppression. This is now represented by BR-M27 and OQ-31.

The tenth read-only omission critic **passed zero-add**: it found no active-retail bridge mechanism or evidence-backed exclusion absent from BR-M01..BR-M27, OQ-01..OQ-31, or the negative-fact ledger. The coverage map is therefore frozen for implementation-contract and design work. A later verified contradiction or genuinely new active caller must reopen the affected row rather than being recorded as a residual.

## 12. Implementation handoff boundary

No Rust implementation is authorized until the global coverage ledger passes zero-add. After that, an individual mechanism may be implemented only after its own material open questions have been resolved into a reviewed implementation contract; open questions in other mechanisms keep those other mechanisms open.

When a mechanism becomes implementation-ready, its handoff must contain:

- a bounded requirement statement;
- active native addresses/control flow and retail inputs;
- exact current Rust owner and delta;
- evidence-backed exclusions;
- deterministic/RNG and ordering requirements;
- focused `--lib` tests, including at least one negative/do-not-do fixture;
- a fresh read-only critic bundle consisting of requirement, native evidence, diff and literal validation output.

The builder/critic closure units are deliberately narrower than the GSI rows:

1. theater/rules/asset inputs, raw flag facts and active TIBTRE resource-placement preservation;
2. active RMG low-deck/end/CABHUT production;
3. low-overlay procedural map-load stamping and flat Road mutation;
4. high topology, records, zones, hierarchy and exact edge re-stamp;
5. explicit TubeClass load/hierarchy/direction-8 movement and persistence;
6. high dual occupancy, entry, A*, peer markers and locomotor transitions;
7. both post-A* bridge-height-aware smoothing passes;
8. selected-layer scatter/crusher dispatch and stuck-unit self-damage safety;
9. spawn, Unlimbo, landing, paradrop, teleport and other relayers;
10. impact-cell admission, Z/family gates and four-path RNG ordering;
11. CellSpread bridge-object tolerance and final layer suppression;
12. AoE Rocker bridge-plane secondary-effect dispatch;
13. high/low ladders, setters and direct walkers;
14. synchronous BlowUpBridge fallout and DBRIS Anim/Bounce runtime;
15. engineer repair scan, walkers and no-overlay terrain/height fallback;
16. CABHUT cursor, C4 timer, attached-bomb and fallback-collapse entries;
17. repair observers/tags and multi-engineer Logic-vector order;
18. projectile/fire/superweapon and remaining effect-plane consumers;
19. tactical inverse, target acquisition, Mirage plane scan, placement, factory/unload, rally/waypoint and trigger-action consumers;
20. high/low render ordering, TooBig split, shadows/railings, raw-`0x1000` PixelFX, selected/enemy action lines, radar/audio;
21. save/load/checksum/rebuild and deterministic effect/RNG projection.

The implementation order should follow dependencies, not GSI numbering:

1. authoritative map/RMG inputs and raw facts;
2. topology, records, zones and mutable terrain;
3. occupancy, entry, A* and movement/landing/tubes;
4. damage/collapse/repair and their deterministic side effects;
5. projectile/effect collision and cross-system consumers;
6. rendering/radar/audio and persistence;
7. bridge-wide reverse audit.

## 13. Ghidra annotation candidates

No annotation synchronization was authorized or performed. Candidates for a later explicitly authorized sync:

- retain `RandomMapGenerator__BuildRiverBridge` only with a plate comment stating that it stamps waterfall terrain and is not runtime bridge topology;
- rename low `BridgeRecord` kind terminology in documentation to tube/tunnel connection;
- preserve `OverlayClass::Mark`, `BulletClass__AI`, and `BounceClass__Update` bridge branch comments if existing documentation is incomplete;
- correct any lingering label that calls RMG shore finalization or water predicates bridge repair/bridge overlay behavior.

## 14. Source ledger

Primary repository evidence includes the bridge system models, map-load/stamping, occupancy, zones/hierarchy, entry/A*, locomotion, damage/collapse/repair/CABHUT, rendering/radar, cross-consumer and trace reports under `docs/research/bridges/`, plus current RMG reports under `docs/research/skirmish-ui/`.

Retail data inspected includes `rulesmd.ini`, `artmd.ini`, theater INIs, `RandMap.Sed`, stock/loose map payloads and the existing 385-map `[Tubes]` census.

Current Rust owners inspected include:

- `src/map/{bridge_facts,resolved_terrain,overlay_types,tube_facts,tubes,theater}.rs`;
- `src/map/rmg/` and its bridge/deck/carve pipeline;
- `src/sim/{bridge_state,bridge_specs,occupancy,pathfinding,movement,combat,world}.rs`;
- spawn/production/aircraft/superweapon/input/render/radar/audio/snapshot/hash owners.

OpenTS navigation covered `code/{overlay,cell,map,astar,drive,walk,hover,infantry,aircraft,bullet,bounce,mapgen,tube,particle,partsys,wave,anim,display,tactical,foot,team,techno,smartdeform,super,ion,ionblast}.cpp` and related headers.
