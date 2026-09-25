//! Simulation snapshot serialization for mid-match save/load.
//!
//! Serializes the full `Simulation` state into a compact binary blob via
//! bincode. Caches and event queues are `#[serde(skip)]`'d on `Simulation`
//! and must be rebuilt through the validated cache and map-authority restore
//! sequence before the simulation is exposed again.
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on sim/world (Simulation).
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::sim::world::Simulation;

/// Bump this when the snapshot binary format changes in a breaking way.
// Bumped 13 -> 14 for the serialized occupancy entry-order fields used to rebuild
// the skipped CellClass-style occupancy cache after load.
// Bumped 14 -> 15: active-vector order + id/enter-order counters relocated under
// Simulation.substrate (ObjectSubstrate); bincode layout changed (state hash unchanged).
// Bumped 15 -> 16: EntityStore relocated under Simulation.substrate (Slice 1b); bincode
// layout changed (state hash unchanged — world_hash reads the store via the new path).
// Bumped 16 -> 17: MissionCom folded into state_hash (Slice 8); bincode layout
// unchanged (MissionCom already serialized since Slice 6), only the hash changed.
// Bumped 17 -> 18: Factory/Economy authority flip (P5b) — the factory registry +
// the per-house economy statistics are now serialized + hashed; the frames-timer
// per-item field progress_carry is removed from the hash (progress lives in
// Factory; remaining_base_frames stays as the sidebar-ETA mirror); next_insertion_seq
// + seq_carry fields removed (insertion_seq == front enqueue_order); the C1
// factory-step-before-house-tail ordering lock is folded in.
// Bumped 18 -> 19: queue-of-record retirement (P5d) — `queues_by_owner` + `BuildQueueItem`
// are retired; the FIFO queue-of-record moves into the registry (`Factory.queue` of
// `QueueEntry{type_id, enqueue_order, total_base_frames}` + the new active-build
// `Factory.active_total_base_frames`). The per-item `queues_by_owner` hash fold is removed;
// `remaining_base_frames` no longer exists (derived from `progress` at sidebar-view time,
// not hashed). bincode layout changes (the `queues_by_owner` field is gone, the registry
// gains fields), so the version MUST bump.
// Bumped 19 -> 20: S2 — `mission.current`/`substate` authority moves to dispatch time for
// scoped move units (arrival tick hashes Move) and load trusts the serialized MissionCom
// (post-load re-derive deleted). Layout is unchanged, but a pre-S2 save replayed on S2
// logic diverges on arrival ticks, so cross-version restores must be refused.
// Bumped 20 -> 21: per-cell radiation field (substrate Slice 7). `Simulation` gains the
// serialized `radiation` state (cell levels + site registry, both state-hashed) and
// `GameEntity` gains `immune_to_radiation`; RadLevel>0 detonations now deal periodic
// foot-unit damage, so a pre-21 save replayed on 21 logic diverges.
// Bumped 21 -> 22: ScenarioSession (SC-2) — `seed`/frame-clock/`GameOptions` move under
// `Simulation.session` and the session identity fields (map name, theater, bounds, MP
// start waypoints, slot->house) are serialized; bincode layout changes. The move itself
// is hash-neutral (golden baseline unshifted); the identity fields fold into the hash in
// the same slice (documented on the golden-harness constant).
// Bumped 22 -> 23: S3 — Unit barrel destinations are read per-object pre-death (kill-tick
// aim hold changes hashed FacingClass values on kill ticks) and idle machine-less Units
// hash mission Guard(5) instead of the legacy None placeholder. Layout unchanged, but a
// pre-S3 save replayed on S3 logic diverges on the first idle-unit tick, so cross-version
// restores must be refused. (21 and 22 were taken by the parallel radiation and
// ScenarioSession slices; the concurrent bumps merged as 22 -> 23.)
// Bumped 23 -> 24: S4a authoritative flip (Option B) — each live non-miner Unit's
// mission (+0xC4 tick_counter + derived_mission) is now committed at the per-object
// AI host (pre-movement, LogicVector order) instead of in movement_tick (scoped
// movers) / the Phase-9 tail (idle). Commit timing is the gamemd-faithful per-object
// point: a unit that retasks mid-tick (e.g. an idle Guard unit that opportunity-
// acquires a target during combat) now hashes the host-committed mission, not the
// end-of-tick re-derivation. Layout unchanged and the committed goldens are unshifted
// (those scenarios don't exercise mid-tick non-miner retasking), but a pre-S4a save
// replayed on S4a logic diverges on the first such tick, so cross-version restores
// must be refused.
// S4b: GameEntity gains the hashed `damage_particle_live_until` (`+0x308`-
// equivalent) field, folded into the state hash, so 24→25 re-baselines. The new
// field is zero for every entity in stock YR (the AI_Update spark gate is
// Cyborg-only, with no stock users), so the only hash shift is the extra per-
// entity zero in the fold — no behavior change to any committed golden scenario.
// Bumped 25 -> 26: HouseState gains the serialized + hashed native per-house
// difficulty field (Hard=0, Normal=1, Easy=2). A pre-26 save lacks the field and
// cannot preserve mixed-difficulty AI behavior after load.
// Bumped 26 -> 27: ObjectSubstrate gains the serialized AnimStore and GameEntity
// gains the authoritative damage-fire transition cache plus eight animation IDs.
// Bumped 27 -> 28: independent object-alive/limbo/cell lifecycle state,
// lifecycle bookkeeping, and the ordered pending-delete queue are serialized and
// hashed instead of being reconstructed from store/LogicVector presence.
// Bumped 28 -> 29: exact native-width Mission state, category readiness leaves,
// archived target/falling state, and raw locomotor-readiness inputs replace the
// reduced Mission schema and are serialized + hashed.
// Bumped 29 -> 30: the miner FSM cursor (`Miner.state`) retired from the
// serialized Miner component — `MissionCom.handler_state` is the cursor of
// record (Harvest handler absorption / substate-authority flip).
// Bumped 33 -> 34: ParticleSystemStore moved into ObjectSubstrate and became
// serialized; particle systems now share object IDs, LogicVector membership,
// and deferred finalization.
// Bumped 34 -> 35: Phase-0 persistence contract. ScenarioSession gains native
// HouseClass registration order and a wrapping 32-bit frame; frame-anchored
// timers replace reduced countdown state; HouseState gains MapIsClear;
// GameEntity persists MoveSound grace state and HomingTarget's object/cell
// discriminator; PassengerCargo persists one Size value per entry; animation
// membership is rebuilt rather than serialized. Production loading now also
// enforces exact map/rules/session metadata before stable-ID fixup.
// Bumped 35 -> 36: lifecycle target and animation identity state is persisted.
// Bumped 36 -> 37: process-global Main/MapGen RNG cursors are no longer
// serialized; in-scenario production load retains their live process state.
// Bumped 37 -> 38: DriveLocomotionRuntime persists the independent head-to
// occupation footprint and whether the current-cell occupation was cleared.
// Bumped 38 -> 39: overlay wall ownership became authoritative persisted state.
// Bumped 39 -> 40: ObjectSubstrate persists the authoritative per-cell raw
// ground/deck occupation bytes instead of reconstructing them from object lists.
// Bumped 40 -> 41: HouseState gains the serialized MultiplayPassive house-type
// fact. Defeat evaluation and the game-over alive scan both skip passive houses,
// so it is an authoritative outcome input and cannot be re-derived on load —
// `rebuild_caches_after_load` takes no RuleSet. Serialized but NOT hashed.
// Bumped 41 -> 42: GameEntity gains the passive target-acquisition bookkeeping
// — `last_target_scan_frame` and `passively_acquired_target` — and its
// `passive_scan_timer` is now armed at the construction frame instead of left
// unarmed. All three are HASHED, so a v41 save written before this change
// restores into a world whose hash differs from a v41 written after it.
// Bumped 42 -> 43: `InfantryRuntime` gains `idle_action_timer` (two u32s), so
// the component grows from 3 to 11 bytes. The encoding is bincode, which is not
// self-describing — the decoder reads the next field's bytes unconditionally and
// a `#[serde(default)]` never fires for a short record. A v42 save read by this
// code would therefore pass the version check and then misread every byte after
// the first infantry entity. The bump turns that silent corruption into a clean
// rejection. The new field is also HASHED.
// Bumped 43 -> 44: `GameEntity` gains the spawn-manager pool
// (`spawn_manager: Option<SpawnManagerState>` — spawn type, missile family,
// regen/reload/kamikaze frames, both manager timers, both targets, manager
// mode, and a variable-length slot vector) and the child back-pointer
// (`spawn_owner_id: Option<u64>`). Same bincode trap as 42 -> 43: the encoding
// is not self-describing, so the decoder reads the next field's bytes
// unconditionally and `#[serde(default)]` never fires for a short record. A v43
// save read by this code would pass the version check and then misread every
// byte from the first entity onward — these two fields are on EVERY entity, not
// just spawner parents, so the corruption starts at entity one. Both are
// HASHED, but only when present (see `world_hash.rs`).
//
// Bumped 44 -> 45: `AnimOverlayState`'s `rate_ms`/`elapsed_ms` became
// `rate_logic_frames`/`elapsed_logic_frames`. Both fields were `u32` before and
// after, so the encoded record is exactly the same width and a v44 save
// deserializes without any error at all — it just means something else. gamemd
// counts an animation's frame delay in logic frames, not wall-clock time, so a
// stored `rate_ms` of 266 comes back as a 266-*frame* delay: the building
// animation is roughly 44x too slow and looks stopped. Identical width is what
// makes this dangerous rather than safe — the test a unit change has to pass is
// whether old bytes still deserialize to the correct meaning, not whether they
// deserialize at all. At v45 this was treated as presentation state and was not
// hashed; v79 below changes that contract.
// (44 is claimed by the in-flight spawner slice, which lands first; this jumps
// over it deliberately.)
//
// FORK WARNING — versions 41 through 46 are AMBIGUOUS. Two branches (this
// repo's dev and the foundations-contracts line merged via PR #109/#110)
// diverged from v40 and independently assigned 41..46 to entirely different
// layout changes. The two lineages are preserved verbatim above and below;
// a version tag in that range does NOT identify a unique layout, so no
// compatibility path may ever key on 41..46. The merge of the two branches
// lands on 47, whose layout is the union of both lineages' fields.
// [foundations lineage] Bumped 40 -> 41: persistent BulletClass-style projectile state is serialized.
// Bumped 41 -> 42: persistent BulletClass collision policy is serialized.
// Bumped 42 -> 43: TriggerRuntime latches now participate in the lockstep hash.
// Bumped 43 -> 44: TeamClass raw actions, deferred advance, timers, attachment
// identities, per-type counts, and non-CRC success state are authoritative.
// Bumped 44 -> 45: piggyback persistence stores one complete nested locomotor runtime.
// Bumped 45 -> 46: Tunnel and DropPod typed special-locomotor runtimes are
// serialized on GameEntity, including their phase and landing state.
// Bumped {41..46 fork} -> 47: merge of the two lineages; layout is the
// union of every field both sides added.
//
// Bumped 47 -> 48: `DriveLocomotionRuntime` gains `occupation_handoff`, the
// forward RawTrack handoff mark that accompanies the head-to mark, and it is
// inserted BETWEEN `occupation_head_to` and `current_occupation_cleared` rather
// than appended. Same bincode trap as 42 -> 43 and 43 -> 44: the encoding is not
// self-describing, so the decoder reads the next field's bytes unconditionally
// and `#[serde(default)]` never fires for a short record — a mid-struct
// insertion is exactly the case it cannot cover. A v47 save written before this
// change would pass the version check and then misread every byte from that
// field onward, for that vehicle and every entity after it. Both occupation
// marks are rebuilt into the transient `CellOccupationGrid` on load, so a
// misread is a wrong cell reservation, not just a cosmetic field.
//
// Bumped 48 -> 49: ObjectSubstrate gains authoritative serialized building
// hidden-occupation counters, and GameEntity gains the immutable fixed-slot
// type profile needed to reverse a contribution after load. RemoveOccupy's
// enter-only cancellation makes this state impossible to reconstruct exactly
// from the currently placed object set.
//
// Bumped 49 -> 50: ObjectSubstrate gains authoritative serialized per-house
// Building base reservations (including shared dummy state), and GameEntity
// gains the immutable signed AIBaseSpacing writer profile.
// Bumped 50 -> 51: GameEntity gains serialized airborne spatial-bucket
// membership and vector-tail order. These fields drive Apply_area_damage
// receiver order and therefore must not be defaulted from an older bincode tail.
// Bumped 51 -> 52: GameEntity gains the mutable per-Techno armor multiplier;
// its exact double bits affect every later receiver result.
// Bumped 52 -> 53: GameEntity gains the authoritative Psychedelic berserk byte
// and signed timer; both affect subsequent receiver callbacks and targeting.
// Bumped 53 -> 54: GameEntity gains the persistent WasAttackedByEnemy byte;
// building AI reads it after load when deciding whether to sell at red health.
// Bumped 54 -> 55: HouseState gains authoritative CurrentIQ.
// Bumped 55 -> 56: GameEntity gains the damage-Smoke ParticleSystem identity.
// Bumped 56 -> 57: GameEntity gains the controller-owned CaptureManager
// capacity snapshot and ordered victim-link vector. Bincode cannot default a
// missing mid-record field safely, and both fields change later mission state.
// Bumped 57 -> 58: HouseState gains receiver-updated AngerStruct scores and
// the selected enemy-house identity. Both feed later AI target decisions.
// Bumped 58 -> 59: the Building shared C4/PostMortem latch now preserves its
// signed start/duration and nullable retained source identity.
// Bumped 59 -> 60: wall-sale commands may persist in the pending queue, and
// HouseState/ScenarioSession now preserve distinct PlayerControl and GameMode
// inputs used by their native EventClass receiver gate.
// Bumped 60 -> 61: ScenarioSession now persists native ScenarioFlags bit 0x20,
// which suppresses direct and area damage and therefore changes later world state.
// Bumped 61 -> 62: Position gains authoritative signed exact-Z leptons, and
// active low-bridge TubeMovement now persists its sole live
// `{tube_id, cursor, target_xyz}` payload. Both change GameEntity's bincode
// layout and must resume without rebuilding the mover's detached cell state.
// Bumped 62 -> 63: GameEntity persists the immutable `Factory=BuildingType`
// profile that gates later house-edge refreshes on reveal and owner transfer.
// Bumped 63 -> 64: HouseState persists the aggregate active SpySat latch.
// Bumped 64 -> 65: ScenarioSession persists both map lighting profiles plus
// the mutable global ambient target/profile and signed transition timer;
// the disproven queued Lightning Storm payload is removed at the same boundary.
// Bumped 65 -> 66: synchronized projectile trajectory/visual state,
// Techno cloak/disguise producers, and the active typed locomotor payload are
// now serialized. Bincode cannot safely decode the old mid-struct layouts.
// Bumped 66 -> 67: persistent WaveClass lifecycle, aircraft release-tail,
// guided projectile, CellClass visibility, BuildingLight, and Anim draw state.
// Bumped 67 -> 68: projectile collision policy, Wave recorded-cell damage
// payload, infantry subcell owners, fogged-object footprints, and per-house
// sensor state are serialized and hashed.
// Bumped 68 -> 69: CellClass per-cell cloak-owner words are serialized and hashed.
// Bumped 69 -> 70: terrain, projectile, and wave stores dropped their local
// counters after consolidation into the serialized global object-id source.
// Bumped 70 -> 71: ProjectileTarget adds the explicit native null-target
// discriminant and stores CellClass identity rather than a frozen target Vec3.
// Bumped 71 -> 72: the snapshot prefix now carries explicit VERA product and
// public-envelope identity plus the player-authored save description.
// Bumped 72 -> 73: HouseState now persists the accepted match-result kind,
// absolute SavourDelay frame target, and expiry latch. These fields control
// the terminal frame and cannot be reconstructed from app state after load.
// Bumped 73 -> 74: pending CommandEnvelope payloads can now carry the native
// EXIT event. App-consumed execution edges remain transient and are not saved.
// Bumped 74 -> 75: generic Building delayed-fire state now persists its signed
// remaining counter and saved weapon slot across save/load.
// Bumped 75 -> 76: AnimClass now persists its cell-drawer and
// terrain-attached constructor bytes.
// Bumped 76 -> 77: GameEntity now persists FootClass's wrapping SHP body-frame
// counter plus Drive/Ship's SHP movement-predicate runtime. Both are
// hash-authoritative and resume without a visual-sequence reset.
// Bumped 77 -> 78: living GameEntity Animation state now advances inside the
// committed master frame and all four serialized fields participate in the
// lockstep hash. Layout is unchanged, but cross-version replay would diverge.
// Bumped 78 -> 79: BuildingAnimOverlays was already serialized with this exact
// layout, but crane, refinery-bale, and tank-bunker overlays now finalize inside
// the committed master frame before `state_hash`, and component presence, vector
// order, and every overlay field participate in the lockstep hash. This is a
// behavior/hash-only boundary: old bytes would decode, but resume under different
// timing and produce an incompatible returned hash, so cross-version load is refused.
// Bumped 79 -> 80: the natural win/loss terminal edge now serializes and hashes
// its one-shot raw score snapshot before returning. This adds the snapshot field
// and prevents score-bonus Scenario RNG draws from repeating after load.
// Bumped 80 -> 81: pending CommandEnvelope payloads can now carry an offline
// SetGameSpeed transition. Appending the enum variant changes the bincode schema.
// Bumped 84 -> 85: Simulation now retains the signed `[Map] Size=` height used
// to normalize later trigger-action-40 LocalSize writers. Without the serialized
// input, a second writer after load could diverge even though mutable bounds load.
// Bumped 85 -> 86: every trigger-action-40 execution now serializes and hashes a
// distinct playfield revision so repeated writers rebuild presentation after save/load.
// Bumped 86 -> 87: GameEntity now persists and hashes the canonical
// TechnoClass+0x3D5 playfield-membership byte. Its hysteretic ordinary-movement
// writer means it cannot be reconstructed from position after load.
// Bumped 87 -> 88: GameEntity now persists the exact deposited sensor owner,
// cell, add/remove radii and building/unit discriminator. Limbo, movement, and
// owner transfer must remove the historical deposit rather than recomputing it
// from current position/rules, so this future-affecting cache is hash authority.
// Bumped 88 -> 89: ProjectileTarget adds the native shared-dummy pointer kind.
// Bumped 89 -> 90: exact real-cell 0x1180 values join Scenario persistence and
// hashing, while the process-global dummy's live subset joins the canonical
// hash even without a retained projectile. The dummy remains outside the
// payload and accepted Resize/load reconstructs it to zero.
// Bumped 90 -> 91: the active factory PendingObject now persists the one-shot
// completion-accounted latch so a refused delivery resumed after load cannot
// increment the score-screen Built statistic again.
// Bumped 91 -> 92: persist each House's BaseClass reservation bounds/vector;
// the process-global reservation dummy mask remains outside the payload and
// reconstructs cleared like native CellClass save/load.
// Bumped 92 -> 93: HouseState gains the serialized/hash-authoritative strategy
// emergency mode, persistent All-To-Hunt target-bias latch, and signed last-
// Building-attack frame. Bincode cannot safely default an absent struct tail.
// Bumped 93 -> 94: add the House last-attacker index plus persistent Techno
// recruitment/archive/base-response cooldown state.
// Bumped 94 -> 95: add TeamType priority/base-defence metadata and TeamClass
// response latches/timer state.
// Bumped 95 -> 96: TeamScriptVm persists the ordered AIMD/map registries.
// Bumped 96 -> 97: resolved ScriptType and TaskForce records now persist their
// fixed-AIMD/scenario provenance. Bincode encodes structs positionally, so serde
// defaults cannot safely decode the shorter v96 record.
// Bumped 97 -> 98: resolved AITriggerType records now persist every proven
// typed raw-reader field in addition to their lossless 18-token source record.
// Bumped 98 -> 99: remove the falsely retained AITrigger token-4 scalar;
// native requires that token but discards it before deriving `+0xB0`.
// Bumped 99 -> 100: retain the three TeamType post-load zone-derivation
// fields serialized inside the ordered TeamScriptVm registry.
// Bumped 100 -> 101: retain each compact TaskForce member's category-distinct
// resolved TechnoType identity rather than an ambiguous interned name alone.
// Bumped 101 -> 102: retain AITrigger token 6's category-distinct resolved
// TechnoType identity rather than an ambiguous interned name alone.
// Bumped 102 -> 103: WaveClass persists live ownership/fade/CellClass identity,
// and destroyable-cliff replacement persists exact changed CellClass values.
// Bumped 103 -> 104: every Techno persists its constructor Scenario-RNG low
// word, and authored structure-upgrade Technos persist their parent/slot link.
// Bumped 104 -> 105: persist active and stashed Drive/Ship locomotor slope
// cache/global-frame timer state; the positional payload enum changed shape.
// Bumped 105 -> 106: active-retail Cell ground is one 104-lepton numeric
// authority rather than the false 90-lepton duplicate. Shape is unchanged,
// but retained Cell targets/world state would resume with different Z results.
// Bumped 106 -> 107: behavior-3 Spark now consumes the shared CellClass dummy's
// persistent Level and Slope without requiring a retained dummy projectile.
// The wire shape remains unchanged, but older saves do not certify the same
// future-affecting lockstep hash authority for those live process-global bytes.
// Bumped 107 -> 108: HouseState gains the serialized/hash-authoritative packed
// alternate base cell written by trigger actions 137/138. Bincode encodes the
// HouseState layout positionally, so an older record cannot supply this field.
// Bumped 108 -> 109: persist the lifecycle-maintained per-House BuildConst
// acquisition order and each entity's immutable resolved membership bit.
// Bumped 109 -> 110: persist ordered House BasePlan authority and the immutable
// BuildingType facts consumed by its Unlimbo/Limbo lifecycle writers.
// Bumped 110 -> 111: persist the distinct BaseClass plan center written after
// a successful non-controlled ConstructionYard deployment.
// Bumped 111 -> 112: persist the three independent House AI activation
// latches co-enabled by successful qualifying base-unit deployment.
// Bumped 112 -> 113: persist House AutocreateAllowed beside the three deploy
// latches in native conceptual byte order.
// Bumped 113 -> 114: persist the raw 256-entry MapClass crate-slot table,
// including accepted ghosts and all native timer words, plus the per-Anim
// Convert palette selector installed by startup crate CellAnim.
// Bumped 114 -> 115: OverlayGrid persists and hashes the retained real-cell
// authored/runtime wall-neighbor count plane. Final overlay identities cannot
// reconstruct counts left behind by a later authored low-body overwrite. The
// v115 hash schema also folds the live shared-dummy overlay identity/state;
// that process-global pair is intentionally not Scenario-serialized.
// Bumped 115 -> 116: `NativeTiberiumClassState` carries the native queue stores
// (`NativeTiberiumQueue`: entry array, heap of references, capacity) and
// `OreGrowthState::native_rect`; the serialized queue shape changed.
// Bumped 116 -> 117: the v117 hash schema folds the disguise-detect plane —
// FogState's `CellClass+0xAC[house]` counters and the cached
// `DetectDisguiseRange=` radius on each SensorDeposit. Both fields already
// round-trip (they are `#[serde(default)]` additive), so this bump exists to
// keep the hash-schema probes and the serialized version aligned.
// Bumped 117 -> 118: `GameEntity` gains the `UnitClass+0x6AF` turret rotation
// latch and the `TechnoClass+0x120` last-fire frame. Both are inserted
// MID-STRUCT — after `barrel_facing`, before `building_up` — and both are
// present on EVERY entity. Bincode is not self-describing: the decoder reads
// the next field's bytes unconditionally and `#[serde(default)]` never fires
// for a short record, so a v117 save would pass the version check and then
// misread every byte from entity one onward. A mid-struct insertion is exactly
// the case serde defaults cannot cover, so the version has to move.
// Bumped 118 -> 119: `ProjectileGuidance` gains the five fields the native
// speed ramp needs — `BulletClass::AI @ 0x00466985` accelerates a homing bullet
// from one lepton per frame toward the MaxSpeed stored at `+0x110`, so a
// projectile must carry its current speed, its cap, its acceleration and its
// flight heading. The heading is stored rather than derived because VERA's
// velocity is integral: at magnitude 1 the direction would be destroyed by
// truncation and the missile would leave along an axis instead of the barrel.
// `ProjectileTrajectory` also gains a `Vertical` variant. All five fields are
// APPENDED at the end of the struct and the variant is appended to the enum, so
// unlike the 117 and 118 bumps this one is serde-coverable in principle; the
// version still moves because the hash schema folds the new state.
// Bumped 119 -> 120: `ObjectSubstrate` gains the `VoxelAnimClass` registry —
// the flying VXL debris a vehicle or building death throws. The field is
// `#[serde(default)]` and appended, so bincode can read a v119 record, but the
// version still moves because the hash schema folds the new store AND because
// the death path now consumes draws it did not before: one
// `RandomRanged(MinDebris, MaxDebris - 1)` per destroyed Techno with
// `MaxDebris > 0`, then per debris entry one `Random__Next()` and seven draws
// per voxel piece, then one `RandomRanged` per SHP debris anim
// (`TechnoClass::ReceiveDamage @ 0x00701900`, `0x00702281`..`0x0070256C`).
// Every consumer after a death in the same tick therefore reads a different
// cursor than it did on v119.
// Bumped 120 -> 123: combat explosions became `AnimClass` instances. They used
// to be legacy `WorldEffect` records, which are in neither `AnimStore`'s
// serialized shape nor the multiplayer checksum; they are now ordinary members
// of `substrate.anims`, which is in both. So every shot that spawns a warhead
// `AnimList=` animation now adds a hash-participating, serialized object where
// it previously added none. No committed golden actually moved on this change
// — the global parity harness authors no `AnimList=`/`Explosion=`/
// `DestroyAnim=`, so it never reached the new path — but any future fixture
// that fires a shot with explosion art will hash differently from a v120 one,
// which is exactly what the version guards. `AnimTypeRuntimeConfig` also
// gained the twenty previously unparsed art keys; that is rules data and is
// not serialized.
//
// 121 and 122 are deliberately skipped because live unmerged branches already
// stamp them, and two branches stamping different serialized shapes with one
// number defeats the guard: a snapshot written under one and loaded under the
// other passes the version check and then mis-decodes. Re-surveyed across
// every local and `origin` ref when this branch was rebased for merge —
// 120 `origin/main` (the `VoxelAnimClass` debris store, now landed),
// 121 `origin/feature/phase3-map-spatial-close`,
// 122 `origin/feature/phase3-map-spatial-integration`;
// nothing else claims 123. Whoever resolves a `snapshot.rs` merge conflict
// here must re-run that survey rather than picking a side.
// v126 adds the retained homing Inaccurate flag, consumed by BulletClass's
// final impact-coordinate snap. v123 cannot decode the new guidance shape.
// Versions 124 and 125 are reserved by the concurrent parasite and audio-lane
// changes, respectively; this format must not share either version.
// v128 retains Bullet launch/previous-cell collision inputs and policy.
// v129 stores exact Bullet binary64 velocity, removes duplicate Vertical
// direction and captured gravity, and retains the Floater policy.
// v130 replaces the visit counter with the signed frame-anchored Bullet Arm timer
// and retains Dropping so Check runs before detector-only admission is suppressed.
// v131 persists the `ScenarioClass` flags bits `0x40`/`0x80` (`TiberiumGrows`/
// `TiberiumSpreads`) on the session and the growth-driver reload selector on
// `OreGrowthConfig`. A v130 skirmish save would restore them clear and reload the
// growth timer at the raw `Growth=` instead of `ftol(Growth * 0.3)`.
// v132 persists the `HouseClass+0x242` harvester no-ore latch on `HouseState`
// (set by `UnitClass::Mission_Harvest` state 0, never cleared). A v131 save
// would restore it clear for a house whose harvester had already exhausted
// its scan, and the state hash folds it.
// v133 persists the `HouseClass+0x57D4` funds-nag timer and the `[0xA8F040]`
// low-power guard on `HouseState` (`HouseClass::Update 0x004F8B3C..0x004F8DAB`).
// A v132 save would restore the timer at its constructor value and the guard
// clear, re-announcing "Low power" on load; the state hash folds both.
// v134 changes the bincode layout for repair-depot docking (GSI-07.38): the
// FIFO `ProductionState.depot_dock_reservations` is gone (native has no
// stored dock queue; admission is RadioClass contact capacity, BuildingClass
// ctor 0x0043BCBD) and `DockState.enter_retry` carries each waiter's
// `FootClass::Mission_Enter @ 0x004D9290` re-probe timer.
// v135 (GSI-09.01) appends three fields to `GameEntity`: the
// `BuildingClass+0x6D0/+0x6D8` ProduceCash timer (oil-derrick income) and the
// `TechnoClass+0x1CC/+0x1D0` drain link pair (Floating Disc money drain). They
// sit ahead of the `#[serde(skip)]` debug log, so a v134 record is short by
// their bytes and bincode would read the next entity's bytes as them; the hash
// schema also folds all three (`include_credit_income_v135`).
// v136 adds the retained Infantry terminal policy, independently of sprite
// animation progress. The entity layout and current deterministic hash change.
// v137 changes ordinary ground-coordinate resume invariants: Walk samples Z
// at its coordinate commits; Drive/Ship retain the last native paid sample,
// while their residual XY can already
// have changed the current cell, OnBridge, and track-relative offset. A v135
// mover may instead carry no exact Z and the previous cell/track frame. Its
// historic paid height cannot be reconstructed from the residual XY on load.
// Reject those saves instead of inventing an idle terrain sample. No fields or
// hash folds were added; see RAMP_UNIT_HEIGHT_GHIDRA_REPORT.md (4B1A96/4B253F).
// v138 combines both branches. The locally runnable v137 candidate did not
// contain infantry_terminal, so its serialized entity layout differs from this
// merged build. Reject v136/v137 saves instead of treating either as compatible.
// v139 persists Scenario+34A4 (map FreeRadar) in ScenarioSession. Bincode
// encodes fields positionally, so serde(default) cannot safely read v138 bytes.
// v140 pairs BridgeRuntimeState records with source Map Size for the native
// signed/clamped zone-node projection. Bincode positional layout changes;
// pre-v140 automatic-shell records also contain non-native invented spans.
// v141 retains CellClass400/800 alongside1180 and hashes live dummy gap flags.
// Old saves lack authoritative post-load gap state; do not invent it on restore.
// v142 retains viewer-owned knowledge, local/effective sight, pending conceal
// and stable generator admission receipts. New positional fields require rejection.
// v143 adds MCV pending deployment and the Drive rotation-edge latch.
// Bincode positional entity layout changes; reject older payloads.
// v144 retains Building6C8/Techno269/26C gap operational/deposit authority.
// Restoring cannot reclassify power or replay a fresh gap event.
// v145 adds literal CellClass+2C allocation identities to dynamic terrain facts.
// The self relation cannot reconstruct pointers preserved by overlapping stamps.
// v146 removes the duplicate BridgeRuntimeCell pavement Boolean. CellClass
// raw flags in dynamic terrain retain bit0x2000 for every allocated cell.
// v147 persists the Scenario+214 native numeric-ID cursor. A v146 save has no
// continuation for live constructors; the missing mid-record field cannot be
// defaulted safely by bincode. Original689310/689470 evidence: native_id_snapshot.
// v148 stores ordinary Drive/Ship destination/head Z in raw world leptons.
// v147 mixed level indices and raw ForceTrack/Tube coordinates; the lost
// original head heights cannot be reconstructed from a later terrain snapshot.
// v149 gives Drive and Ship the same retained signed track state. The prior
// Drive cursor was u16 and Ship had no independent residual/selector fields;
// bincode cannot infer those records across this ownership migration.
// v150 moves Foot+5E0/+558 replay from class payloads to NavigationState.
// Bincode field order changes even for owners without Drive/Ship instances.
// v151 moves applied speed and its existing query cache to the live Foot owner.
// v152 stores Foot occupation enable, pending fresh Apply1, and Ship head/handoff
// projection metadata alongside the existing serialized raw occupation plane.
// Drive END permission and the distinct Foot forced-swap gate are retained too.
// v156 persists current-house process input. Versions153-155 belong to
// the separate unmerged bridge locomotor layouts; do not accept those saves.
// v159 persists actual Cell membership/order, discovery history and the exact
// immutable Sight==0 predicate. Versions157-158 belong to the bridge branch.
// v160 moves the two Foot path timers, blocked latch and dword retry count
// from MovementTarget into persistent NavigationState.
// v161 adds the bridge locomotor instance state (Walk head/destination/moving
// bytes, Hover head, Jumpjet cached XYZ/moving/phase, including stashes) and
// raw Infantry house identities. None of the earlier branch-local layouts can
// be decoded as this combined schema.
// v163 adds the Jumpjet locomotor's linked type block and flight fields
// (facing, speed doubles, target height, bob phase) to its runtime payload.
// v165 adds DriveTrackState::before_first_point, which sits mid-record
// between point_index and residual. serde(default) does not help here -
// bincode cannot default a missing mid-record field - so a v164 save would
// otherwise decode residual, transform_flags and both head offsets from the
// wrong bytes instead of being rejected.
// v164 adds the per-cell AltObject air slots (CellClass+0xE0), the authority
// that keeps one hovering Jumpjet per cell.
// 165 -> 166: remove the duplicate HouseState credit balance; Economy owns cash.
// 166 -> 167: remove ordinary/forced geometry records; retained class track owns progress.
// Drive/Ship also own valid/turn-latched bytes; retire the MCV-only entity latch.
// 167 -> 168: retain signed Object+70 targeting reservations independently of HP.
// 168 -> 169 replaces cached u16 HP/max with signed actual Object health.
// The Health width is unchanged but its meaning differs. Building ART retains
// its native damaged-state latch and saves its 21 owned Anim slot references.
// 169 -> 170: aircraft dock indices widen from u8 to u32; save only reservation
// slots and rebuild their reverse lookup, rejecting duplicate occupants on load.
// 170 -> 171: save shared pixel-conversion bounds and retained HasEngineer.
// 171 -> 172: remove the refinery dock registry from ProductionState; the radio
// bus (entity contacts and the dock-entered flag) is the only contact record.
// 172 -> 173: teleport WarpOut, superweapon invoke and Lightning Storm
// animations are AnimStore members. The layout is unchanged, but those objects
// are now saved, hashed and take ids from the shared stable-id counter, so a
// 172 save taken after a teleport or storm resumes with different ids.
// 173 -> 174: remove the per-cell resource node map and the fallback ore
// overlay id from ProductionState, the node-era scanner and queue fields from
// OreGrowthState and the scan rate from OreGrowthConfig; the overlay grid is the
// only tiberium store.
// 174 -> 175: bridge collapse explosions are AnimStore members. The layout is
// unchanged, but they are now saved, hashed and take ids from the shared
// stable-id counter, so a 174 save taken during a collapse resumes differently.
// 175 -> 176: the effect asset catalog folded into `simulation_config_hash`
// now holds particle images only. The layout is unchanged, but a 175 save
// carries the old rules hash and would fail as a rules mismatch; the version
// gate names the real cause instead.
// 176 -> 177: AnimStore coordinates are world leptons on every axis. Anims a
// producer placed by height level used to store `level * 128`; they now store
// `level * 104`, and an attached anim's delta is taken from the owner's actual
// height. The layout is unchanged, but a 176 save with such an anim above
// level 0 would draw and hash it at the wrong height.
// 177 -> 178: weapon and occupant muzzle flashes are AnimStore members. The
// layout is unchanged, but they are now saved, hashed and take ids from the
// shared stable-id counter, so a 177 save taken mid-firefight resumes with
// different ids.
// 178 -> 179: a dropped object's parachute canopy is an AnimStore member
// attached to it. The layout is unchanged, but a 178 save taken during a
// paradrop resumes without canopies and with different ids.
// 179 -> 180: a Jumpjet's `air_phase` is no longer written as a mirror of its
// state field, and `AirMovePhase::Hovering`, which only that mirror produced, is
// gone, so the enum's encoding and a flying Jumpjet's stored value both differ
// from what a 179 save holds.
// 180 -> 181: Foot+580 crate speed multiplier is retained with the Foot owner,
// including across locomotor replacement. It is serialized and hashed.
// 181 -> 182: persistent DisplayClass layer membership and order. Its lookup
// cache is rebuilt, but history cannot be recovered from current coordinates.
// 182 -> 183: display vectors now include terrain, particle systems, bullets,
// waves and voxel debris. Prior snapshots cannot recover their registration history.
// 183 -> 184: animation display membership, retained instance YSortAdjust and
// Object+74 marking. Prior snapshots cannot reconstruct attachment history.
// 184 -> 185: Fly owns its integer height target and native takeoff/landing
// flags; the shared synthetic air phase, target and climb-rate fields are gone.
// 185 -> 186: aircraft retain signed Ammo and the pending-release byte outside
// the Attack variant. Duplicate mission flags and the fabricated release tail
// are removed; pending state cannot be recovered from the old representation.
// 186 -> 187: object burst position replaces AttackTarget's remaining-shot count.
// 187 -> 188: Fly retains exact destination XYZ, independent of the cell cache.
// 188 -> 189: retained Techno+3D4 cannot be recovered from live type or cargo.
// 189 -> 190: Foot+55C history and the combined retained wall/Foot Cell+122 plane.
// 190 -> 191: Fly retains its native cruise-mode byte through save and piggyback.
// 191 -> 192: Fly moving+34, the landing-effect latch +52, the linked
// AirportBound+18 and Techno+2E8 flight attitude are saved and hashed. A 191
// save cannot recover an in-progress landing effect or approach pitch.
// 192 -> 193: ParasiteClass (Foot+69C) and the victim's Foot+694 link,
// Foot+698 launch lock and Foot+6A0 paralysis timer are saved and hashed.
// 193 -> 194: Building+6E3 HasBeenCaptured is saved and hashed; a 193 save
// cannot tell a captured building from one built by its owner.
// 194 -> 195: death anims construct with the death producers' arguments
// (flags 0x600, zAdjust 0, the exact coordinate) and a building's per-cell
// `Explosion=` and `DestroyAnim=` anims are AnimStore members. The layout is
// unchanged, but a 194 save holding a death anim draws and hashes it with the
// impact's arguments, and one taken during a building's death resumes without
// its anims and with different ids.
// 195 -> 196: mind control: the CaptureManager keeps each node's original
// house and the overload countdown and voice latch; the victim keeps its
// controller and ring anim. A 195 save cannot tell whom a captive returns to.
// 196 -> 197: TemporalClass: a Temporal firer keeps its link (target, chain
// neighbours, WarpRemaining) and a warped object its chain head. A 196 save
// cannot tell a warped object from a free one.
// 197 -> 198: the house keeps its native defeat counts (`house_tracking`) in
// place of the two retired owned counts; an entity keeps its DontScore and
// tracking facts, and `owned_count_released` is renamed
// `destruction_recorded`. A 197 save holds none of the counts.
// 198 -> 199: a Crazy Ivan bomb: the carrier keeps its bomb (planter, house,
// fuse, who sees it); the carrier index is rebuilt on load. A 198 save has
// no bombs.
// 199 -> 200: Gattling: an entity keeps its stage and value (`+0x140`,
// `+0x144`) and the turret animation counter `+0x148`; the report latch is not
// saved (`TechnoClass::Load` clears it, `0x0070C20E`). A 199 save has none.
// 200 -> 201: an entity no longer keeps `last_attacker_id` (retaliation is the
// receiver's inline ShouldRetaliate), and a TeamType keeps `Suicide=`.
// 201 -> 202: the rearm countdown moves onto the object (`TechnoClass+0x2EC`):
// AttackTarget loses its cooldown/burst-delay counters and the cloak runtime
// its copy of the timer.
// 202 -> 203: the Simulation no longer keeps its own CloseEnough copy; the
// movement owners read `Rules+0x1718` (`[General] CloseEnough=`).
// 203 -> 204: a bullet keeps OnBridge and no owner house, a house its ROF
// bias (`HouseClass+0x1A8`), and a TeamType `Aggressive=`.
// 204 -> 205: a bullet keeps its `Arcing=` (`BulletTypeClass+0x29B`) and an
// anim its BounceClass body (`AnimClass+0x128`).
// 205 -> 206: a miner no longer keeps the Chrono Miner's retired dock phases
// (home refinery, dock-queued byte, dock phase, pivot facing, enter/approach/
// deploy timers, exit cell); an entity keeps Techno+0x1F8.
// 206 -> 207: a miner keeps Unit+0x6D2 and one +0xF8 StageClass (the unload
// counter renamed) instead of its target-ore cell, harvest timer, rescan
// cooldown and archive copy (the archive is Techno+0x218).
// 207 -> 208: an entity keeps the crash latch and its AI edge (Foot+0x425/
// +0x426) and a Fly its fall counter (+0x58).
// 208 -> 209: a master keeps its SlaveManagerClass (Techno+0x2D8) and a slave
// its SlaveOwner (+0x2DC) and Storage, replacing the production slave
// bindings and the slave harvester cursor.
// 209 -> 210: a locomotor no longer keeps the retired copy of the Jumpjet type
// block (speed, accel, current speed, deviation, crash speed, turn rate).
const SNAPSHOT_VERSION: u32 = 210;

const SNAPSHOT_PRODUCT_MAGIC: [u8; 8] = *b"VERA20K\0";
const SNAPSHOT_ENVELOPE_VERSION: u32 = 1;

/// Binary snapshot envelope — wraps the full `Simulation` state plus
/// compatibility hashes for the map and rules that were active at save time.
#[derive(Serialize, Deserialize)]
pub struct GameSnapshot {
    /// Stable VERA snapshot identity, independent of the file name.
    pub product_magic: [u8; 8],
    /// Public envelope contract. This changes only when the outer prefix does.
    pub envelope_version: u32,
    /// Format version — checked on load to reject incompatible saves.
    pub version: u32,
    /// Player-authored save description used by the load-game list.
    pub description: String,
    /// Hash of the map file — caller verifies on load to ensure same map.
    pub map_hash: u64,
    /// Hash of the merged rules — caller verifies on load to ensure same rules.
    pub rules_hash: u64,
    /// Simulation tick at save time — stored in header for quick preview.
    pub tick: u64,
    /// Unix timestamp (seconds) when the save was created.
    pub save_timestamp: u64,
    /// Map name at save time — stored in header for quick preview.
    pub map_name: String,
    /// The full authoritative simulation state (caches excluded via serde skip).
    pub sim: Simulation,
}

/// Lightweight header extracted from a save file without deserializing the
/// full `Simulation`. Fields are laid out in the same order as `GameSnapshot`
/// so bincode can decode them as a prefix.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameSnapshotHeader {
    pub product_magic: [u8; 8],
    pub envelope_version: u32,
    pub version: u32,
    pub description: String,
    pub map_hash: u64,
    pub rules_hash: u64,
    pub tick: u64,
    pub save_timestamp: u64,
    pub map_name: String,
}

#[derive(Serialize, Deserialize)]
struct GameSnapshotPreamble {
    product_magic: [u8; 8],
    envelope_version: u32,
    version: u32,
}

/// Errors that can occur during snapshot deserialization.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot product identity {found:?} is not a VERA20k save")]
    ProductMismatch { found: [u8; 8] },
    #[error("snapshot envelope version {found} does not match expected {expected}")]
    EnvelopeVersionMismatch { expected: u32, found: u32 },
    #[error("snapshot version {found} does not match expected {expected}")]
    VersionMismatch { expected: u32, found: u32 },
    #[error("map hash {found:#018x} does not match active map {expected:#018x}")]
    MapMismatch { expected: u64, found: u64 },
    #[error("rules hash {found:#018x} does not match active rules {expected:#018x}")]
    RulesMismatch { expected: u64, found: u64 },
    #[error("snapshot map name {found:?} does not match active map {expected:?}")]
    MapNameMismatch { expected: String, found: String },
    #[error(
        "snapshot header tick {header_tick} does not match serialized simulation tick {simulation_tick}"
    )]
    TickMetadataMismatch {
        header_tick: u64,
        simulation_tick: u64,
    },
    #[error(
        "snapshot header map name {header_map_name:?} does not match serialized session map name {simulation_map_name:?}"
    )]
    MapNameMetadataMismatch {
        header_map_name: String,
        simulation_map_name: String,
    },
    #[error("deserialization failed: {0}")]
    DeserializeFailed(#[from] bincode::Error),
}

/// Structural failures found before a deserialized simulation is admitted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotRestoreError {
    #[error("non-Building entity {owner_id} retains animation slot {slot}")]
    InvalidBuildingAnimSlotOwner { owner_id: u64, slot: u8 },
    #[error("Building {owner_id} slot {slot} references missing AnimStore object {anim_id}")]
    MissingBuildingSlotAnim {
        owner_id: u64,
        slot: u8,
        anim_id: u64,
    },
    #[error(
        "animation {anim_id} belongs to both Building {first_owner} slot {first_slot} and Building {second_owner} slot {second_slot}"
    )]
    DuplicateBuildingSlotAnim {
        anim_id: u64,
        first_owner: u64,
        first_slot: u8,
        second_owner: u64,
        second_slot: u8,
    },
    #[error("invalid saved Cell list: {reason}")]
    InvalidCellMembership { reason: String },
    #[error("saved current house {owner} is absent from the HouseClass registry")]
    InvalidCurrentHouse {
        owner: crate::sim::intern::InternedId,
    },
    #[error("entity {object_id} has an invalid or incomplete Infantry terminal handoff")]
    InvalidInfantryTerminalState { object_id: u64 },
    #[error("house {owner} has invalid serialized outcome state: {reason}")]
    InvalidHouseOutcomeState {
        owner: crate::sim::intern::InternedId,
        reason: &'static str,
    },
    #[error("{registry} contains reserved object id 0")]
    ReservedObjectId { registry: &'static str },
    #[error(
        "{registry} registry key {registry_id} does not match the object's serialized id {object_id}"
    )]
    ObjectIdentityMismatch {
        registry: &'static str,
        registry_id: u64,
        object_id: u64,
    },
    #[error("object id {object_id} is registered by both {first_registry} and {second_registry}")]
    DuplicateObjectIdentity {
        object_id: u64,
        first_registry: &'static str,
        second_registry: &'static str,
    },
    #[error("next object id {next_id} is not after the highest restored object id {highest_id}")]
    ObjectIdCounterBehind { next_id: u64, highest_id: u64 },
    #[error(
        "next occupancy-enter order {next_order} is not after the highest restored order {highest_order}"
    )]
    OccupancyOrderCounterBehind { next_order: u64, highest_order: u64 },
    #[error("LogicVector contains duplicate object id {object_id}")]
    DuplicateLogicIdentity { object_id: u64 },
    #[error("LogicVector object id {object_id} has no restored registry identity")]
    MissingLogicIdentity { object_id: u64 },
    #[error("DisplayClass object id {object_id} has no restored registry identity")]
    MissingDisplayIdentity { object_id: u64 },
    #[error("live {registry} object id {object_id} is absent from LogicVector")]
    MissingRequiredLogicIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("inactive {registry} object id {object_id} remains in LogicVector")]
    InactiveLogicIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("terminal {registry} object id {object_id} is absent from PendingDeleteList")]
    MissingDeferredDeleteIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error("PendingDeleteList {registry} object id {object_id} remains in LogicVector")]
    DeferredDeleteLogicIdentity {
        registry: &'static str,
        object_id: u64,
    },
    #[error(
        "{source_registry} object {source_id} field {field} references missing {target_registry} object {target_id}"
    )]
    UnresolvedObjectReference {
        source_registry: &'static str,
        source_id: u64,
        field: &'static str,
        target_registry: &'static str,
        target_id: u64,
    },
    #[error(
        "carrier {carrier_id} has {passenger_count} passengers but {size_count} saved Size entries"
    )]
    PassengerSizeCountMismatch {
        carrier_id: u64,
        passenger_count: usize,
        size_count: usize,
    },
    #[error(
        "carrier {carrier_id} saved total Size {saved_total}, but its entry sizes sum to {computed_total}"
    )]
    PassengerSizeTotalMismatch {
        carrier_id: u64,
        saved_total: u32,
        computed_total: u64,
    },
    #[error(
        "entity {object_id} has active MoveSound state but no restorable configured sound identity"
    )]
    ActiveMoveSoundUnresolvable { object_id: u64 },
    #[error("snapshot restore requires the {component}")]
    MissingMapAuthorityComponent { component: &'static str },
    #[error(
        "snapshot overlay grid is {overlay_width}x{overlay_height}, but restored terrain is {terrain_width}x{terrain_height}"
    )]
    MapAuthorityDimensionMismatch {
        overlay_width: u16,
        overlay_height: u16,
        terrain_width: u16,
        terrain_height: u16,
    },
    #[error(
        "snapshot overlay grid storage has {found} cells, but its dimensions require {expected}"
    )]
    MapAuthorityCellStorageMismatch { expected: usize, found: usize },
    #[error(
        "snapshot retained wall-neighbor storage has {found} cells, but its dimensions require {expected}"
    )]
    RetainedWallNeighborStorageMismatch { expected: usize, found: usize },
    #[error(
        "snapshot overlay grid carries no retained wall-neighbor plane; every current-version map authority owns one"
    )]
    MissingRetainedWallNeighborPlane,
    #[error("snapshot real-cell bridge flags do not match restored CellClass allocation")]
    RealCellBridgeFlagAuthorityMismatch,
    #[error(
        "snapshot dynamic terrain cell ({rx},{ry}) is absent from restored CellClass allocation"
    )]
    DynamicTerrainCellMissing { rx: u16, ry: u16 },
}

/// Derived facts produced by the transactional map-authority restore seam.
#[derive(Debug)]
pub(crate) struct SnapshotMapRestoreOutput {
    pub occupied_overlays: Vec<crate::map::overlay::OverlayEntry>,
    pub native_tiberium_stats: crate::sim::ore_growth::NativeTiberiumRebuildStats,
}

/// Internal borrow-based envelope for serialization (avoids cloning Simulation).
#[derive(Serialize)]
struct GameSnapshotRef<'a> {
    product_magic: [u8; 8],
    envelope_version: u32,
    version: u32,
    description: String,
    map_hash: u64,
    rules_hash: u64,
    tick: u64,
    save_timestamp: u64,
    map_name: String,
    sim: &'a Simulation,
}

impl GameSnapshot {
    fn serialize(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        map_name: &str,
        description: &str,
        save_timestamp: u64,
    ) -> Vec<u8> {
        // Retail provenance: Save_Game_To_File @ 0x0067CEF0 supplies a distinct
        // outer file identity; Write_Savegame_Metadata_To_Storage @ 0x006812E0
        // writes public Version=1, exact internal version, and Scenario
        // Description. The active list admits only an exact internal-version
        // match at 0x00559ED0..0x0055A04A. VERA keeps its Rust-native bincode
        // body while making the same load-bearing envelope identities explicit.
        let snapshot = GameSnapshotRef {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            version: SNAPSHOT_VERSION,
            description: description.to_string(),
            map_hash,
            rules_hash,
            tick: sim.session.tick,
            save_timestamp,
            map_name: map_name.to_string(),
            sim,
        };
        bincode::serialize(&snapshot).expect("snapshot serialization should not fail")
    }

    /// Serialize a production save with metadata owned by the active session.
    ///
    /// Map/rules digests come from the app's loaded content owners. Tick and map
    /// name are copied from `ScenarioSession`, so the envelope cannot disagree
    /// with the authoritative body. The app supplies the wall-clock timestamp;
    /// sim code remains clock-independent.
    pub fn save_validated(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        description: &str,
        save_timestamp: u64,
    ) -> Vec<u8> {
        Self::serialize(
            sim,
            map_hash,
            rules_hash,
            &sim.session.map_name,
            description,
            save_timestamp,
        )
    }

    /// Test-only constructor for deliberately synthetic envelope metadata.
    #[cfg(test)]
    pub(crate) fn save(
        sim: &Simulation,
        map_hash: u64,
        rules_hash: u64,
        map_name: &str,
        save_timestamp: u64,
    ) -> Vec<u8> {
        Self::serialize(
            sim,
            map_hash,
            rules_hash,
            map_name,
            map_name,
            save_timestamp,
        )
    }

    /// Deserialize a current-version snapshot without content validation.
    ///
    /// This exists for internal tests and diagnostics. Production restoration
    /// must use [`Self::load_validated`]. Like retail's post-read Scenario
    /// reinitializer, full deserialization resets the embedded Scenario RNG to
    /// `Random__Seed(0)` even though its saved bytes remain in the wire layout.
    #[cfg(test)]
    pub(crate) fn load_unchecked(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        let _ = Self::read_header(bytes)?;
        Ok(bincode::deserialize(bytes)?)
    }

    /// Compatibility alias for existing unit fixtures that intentionally use
    /// synthetic zero hashes and header-only map names.
    #[cfg(test)]
    pub(crate) fn load(bytes: &[u8]) -> Result<GameSnapshot, SnapshotError> {
        Self::load_unchecked(bytes)
    }

    /// Deserialize only when the save belongs to the exact active content.
    ///
    /// Version and compatibility metadata are rejected before the simulation
    /// body is admitted. The duplicated preview metadata must also agree with
    /// `ScenarioSession`; no warning/continue or zero-hash sentinel exists.
    /// The returned Scenario RNG is the canonical seed-zero state. Main/MapGen
    /// fields are deserialize placeholders; the app's in-scenario production
    /// load seam replaces them with the live process cursors and seed.
    pub fn load_validated(
        bytes: &[u8],
        expected_map_hash: u64,
        expected_rules_hash: u64,
        expected_map_name: &str,
    ) -> Result<GameSnapshot, SnapshotError> {
        let header = Self::read_header(bytes)?;
        if header.map_hash != expected_map_hash {
            return Err(SnapshotError::MapMismatch {
                expected: expected_map_hash,
                found: header.map_hash,
            });
        }
        if header.rules_hash != expected_rules_hash {
            return Err(SnapshotError::RulesMismatch {
                expected: expected_rules_hash,
                found: header.rules_hash,
            });
        }
        if header.map_name != expected_map_name {
            return Err(SnapshotError::MapNameMismatch {
                expected: expected_map_name.to_string(),
                found: header.map_name,
            });
        }

        let snapshot: GameSnapshot = bincode::deserialize(bytes)?;
        if snapshot.tick != snapshot.sim.session.tick {
            return Err(SnapshotError::TickMetadataMismatch {
                header_tick: snapshot.tick,
                simulation_tick: snapshot.sim.session.tick,
            });
        }
        if snapshot.map_name != snapshot.sim.session.map_name {
            return Err(SnapshotError::MapNameMetadataMismatch {
                header_map_name: snapshot.map_name,
                simulation_map_name: snapshot.sim.session.map_name,
            });
        }
        Ok(snapshot)
    }

    /// Read only the header fields from a save file without deserializing the
    /// full Simulation. Useful for listing saves in the UI.
    pub fn read_header(bytes: &[u8]) -> Result<GameSnapshotHeader, SnapshotError> {
        let preamble: GameSnapshotPreamble = bincode::deserialize(bytes)?;
        if preamble.product_magic != SNAPSHOT_PRODUCT_MAGIC {
            return Err(SnapshotError::ProductMismatch {
                found: preamble.product_magic,
            });
        }
        if preamble.envelope_version != SNAPSHOT_ENVELOPE_VERSION {
            return Err(SnapshotError::EnvelopeVersionMismatch {
                expected: SNAPSHOT_ENVELOPE_VERSION,
                found: preamble.envelope_version,
            });
        }
        if preamble.version != SNAPSHOT_VERSION {
            return Err(SnapshotError::VersionMismatch {
                expected: SNAPSHOT_VERSION,
                found: preamble.version,
            });
        }
        Ok(bincode::deserialize(bytes)?)
    }
}

#[derive(Debug, Clone, Copy)]
struct RestoredObjectIndex {
    highest_id: u64,
}

impl RestoredObjectIndex {
    fn build(
        sim: &Simulation,
    ) -> Result<(Self, BTreeMap<u64, &'static str>), SnapshotRestoreError> {
        let mut identities = BTreeMap::new();
        let mut highest_id = 0;

        for (registry_id, entity) in sim.substrate.entities.iter_sorted() {
            Self::register(
                &mut identities,
                "EntityStore",
                registry_id,
                entity.stable_id(),
            )?;
            highest_id = highest_id.max(registry_id);
        }
        for (&registry_id, anim) in sim.substrate.anims.iter() {
            Self::register(&mut identities, "AnimStore", registry_id, anim.stable_id)?;
            highest_id = highest_id.max(registry_id);
        }
        for (&registry_id, debris) in sim.substrate.voxel_anims.iter() {
            Self::register(
                &mut identities,
                "VoxelAnimStore",
                registry_id,
                debris.stable_id,
            )?;
            highest_id = highest_id.max(registry_id);
        }
        for (&registry_id, system) in sim.substrate.particle_systems.iter() {
            Self::register(
                &mut identities,
                "ParticleSystemStore",
                registry_id,
                system.stable_id,
            )?;
            highest_id = highest_id.max(registry_id);
        }
        for (&registry_id, terrain) in &sim.production.terrain_objects {
            Self::register(
                &mut identities,
                "TerrainObjectStore",
                registry_id,
                terrain.stable_id,
            )?;
            highest_id = highest_id.max(registry_id);
        }
        for (&registry_id, projectile) in sim.projectiles.iter() {
            Self::register(
                &mut identities,
                "ProjectileStore",
                registry_id,
                projectile.id,
            )?;
            highest_id = highest_id.max(registry_id);
        }
        for (&registry_id, wave) in sim.waves.iter() {
            Self::register(&mut identities, "WaveStore", registry_id, wave.id)?;
            highest_id = highest_id.max(registry_id);
        }

        Ok((Self { highest_id }, identities))
    }

    fn register(
        identities: &mut BTreeMap<u64, &'static str>,
        registry: &'static str,
        registry_id: u64,
        object_id: u64,
    ) -> Result<(), SnapshotRestoreError> {
        if registry_id == 0 {
            return Err(SnapshotRestoreError::ReservedObjectId { registry });
        }
        if registry_id != object_id {
            return Err(SnapshotRestoreError::ObjectIdentityMismatch {
                registry,
                registry_id,
                object_id,
            });
        }
        if let Some(first_registry) = identities.insert(registry_id, registry) {
            return Err(SnapshotRestoreError::DuplicateObjectIdentity {
                object_id: registry_id,
                first_registry,
                second_registry: registry,
            });
        }
        Ok(())
    }
}

fn require_resolved_reference(
    resolves: bool,
    source_registry: &'static str,
    source_id: u64,
    field: &'static str,
    target_registry: &'static str,
    target_id: u64,
) -> Result<(), SnapshotRestoreError> {
    if resolves {
        Ok(())
    } else {
        Err(SnapshotRestoreError::UnresolvedObjectReference {
            source_registry,
            source_id,
            field,
            target_registry,
            target_id,
        })
    }
}

fn validate_nav_reference(
    target: &crate::sim::components::NavTargetRef,
    identities: &BTreeMap<u64, &'static str>,
    entity_ids: &BTreeSet<u64>,
    source_id: u64,
    field: &'static str,
) -> Result<(), SnapshotRestoreError> {
    match target {
        crate::sim::components::NavTargetRef::Cell { .. } => Ok(()),
        crate::sim::components::NavTargetRef::Entity { id }
        | crate::sim::components::NavTargetRef::Building { id } => require_resolved_reference(
            entity_ids.contains(id),
            "EntityStore",
            source_id,
            field,
            "EntityStore",
            *id,
        ),
        crate::sim::components::NavTargetRef::Object { id } => require_resolved_reference(
            identities.contains_key(id),
            "EntityStore",
            source_id,
            field,
            "object namespace",
            *id,
        ),
    }
}

fn validate_passenger_size_tables(sim: &Simulation) -> Result<(), SnapshotRestoreError> {
    for (carrier_id, entity) in sim.substrate.entities.iter_sorted() {
        let crate::sim::passenger::PassengerRole::Transport { cargo } = &entity.passenger_role
        else {
            continue;
        };
        if cargo.passengers.len() != cargo.passenger_sizes.len() {
            return Err(SnapshotRestoreError::PassengerSizeCountMismatch {
                carrier_id,
                passenger_count: cargo.passengers.len(),
                size_count: cargo.passenger_sizes.len(),
            });
        }
        let computed_total: u64 = cargo
            .passenger_sizes
            .iter()
            .map(|&size| u64::from(size))
            .sum();
        if computed_total != u64::from(cargo.total_size) {
            return Err(SnapshotRestoreError::PassengerSizeTotalMismatch {
                carrier_id,
                saved_total: cargo.total_size,
                computed_total,
            });
        }
    }
    Ok(())
}

fn restore_object_references(
    sim: &mut Simulation,
    identities: &BTreeMap<u64, &'static str>,
) -> Result<(), SnapshotRestoreError> {
    use crate::sim::combat::TargetKind;
    use crate::sim::game_entity::BunkerLink;
    use crate::sim::movement::homing_movement::HomingTarget;
    use crate::sim::passenger::PassengerRole;
    use crate::sim::projectile::ProjectileTarget;

    let entity_ids: BTreeSet<u64> = sim.substrate.entities.keys_sorted().into_iter().collect();
    let anim_ids: BTreeSet<u64> = sim.substrate.anims.iter().map(|(&id, _)| id).collect();
    let particle_system_ids: BTreeSet<u64> = sim
        .substrate
        .particle_systems
        .iter()
        .map(|(&id, _)| id)
        .collect();

    // Building saved+55C slots swizzle to distinct owned Anim objects. Validate
    // before weak-pointer cleanup or reverse-index mutation. Anim+CC may remain
    // null, and a deferred Destroy is still a physically present valid target.
    let mut claimed_slots = BTreeMap::new();
    for (owner_id, entity) in sim.substrate.entities.iter_sorted() {
        // Diagnostic slots0..20 are Building+55C, slots21..28 are +5C8.
        for (slot, anim_id) in entity
            .building_anim_slots
            .iter()
            .chain(entity.damage_fire_anim_ids.iter())
            .enumerate()
        {
            let Some(anim_id) = *anim_id else { continue };
            let slot = slot as u8;
            if entity.category != crate::map::entities::EntityCategory::Structure {
                return Err(SnapshotRestoreError::InvalidBuildingAnimSlotOwner { owner_id, slot });
            }
            if !anim_ids.contains(&anim_id) {
                return Err(SnapshotRestoreError::MissingBuildingSlotAnim {
                    owner_id,
                    slot,
                    anim_id,
                });
            }
            if let Some((first_owner, first_slot)) = claimed_slots.insert(anim_id, (owner_id, slot))
            {
                return Err(SnapshotRestoreError::DuplicateBuildingSlotAnim {
                    anim_id,
                    first_owner,
                    first_slot,
                    second_owner: owner_id,
                    second_slot: slot,
                });
            }
        }
    }

    // Swizzle::Apply has no unmatched-reference recovery path. Validate the
    // complete modeled pointer graph before mutating even weak references or
    // derived caches, so a failed restore remains an atomic rejection.
    for (entity_id, entity) in sim.substrate.entities.iter_sorted() {
        for contact_id in entity.radio_contacts.iter_live() {
            require_resolved_reference(
                entity_ids.contains(&contact_id),
                "EntityStore",
                entity_id,
                "radio_contacts",
                "EntityStore",
                contact_id,
            )?;
        }

        if let PassengerRole::Transport { cargo } = &entity.passenger_role {
            for &passenger_id in &cargo.passengers {
                require_resolved_reference(
                    entity_ids.contains(&passenger_id),
                    "EntityStore",
                    entity_id,
                    "passenger_role.cargo.passengers",
                    "EntityStore",
                    passenger_id,
                )?;
            }
        }

        // SpawnManagerClass__Load @ 0x006B7F10 queues every non-null
        // SpawnControl child pointer plus its current and queued target slots
        // for the common post-load swizzle pass. Rust keeps stable IDs in those
        // fields, so admission requires the same references to resolve before
        // any cleanup mutates the restored graph.
        if let Some(manager) = entity.spawn_manager.as_ref() {
            for slot in &manager.slots {
                if let Some(spawn_id) = slot.spawn {
                    require_resolved_reference(
                        entity_ids.contains(&spawn_id),
                        "EntityStore",
                        entity_id,
                        "spawn_manager.slots.spawn",
                        "EntityStore",
                        spawn_id,
                    )?;
                }
            }
            if let Some(TargetKind::Entity(target_id)) = manager.current_target {
                require_resolved_reference(
                    entity_ids.contains(&target_id),
                    "EntityStore",
                    entity_id,
                    "spawn_manager.current_target",
                    "EntityStore",
                    target_id,
                )?;
            }
            if let Some(TargetKind::Entity(target_id)) = manager.queued_target {
                require_resolved_reference(
                    entity_ids.contains(&target_id),
                    "EntityStore",
                    entity_id,
                    "spawn_manager.queued_target",
                    "EntityStore",
                    target_id,
                )?;
            }
        }

        // TechnoClass__Load @ 0x0070BF50 queues the spawned-child parent
        // pointer at Techno+0x2D4. It is an independently swizzled slot: native
        // load does not require a reciprocal SpawnControl entry.
        if let Some(parent_id) = entity.spawn_owner_id {
            require_resolved_reference(
                entity_ids.contains(&parent_id),
                "EntityStore",
                entity_id,
                "spawn_owner_id",
                "EntityStore",
                parent_id,
            )?;
        }

        // A bomb's planter (`BombClass+0x24`) is a pointer its expiry nulls
        // (`BombListClass::PointerGotInvalid @ 0x00439150`).
        if let Some(planter) = entity.bomb.and_then(|bomb| bomb.planter()) {
            require_resolved_reference(
                entity_ids.contains(&planter),
                "EntityStore",
                entity_id,
                "bomb.planter",
                "EntityStore",
                planter,
            )?;
        }

        if let Some(TargetKind::Entity(target_id)) =
            entity.attack_target.as_ref().map(|target| target.target)
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "attack_target",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(TargetKind::Entity(target_id)) = entity.suspended_attack_target {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "suspended_attack_target",
                "EntityStore",
                target_id,
            )?;
        }

        if let Some(target) = entity.navigation.suspended_nav_com.as_ref() {
            validate_nav_reference(
                target,
                identities,
                &entity_ids,
                entity_id,
                "navigation.suspended_nav_com",
            )?;
        }
        if let Some(target) = entity.navigation.nav_com_aux.as_ref() {
            validate_nav_reference(
                target,
                identities,
                &entity_ids,
                entity_id,
                "navigation.nav_com_aux",
            )?;
        }
        if let Some(target) = entity.navigation.nav_com.as_ref() {
            validate_nav_reference(
                target,
                identities,
                &entity_ids,
                entity_id,
                "navigation.nav_com",
            )?;
        }
        for target in &entity.navigation.nav_queue {
            validate_nav_reference(
                target,
                identities,
                &entity_ids,
                entity_id,
                "navigation.nav_queue",
            )?;
        }

        if let Some(target_id) = entity.dock_entered_with {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "dock_entered_with",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(target_id) = entity.capture_target {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "capture_target",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(manager) = entity.capture_manager.as_ref() {
            for target_id in manager.victims() {
                require_resolved_reference(
                    entity_ids.contains(&target_id),
                    "EntityStore",
                    entity_id,
                    "capture_manager.nodes",
                    "EntityStore",
                    target_id,
                )?;
            }
        }
        if let Some(controller_id) = entity.mind_control.controller() {
            require_resolved_reference(
                entity_ids.contains(&controller_id),
                "EntityStore",
                entity_id,
                "mind_control.controller",
                "EntityStore",
                controller_id,
            )?;
        }
        for target_id in entity.temporal.references() {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "temporal",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(plant) = entity.c4_plant.as_ref() {
            require_resolved_reference(
                entity_ids.contains(&plant.target_building_id),
                "EntityStore",
                entity_id,
                "c4_plant.target_building_id",
                "EntityStore",
                plant.target_building_id,
            )?;
        }

        if let Some(dock) = entity.dock_state.as_ref() {
            require_resolved_reference(
                entity_ids.contains(&dock.dock_building_id),
                "EntityStore",
                entity_id,
                "dock_state.dock_building_id",
                "EntityStore",
                dock.dock_building_id,
            )?;
        }
        if let Some(ammo) = entity.aircraft_ammo.as_ref()
            && let Some(target_id) = ammo.target_airfield
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "aircraft_ammo.target_airfield",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(target_id) = entity
            .miner
            .as_ref()
            .and_then(|miner| miner.reserved_refinery)
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "miner.reserved_refinery",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(master_id) = entity.slave.owner() {
            require_resolved_reference(
                entity_ids.contains(&master_id),
                "EntityStore",
                entity_id,
                "slave.owner",
                "EntityStore",
                master_id,
            )?;
        }
        if let Some(manager) = entity.slave_manager.as_ref() {
            for slave_id in manager.slaves() {
                require_resolved_reference(
                    entity_ids.contains(&slave_id),
                    "EntityStore",
                    entity_id,
                    "slave_manager.nodes.slave",
                    "EntityStore",
                    slave_id,
                )?;
            }
        }

        let passenger_partner = match &entity.passenger_role {
            PassengerRole::Boarding {
                target_transport_id,
                ..
            } => Some((*target_transport_id, "passenger_role.boarding")),
            PassengerRole::Inside { transport_id } => {
                Some((*transport_id, "passenger_role.inside"))
            }
            PassengerRole::None | PassengerRole::Transport { .. } => None,
        };
        if let Some((target_id, field)) = passenger_partner {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                field,
                "EntityStore",
                target_id,
            )?;
        }

        if let Some(homing) = entity.homing_state.as_ref()
            && let Some(HomingTarget::Object(target_id)) = homing.target
        {
            require_resolved_reference(
                identities.contains_key(&target_id),
                "EntityStore",
                entity_id,
                "homing_state.target",
                "object namespace",
                target_id,
            )?;
        }

        if let Some(system_id) = entity.damage_smoke_system_id {
            require_resolved_reference(
                particle_system_ids.contains(&system_id),
                "EntityStore",
                entity_id,
                "damage_smoke_system_id",
                "ParticleSystemStore",
                system_id,
            )?;
        }
        if let Some(target_id) = entity.bunker_occupant {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_occupant",
                "EntityStore",
                target_id,
            )?;
        }
        if let BunkerLink::Approaching(target_id) | BunkerLink::Installed(target_id) =
            entity.bunker_link
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_link",
                "EntityStore",
                target_id,
            )?;
        }
        if let Some(runtime) = entity.bunker_runtime.as_ref()
            && let Some(target_id) = runtime.installing_unit
        {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "EntityStore",
                entity_id,
                "bunker_runtime.installing_unit",
                "EntityStore",
                target_id,
            )?;
        }
    }

    for (&anim_id, anim) in sim.substrate.anims.iter() {
        if let Some(owner_id) = anim.owner_entity {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "AnimStore",
                anim_id,
                "owner_entity",
                "EntityStore",
                owner_id,
            )?;
        }
    }

    for (&system_id, system) in sim.substrate.particle_systems.iter() {
        if let Some(owner_id) = system.owner_entity {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "ParticleSystemStore",
                system_id,
                "owner_entity",
                "EntityStore",
                owner_id,
            )?;
        }
        if let Some(attached_id) = system.attached_entity {
            require_resolved_reference(
                entity_ids.contains(&attached_id),
                "ParticleSystemStore",
                system_id,
                "attached_entity",
                "EntityStore",
                attached_id,
            )?;
        }
    }

    // BulletClass__Load @ 0x0046AE70 queues the non-null source/firer pointer
    // at +0xB0 and target pointer at +0x10C for global swizzling. VERA's zero
    // source sentinel and non-entity Cell/null targets are the corresponding
    // null/non-pointer representations and therefore need no object lookup.
    for (&projectile_id, projectile) in sim.projectiles.iter() {
        if projectile.source_id != crate::sim::combat::RAD_NO_ATTACKER {
            require_resolved_reference(
                entity_ids.contains(&projectile.source_id),
                "ProjectileStore",
                projectile_id,
                "source_id",
                "EntityStore",
                projectile.source_id,
            )?;
        }
        if let ProjectileTarget::Entity(target_id) = projectile.target {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "ProjectileStore",
                projectile_id,
                "target",
                "EntityStore",
                target_id,
            )?;
        }
    }

    let wave_ids = sim.waves.iter().map(|(&id, _)| id).collect::<BTreeSet<_>>();
    for (&wave_id, wave) in sim.waves.iter() {
        if let Some(owner_id) = wave.owner_id {
            require_resolved_reference(
                entity_ids.contains(&owner_id),
                "WaveStore",
                wave_id,
                "owner_id",
                "EntityStore",
                owner_id,
            )?;
        }
        if let Some(TargetKind::Entity(target_id)) = wave.target_ref {
            require_resolved_reference(
                entity_ids.contains(&target_id),
                "WaveStore",
                wave_id,
                "target_ref",
                "EntityStore",
                target_id,
            )?;
        }
    }
    for (&owner_id, &wave_id) in &sim.active_wave_links {
        require_resolved_reference(
            entity_ids.contains(&owner_id),
            "ActiveWaveLinks",
            owner_id,
            "owner",
            "EntityStore",
            owner_id,
        )?;
        require_resolved_reference(
            wave_ids.contains(&wave_id)
                && sim.waves.get(wave_id).and_then(|wave| wave.owner_id) == Some(owner_id),
            "ActiveWaveLinks",
            owner_id,
            "wave",
            "WaveStore",
            wave_id,
        )?;
    }

    for &object_id in &sim.substrate.pending_delete {
        require_resolved_reference(
            identities.contains_key(&object_id),
            "PendingDeleteList",
            object_id,
            "entry",
            "object namespace",
            object_id,
        )?;
    }

    for factory in sim.production.factory_shadow.iter_insertion_ordered() {
        if let Some(object_id) = factory.object.as_ref().and_then(|object| object.entity_id) {
            require_resolved_reference(
                entity_ids.contains(&object_id),
                "FactoryRegistry",
                u64::from(factory.owner.index()),
                "object.entity_id",
                "EntityStore",
                object_id,
            )?;
        }
    }

    // A deliberate weak identity: C4 kill credit may outlive its attacker.
    for entity in sim.substrate.entities.values_mut() {
        if let Some(pending) = entity.pending_c4_detonation.as_mut()
            && pending
                .source_entity_id
                .is_some_and(|id| !entity_ids.contains(&id))
        {
            pending.source_entity_id = None;
        }
    }

    // These manager maps are derived/transitional mirrors rather than modeled
    // native pointer slots. Restore prunes them only after the authoritative
    // object graph has passed validation.
    for producers in sim.production.active_producer_by_owner.values_mut() {
        producers.retain(|_, id| entity_ids.contains(id));
    }
    sim.production.airfield_docks.cleanup_dead(&entity_ids);

    // The produced-object link was validated above, so this legacy helper is
    // intentionally a no-op for every admitted snapshot.
    sim.production
        .factory_shadow
        .fixup_object_references(&entity_ids);

    Ok(())
}

impl Simulation {
    /// Validate and re-register a deserialized simulation before it can resume.
    ///
    /// Native load registers old identities and pointer slots, resolves them,
    /// then restores global active/cell membership. Rust stores stable IDs
    /// directly, so the equivalent transaction builds one global namespace,
    /// rejects any unmatched modeled native pointer, cleans only deliberate
    /// weak/derived identities, and reconstructs skipped indexes in dependency
    /// order.
    pub(crate) fn restore_after_snapshot_load(&mut self) -> Result<(), SnapshotRestoreError> {
        if let Some(owner) = self.session.current_house {
            if !self.houses.contains_key(&owner) || !self.session.house_order.contains(&owner) {
                return Err(SnapshotRestoreError::InvalidCurrentHouse { owner });
            }
        }
        // VERA-internal terminal admission invariant. UnInit clears the policy
        // before marking the object dead; pending deletion needs no new visit.
        // A borrowed consequence packet cannot survive a save boundary.
        // Logic membership is rebuilt later. A retained limbo object may carry
        // a terminal policy until its owner releases it back into Logic.
        let terminal_logic_members: std::collections::BTreeSet<_> =
            self.substrate.logic.snapshot().into_iter().collect();
        for (object_id, entity) in self.substrate.entities.iter_sorted() {
            let infantry = entity.category == crate::map::entities::EntityCategory::Infantry;
            if (entity.infantry_terminal.is_some()
                && (!infantry
                    || !entity.dying
                    || !entity.lifecycle.object_alive
                    || (!entity.lifecycle.in_limbo
                        && !terminal_logic_members.contains(&object_id))))
                || entity.infantry_terminal
                    == Some(crate::sim::world::InfantryTerminal::AwaitingConsequences)
                || (infantry
                    && entity.dying
                    && entity.lifecycle.object_alive
                    && entity.infantry_terminal.is_none())
            {
                return Err(SnapshotRestoreError::InvalidInfantryTerminalState { object_id });
            }
        }
        for (&owner, house) in &self.houses {
            let Some(outcome) = house.outcome_state else {
                if house.is_defeated || house.has_won || house.has_lost {
                    return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                        owner,
                        reason: "terminal flags require serialized outcome authority",
                    });
                }
                continue;
            };
            let flags_match = match outcome.kind {
                crate::sim::house_state::HouseOutcomeKind::Victory => {
                    house.has_won && !house.has_lost && !house.is_defeated
                }
                crate::sim::house_state::HouseOutcomeKind::Defeat => {
                    house.has_lost && !house.has_won
                }
            };
            if !flags_match {
                return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "kind disagrees with terminal house flags",
                });
            }
            let next_tick = self.session.tick.saturating_add(1);
            let timer_position_is_valid = if outcome.exit_ready {
                outcome.savour_until_tick <= next_tick
            } else {
                outcome.savour_until_tick > self.session.tick
            };
            if !timer_position_is_valid {
                return Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "SavourDelay target disagrees with expiry latch",
                });
            }
        }

        let (index, identities) = RestoredObjectIndex::build(self)?;
        let seen_logic = self
            .substrate
            .validate_restored_counters_and_logic(index.highest_id, &identities)?;
        let pending_delete_ids = self
            .substrate
            .pending_delete
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        for terrain in self.production.terrain_objects.values() {
            let in_logic = seen_logic.contains(&terrain.stable_id);
            let pending_delete = pending_delete_ids.contains(&terrain.stable_id);
            if terrain.is_live() && !in_logic {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id,
                });
            }
            if !terrain.is_live() && in_logic {
                return Err(SnapshotRestoreError::InactiveLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id,
                });
            }
            if terrain.is_live() && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id,
                });
            }
            if !terrain.is_live() && !pending_delete {
                return Err(SnapshotRestoreError::MissingDeferredDeleteIdentity {
                    registry: "TerrainObjectStore",
                    object_id: terrain.stable_id,
                });
            }
        }
        for (&object_id, _) in self.projectiles.iter() {
            let in_logic = seen_logic.contains(&object_id);
            let pending_delete = pending_delete_ids.contains(&object_id);
            if !in_logic && !pending_delete {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "ProjectileStore",
                    object_id,
                });
            }
            if in_logic && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "ProjectileStore",
                    object_id,
                });
            }
        }
        for (&object_id, _) in self.waves.iter() {
            let in_logic = seen_logic.contains(&object_id);
            let pending_delete = pending_delete_ids.contains(&object_id);
            if !in_logic && !pending_delete {
                return Err(SnapshotRestoreError::MissingRequiredLogicIdentity {
                    registry: "WaveStore",
                    object_id,
                });
            }
            if in_logic && pending_delete {
                return Err(SnapshotRestoreError::DeferredDeleteLogicIdentity {
                    registry: "WaveStore",
                    object_id,
                });
            }
        }

        validate_passenger_size_tables(self)?;
        restore_object_references(self, &identities)?;

        // EntityStore already restored its indexes during Deserialize (Clone
        // retains them too). Reference restoration above changes no indexed identity.
        // Continue with Logic slots (including ParticleSystem). Actual CellClass
        // list heads/order were serialized, like483C10/4839F0: validate references
        // without requerying geometry before map authority is reattached.
        // Bullet Load 46AE9C..46AEB0 starts both embedded timers at the
        // already-restored global frame with duration zero. Only C4/CC gates
        // AI through Check (4E11F0); retain the saved reference/watermark.
        for (_, projectile) in self.projectiles.iter_mut() {
            projectile
                .arm_timer
                .start(self.session.binary_frame as i32, 0);
        }
        // ParasiteClass Load 6295DB..6295F3 does the same for its suppression
        // and bite timers: a bite is due at once and suppression is dropped.
        let frame = self.session.binary_frame;
        for id in self.substrate.entities.keys_sorted() {
            if let Some(parasite) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|entity| entity.parasite.as_deref_mut())
            {
                parasite.restart_timers_after_load(frame);
            }
        }
        self.rebuild_logic_membership();
        self.rebuild_building_anim_slot_indices();
        self.rebuild_bomb_carriers();
        self.substrate
            .occupancy
            .restore_memberships(&self.substrate.entities)
            .map_err(|reason| SnapshotRestoreError::InvalidCellMembership { reason })?;
        self.substrate.cell_occupation =
            crate::sim::occupancy::CellOccupationGrid::rebuild(&self.substrate.entities);
        Ok(())
    }

    /// Recreate app-owned loop handles for serialized active MoveSound state.
    ///
    /// The configured identity is re-emitted as a transient sound event while
    /// the authoritative active/countdown bytes remain untouched. The local
    /// audio owner performs any process-global selection after its acceptance
    /// gates; snapshot restoration never advances an RNG itself.
    pub(crate) fn restore_move_sound_handles_after_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Result<(), SnapshotRestoreError> {
        let object_ids = self.substrate.entities.keys_sorted();
        for object_id in object_ids {
            let Some(entity) = self.substrate.entities.get(object_id) else {
                continue;
            };
            if !entity.move_sound_active {
                continue;
            }

            let type_ref = entity.type_ref();
            let world = Self::movement_sound_world(entity);
            let Some(configured_sound) = self
                .object_type(type_ref, rules)
                .and_then(|object| object.move_sound.as_deref())
                .map(str::trim)
                .filter(|sound| !sound.is_empty() && !sound.eq_ignore_ascii_case("none"))
                .map(str::to_owned)
            else {
                return Err(SnapshotRestoreError::ActiveMoveSoundUnresolvable { object_id });
            };
            let Some(sound_id) = self.interner.get(&configured_sound) else {
                return Err(SnapshotRestoreError::ActiveMoveSoundUnresolvable { object_id });
            };

            self.sound_events
                .push(crate::sim::world::SimSoundEvent::AnimationStarted {
                    anim_id: object_id,
                    sound_id,
                    world,
                });
        }
        Ok(())
    }

    /// Reproject serialized overlay and bridge authority onto the fresh
    /// map-derived terrain cache, then publish canonical navigation before the
    /// restored simulation can run another frame.
    ///
    /// The full row-major sweep is required because OverlayGrid's dirty queues
    /// are transient: a saved cell may have been cleared since map load, so an
    /// occupied-only replay would leave the original map overlay's passability
    /// behind. Low-bridge state is reconciled afterward because its serialized
    /// runtime cell is the final authority for the bridge surface.
    pub(crate) fn restore_map_authority_after_snapshot_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) -> Result<SnapshotMapRestoreOutput, SnapshotRestoreError> {
        let (overlay_width, overlay_height, overlay_cell_count, retained_wall_count) = self
            .overlay_grid
            .as_ref()
            .map(|grid| {
                (
                    grid.width(),
                    grid.height(),
                    grid.cell_storage_len(),
                    grid.retained_neighbor_count_storage_len(),
                )
            })
            .ok_or(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            })?;
        let expected_overlay_cell_count = overlay_width as usize * overlay_height as usize;
        if overlay_cell_count != expected_overlay_cell_count {
            return Err(SnapshotRestoreError::MapAuthorityCellStorageMismatch {
                expected: expected_overlay_cell_count,
                found: overlay_cell_count,
            });
        }
        // `CellClass+0x122`'s wall contribution is written only by
        // `OverlayClass::Mark` and decremented only by an explicit removal;
        // nothing in gamemd rebuilds it from final wall identities. Both
        // production map-authority constructors now retain the plane (the
        // finalized authored payload and the map-pack boundary), so a
        // current-version state without one has no wall authority to restore.
        let Some(retained_wall_count) = retained_wall_count else {
            return Err(SnapshotRestoreError::MissingRetainedWallNeighborPlane);
        };
        if retained_wall_count != expected_overlay_cell_count {
            return Err(SnapshotRestoreError::RetainedWallNeighborStorageMismatch {
                expected: expected_overlay_cell_count,
                found: retained_wall_count,
            });
        }
        let (terrain_width, terrain_height) = self
            .resolved_terrain
            .as_ref()
            .map(|terrain| (terrain.width(), terrain.height()))
            .ok_or(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            })?;
        if (overlay_width, overlay_height) != (terrain_width, terrain_height) {
            return Err(SnapshotRestoreError::MapAuthorityDimensionMismatch {
                overlay_width,
                overlay_height,
                terrain_width,
                terrain_height,
            });
        }

        // Reproject runtime isometric replacements onto the pristine
        // app-supplied map before overlay, bridge, and zone reconstruction.
        {
            let terrain = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache");
            for (&(rx, ry), state) in &self.dynamic_terrain_cells {
                if !terrain.apply_dynamic_cell_state(rx, ry, state) {
                    return Err(SnapshotRestoreError::DynamicTerrainCellMissing { rx, ry });
                }
            }
        }

        // Native CellClass::Load restores allocated real-cell flag words as
        // values. Do this before derived overlay/bridge reconstruction and do
        // not route through GetCell or SetBridgeDirection: those would stamp
        // the process-global dummy that Resize reconstructs at commit.
        if !self
            .resolved_terrain
            .as_mut()
            .expect("validated terrain cache")
            .restore_real_cell_bridge_flags_0x1180(&self.real_cell_bridge_flags_0x1180)
        {
            return Err(SnapshotRestoreError::RealCellBridgeFlagAuthorityMismatch);
        }

        // The app-supplied terrain template is the immutable load-time map.
        // Action 40's mutable LocalSize is serialized on Simulation, so replay
        // its CellClass-derived cache before overlay and zone reconstruction.
        if let Some(bounds) = self.playfield_bounds {
            let _ = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache")
                .recalc_playfield_attributes(bounds);
        }

        {
            let overlay_grid = self.overlay_grid.as_mut().expect("validated overlay cache");
            let resolved_terrain = self
                .resolved_terrain
                .as_mut()
                .expect("validated terrain cache");
            for ry in 0..overlay_height {
                for rx in 0..overlay_width {
                    let _ = crate::sim::overlay_grid::recalc_overlay_passability(
                        overlay_grid,
                        resolved_terrain,
                        overlay_registry,
                        rx,
                        ry,
                    );
                }
            }
        }

        crate::sim::world::bridge_orchestrator::reconcile_low_bridge_surface_after_cache_load(
            self,
            overlay_registry,
        );
        if !self.rebuild_dynamic_navigation(rules) {
            return Err(SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            });
        }

        let occupied_overlays = self
            .overlay_grid
            .as_ref()
            .expect("validated overlay cache")
            .iter_occupied()
            .filter_map(|(rx, ry, cell)| {
                cell.overlay_id
                    .map(|overlay_id| crate::map::overlay::OverlayEntry {
                        rx,
                        ry,
                        overlay_id,
                        frame: cell.overlay_data,
                    })
            })
            .collect();
        let native_tiberium_stats =
            self.rebuild_native_tiberium_queues_after_snapshot_load(rules, overlay_registry)?;
        Ok(SnapshotMapRestoreOutput {
            occupied_overlays,
            native_tiberium_stats,
        })
    }

    /// Replace serialized Tiberium queue stores with their native post-load
    /// cell-derived result. This consumes no RNG and leaves both class timers
    /// due at the restored binary frame.
    ///
    /// gamemd-derived: `TiberiumClass::Load @ 0x00721E80` discards both
    /// dynamic queue stores and calls `InitGrowthQueues_All @ 0x00722D00` then
    /// `InitSpreadQueues_All @ 0x00722240`. Spread admission reads the ground
    /// CellClass object list, never the bridge/AltObject list.
    fn rebuild_native_tiberium_queues_after_snapshot_load(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
    ) -> Result<crate::sim::ore_growth::NativeTiberiumRebuildStats, SnapshotRestoreError> {
        let overlay_grid = self.overlay_grid.as_ref().ok_or(
            SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            },
        )?;
        let resolved_terrain = self.resolved_terrain.as_ref().ok_or(
            SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "ResolvedTerrainGrid",
            },
        )?;
        let mut source_object_cells: BTreeSet<(u16, u16)> = self
            .production
            .terrain_objects
            .values()
            .filter(|terrain| terrain.is_live())
            .map(crate::sim::terrain_object::TerrainObjectState::cell)
            .collect();
        source_object_cells.extend(
            self.substrate
                .occupancy
                .occupied_cells_on_layer(crate::sim::movement::locomotor::MovementLayer::Ground),
        );

        let grows = self.production.ore_growth_config.grows;
        let spreads = self.production.ore_growth_config.spreads;
        let current_frame = self.session.binary_frame;
        // The stores are sized and walked from the native `MapRect` (`[Map]
        // Size`), never the cell-array extent in `session.map_*`. The rect the
        // fresh load rebuilt with is serialized; a zero rect (no rebuild ever
        // ran) falls back to the retained MapClass Size dimensions.
        let mut native_rect = self.production.ore_growth_state.native_rect();
        if native_rect == (0, 0)
            && let (Some(bounds), Some(height)) =
                (self.playfield_bounds, self.playfield_size_height)
        {
            native_rect = (
                u16::try_from(bounds.base).unwrap_or(0),
                u16::try_from(height).unwrap_or(0),
            );
        }
        Ok(self
            .production
            .ore_growth_state
            .rebuild_native_tiberium_queues_from_overlays(
                overlay_grid,
                overlay_registry,
                &rules.tiberium_types,
                Some(resolved_terrain),
                &source_object_cells,
                grows,
                spreads,
                current_frame,
                native_rect,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::locomotor_type::{MovementZone, SpeedType};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    use crate::sim::world::{RevealOutcome, Simulation};
    use std::collections::BTreeMap;

    /// Helper: advance a sim by one tick with empty inputs.
    fn tick(sim: &mut Simulation) {
        let height_map = BTreeMap::new();
        sim.advance_tick(&[], None, &height_map, None, None, 67);
    }

    fn clear_terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: crate::map::resolved_terrain::zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: SpeedCostProfile::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for ry in 0..height {
            for rx in 0..width {
                cells.push(clear_terrain_cell(rx, ry));
            }
        }
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    mod gsi_17_04_tests {
        use super::flat_terrain;
        use crate::map::basic::{BasicSection, SpecialFlagsSection};
        use crate::map::entities::EntityCategory;
        use crate::map::map_file::MapHeader;
        use crate::map::overlay_types::OverlayTypeRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
        use crate::sim::overlay_grid::OverlayGrid;
        use crate::sim::rng::SimRng;
        use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
        use crate::sim::terrain_object::{TerrainObjectLifecycle, TerrainObjectState};
        use crate::sim::world::Simulation;
        use std::collections::{BTreeMap, BTreeSet};

        fn tiberium_fixture() -> (RuleSet, OverlayTypeRegistry, u8) {
            let ini = IniFile::from_str(
                "[General]\n\
                 TiberiumGrows=yes\n\
                 TiberiumSpreads=yes\n\
                 [InfantryTypes]\n\
                 [VehicleTypes]\n\
                 [AircraftTypes]\n\
                 [BuildingTypes]\n\
                 [Warheads]\n\
                 [OverlayTypes]\n\
                 0=ORE\n\
                 [ORE]\n\
                 Tiberium=yes\n\
                 [Tiberiums]\n\
                 0=Riparius\n\
                 [Riparius]\n\
                 Image=1\n\
                 Growth=17\n\
                 GrowthPercentage=1\n\
                 Spread=23\n\
                 SpreadPercentage=1\n",
            );
            let rules = RuleSet::from_ini(&ini).expect("post-load tiberium fixture rules");
            let registry = OverlayTypeRegistry::from_ini(&ini, None);
            let ore_id = registry.id_for_name("ORE").expect("fixture ORE id");
            (rules, registry, ore_id)
        }

        fn base_sim(ore_id: u8, cells: &[(u16, u16)]) -> Simulation {
            base_sim_sized(ore_id, 8, cells)
        }

        fn base_sim_sized(ore_id: u8, size: u16, cells: &[(u16, u16)]) -> Simulation {
            let mut sim = Simulation::with_seed(0x17_04);
            sim.session.binary_frame = 91;
            sim.session.map_width = size;
            sim.session.map_height = size;
            sim.production.ore_growth_config = OreGrowthConfig {
                grows: true,
                spreads: true,
                tiberium_grows_flag: false,
            };
            sim.production.ore_growth_state = OreGrowthState::new(size, size);
            let mut overlays = OverlayGrid::new_with_retained_wall_plane(size, size);
            for &(rx, ry) in cells {
                overlays.place_overlay(rx, ry, ore_id, 5);
            }
            sim.overlay_grid = Some(overlays);
            sim.install_resolved_terrain_for_new_map(flat_terrain(size, size));
            sim
        }

        fn map_header(width: u32, height: u32) -> MapHeader {
            MapHeader {
                theater: "TEMPERATE".to_string(),
                fill: "Clear".to_string(),
                level: 0,
                width,
                height,
                local_left: 0,
                local_top: 0,
                local_width: width,
                local_height: height,
            }
        }

        fn add_marked_unit(sim: &mut Simulation, cell: (u16, u16), on_bridge: bool) {
            let stable_id = sim.allocate_stable_id();
            let owner = sim.interner.intern("Americans");
            let type_ref = sim.interner.intern("TESTUNIT");
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                stable_id,
                cell.0,
                cell.1,
                0,
                0,
                owner,
                Health { current: 100 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                true,
            );
            entity.lifecycle.in_limbo = false;
            entity.in_logic_vector = true;
            entity.on_bridge = on_bridge;
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(stable_id);
            sim.substrate
                .logic
                .try_push(stable_id)
                .expect("fixture LogicClass registration");
        }

        fn add_live_terrain_object(sim: &mut Simulation, cell: (u16, u16)) {
            let stable_id = sim.allocate_stable_id();
            let type_ref = sim.interner.intern("TREE01");
            sim.production.terrain_objects.insert(
                stable_id,
                TerrainObjectState {
                    stable_id,
                    native_unique_id: None,
                    in_logic_vector: true,
                    type_ref,
                    rx: cell.0,
                    ry: cell.1,
                    health: 100,
                    max_health: 100,
                    occupation_bits: 7,
                    lifecycle: TerrainObjectLifecycle::Live,
                },
            );
            sim.production.terrain_object_cells.insert(cell, stable_id);
            sim.substrate
                .logic
                .try_push(stable_id)
                .expect("fixture TerrainClass Logic registration");
        }

        fn seed_serialized_queue_state(
            sim: &mut Simulation,
            rules: &RuleSet,
            registry: &OverlayTypeRegistry,
            ore_cell: (u16, u16),
        ) {
            sim.production
                .ore_growth_state
                .reset_native_tiberium_classes(rules.tiberium_types.len(), 7);
            let overlay = sim.overlay_grid.as_ref().expect("fixture overlay grid");
            let resolved = sim.resolved_terrain.as_ref();
            let mut throwaway_rng = SimRng::new(13);
            assert!(
                sim.production
                    .ore_growth_state
                    .add_native_growth_queue_cell(
                        overlay,
                        registry,
                        &rules.tiberium_types,
                        ore_cell.0,
                        ore_cell.1,
                        41,
                        &mut throwaway_rng,
                    )
                    .is_some()
            );
            assert!(
                sim.production
                    .ore_growth_state
                    .add_native_spread_queue_cell(
                        overlay,
                        registry,
                        &rules.tiberium_types,
                        resolved,
                        false,
                        ore_cell.0,
                        ore_cell.1,
                        41,
                        true,
                        &mut throwaway_rng,
                    )
                    .is_some()
            );
        }

        fn restore_before_map_authority(sim: &Simulation) -> Simulation {
            let resolved = sim
                .resolved_terrain
                .clone()
                .expect("fixture resolved terrain cache");
            let bytes = GameSnapshot::save(sim, 0, 0, "gsi_17_04", 0);
            let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
            restored
                .restore_after_snapshot_load()
                .expect("stable references and substrate re-register");
            restored.rebuild_caches_after_load(
                resolved,
                Default::default(),
                Vec::new(),
                Vec::new(),
            );
            restored
        }

        #[test]
        fn gsi_17_04_postload_rebuild_discards_saved_queues_and_uses_ground_object_list() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5), (5, 5), (6, 5), (7, 5)]);
            add_marked_unit(&mut sim, (5, 5), false);
            add_marked_unit(&mut sim, (6, 5), true);
            add_live_terrain_object(&mut sim, (7, 5));
            seed_serialized_queue_state(&mut sim, &rules, &registry, (4, 5));

            let mut restored = restore_before_map_authority(&sim);
            let saved_class = &restored
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0];
            assert_eq!(saved_class.growth_timer.start_frame, 7);
            assert_ne!(
                saved_class.growth.heap_entry(0).unwrap().priority_bits,
                0.0f32.to_bits()
            );
            assert_eq!(
                restored
                    .substrate
                    .occupancy
                    .count_on_layer(5, 5, MovementLayer::Ground),
                1
            );
            assert_eq!(
                restored
                    .substrate
                    .occupancy
                    .count_on_layer(6, 5, MovementLayer::Bridge),
                1
            );

            let rng_before = restored.rng_state();
            let restore_output = restored
                .restore_map_authority_after_snapshot_load(&rules, &registry)
                .expect("all map-authority rebuild dependencies");
            assert_eq!(
                restored.rng_state(),
                rng_before,
                "load rebuild draws no RNG"
            );
            assert_eq!(restore_output.native_tiberium_stats.growth_entries, 4);
            assert_eq!(restore_output.native_tiberium_stats.spread_entries, 2);

            let class = &restored
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0];
            assert_eq!(class.growth_timer.start_frame, 91);
            assert_eq!(class.growth_timer.interval, 0);
            assert_eq!(class.spread_timer.start_frame, 91);
            assert_eq!(class.spread_timer.interval, 0);
            assert!(
                class
                    .growth
                    .iter_heap()
                    .all(|entry| entry.priority_bits == 0.0f32.to_bits())
            );
            assert!(
                class
                    .spread
                    .iter_heap()
                    .all(|entry| entry.priority_bits == 0.0f32.to_bits())
            );
            let growth_cells: BTreeSet<_> = class
                .growth
                .iter_heap()
                .map(|entry| (entry.rx, entry.ry))
                .collect();
            let spread_cells: BTreeSet<_> = class
                .spread
                .iter_heap()
                .map(|entry| (entry.rx, entry.ry))
                .collect();
            assert_eq!(
                growth_cells,
                BTreeSet::from([(4, 5), (5, 5), (6, 5), (7, 5)])
            );
            assert_eq!(spread_cells, BTreeSet::from([(4, 5), (6, 5)]));
            assert_eq!(class.growth_bitmap, growth_cells);
            assert_eq!(class.spread_bitmap, spread_cells);
        }

        /// The post-load rebuild walks the serialized native `MapRect`
        /// (`0x0087F8DC` / `0x0087F8E0`), never the cell-array extent in
        /// `session.map_*`. With 12x12 storage over an 8x8 `[Map] Size`,
        /// (5, 5) lies on anti-diagonal 10: inside the (8, 8) walk (first
        /// diagonal 9), outside a (12, 12) walk (first diagonal 13).
        #[test]
        fn gsi_17_04_postload_rebuild_walks_the_serialized_native_rect() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim_sized(ore_id, 12, &[(5, 5), (7, 7)]);
            sim.install_playfield_from_map_header(&map_header(8, 8));
            {
                let overlay = sim.overlay_grid.as_ref().expect("fixture overlay grid");
                let fresh = sim
                    .production
                    .ore_growth_state
                    .rebuild_native_tiberium_queues_from_overlays(
                        overlay,
                        &registry,
                        &rules.tiberium_types,
                        sim.resolved_terrain.as_ref(),
                        &BTreeSet::new(),
                        true,
                        true,
                        7,
                        (8, 8),
                    );
                assert_eq!(fresh.growth_entries, 2);
            }
            assert_eq!(sim.production.ore_growth_state.native_rect(), (8, 8));

            let mut restored = restore_before_map_authority(&sim);
            assert_eq!(
                restored.production.ore_growth_state.native_rect(),
                (8, 8),
                "the rect survives the snapshot"
            );
            assert_eq!(restored.session.map_width, 12);
            let output = restored
                .restore_map_authority_after_snapshot_load(&rules, &registry)
                .expect("all map-authority rebuild dependencies");
            assert_eq!(output.native_tiberium_stats.growth_entries, 2);
            assert_eq!(output.native_tiberium_stats.spread_entries, 2);
            let class = &restored
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0];
            assert_eq!(class.growth_bitmap, BTreeSet::from([(5, 5), (7, 7)]));
            assert_eq!(
                class.growth.capacity(),
                crate::sim::ore_growth::native_tiberium_queue_capacity((8, 8))
            );
        }

        /// A snapshot written before the rect was serialized deserializes it
        /// as `(0, 0)`; the rebuild then falls back to the retained MapClass
        /// `Size` (`+0xF4` width plus the separately retained height), never
        /// the cell-array extent.
        #[test]
        fn gsi_17_04_postload_rebuild_zero_rect_falls_back_to_map_size() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim_sized(ore_id, 12, &[(5, 5), (7, 7)]);
            sim.install_playfield_from_map_header(&map_header(8, 8));
            sim.production
                .ore_growth_state
                .set_native_rect_for_tests((0, 0));

            let mut restored = restore_before_map_authority(&sim);
            assert_eq!(restored.production.ore_growth_state.native_rect(), (0, 0));
            let output = restored
                .restore_map_authority_after_snapshot_load(&rules, &registry)
                .expect("all map-authority rebuild dependencies");
            assert_eq!(output.native_tiberium_stats.growth_entries, 2);
            assert_eq!(restored.production.ore_growth_state.native_rect(), (8, 8));
        }

        /// Fresh-load `InitGrowthQueues_All @ 0x00722D00` /
        /// `InitSpreadQueues_All @ 0x00722240` read `Cell+0xE4 FirstObject`,
        /// which `TerrainClass::Mark @ 0x0071BFF8` links for every terrain
        /// object, spawning or not. A plain tree over ore keeps the cell out
        /// of the spread store while `CanGrowTiberium @ 0x00483620` still
        /// admits it to growth.
        #[test]
        fn oq_38_fresh_load_queue_init_counts_plain_terrain_objects_as_first_object() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5), (5, 5)]);
            add_live_terrain_object(&mut sim, (5, 5));
            assert!(
                !sim.production
                    .tiberium_spawning_terrain_cells
                    .contains(&(5, 5)),
                "the tree is not an ore spawner"
            );

            let live_grid = sim.overlay_grid.take();
            let stats = crate::sim::runtime::initialize_native_tiberium_queues(
                &mut sim,
                &BasicSection::default(),
                &SpecialFlagsSection::default(),
                &rules,
                &registry,
                live_grid.as_ref(),
                (8, 8),
            )
            .expect("overlay grid present");
            sim.overlay_grid = live_grid;

            assert_eq!(stats.growth_entries, 2);
            assert_eq!(stats.spread_entries, 1);
            let class = &sim
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0];
            assert_eq!(class.growth_bitmap, BTreeSet::from([(4, 5), (5, 5)]));
            assert_eq!(class.spread_bitmap, BTreeSet::from([(4, 5)]));
        }

        #[test]
        fn gsi_17_04_first_resumed_pass_reloads_type_growth_and_spread_intervals() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5)]);
            seed_serialized_queue_state(&mut sim, &rules, &registry, (4, 5));
            let mut restored = restore_before_map_authority(&sim);
            restored
                .restore_map_authority_after_snapshot_load(&rules, &registry)
                .expect("all map-authority rebuild dependencies");
            restored.resolve_type_handles(&rules);

            let timer_before = restored
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0]
                .growth_timer;
            assert_eq!((timer_before.start_frame, timer_before.interval), (91, 0));
            restored.advance_tick(
                &[],
                Some(&rules),
                &BTreeMap::new(),
                None,
                Some(&registry),
                67,
            );

            let class = &restored
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0];
            assert_eq!(
                (class.growth_timer.start_frame, class.growth_timer.interval),
                (91, 17)
            );
            assert_eq!(
                (class.spread_timer.start_frame, class.spread_timer.interval),
                (91, 23)
            );
            assert_eq!(restored.session.binary_frame, 92);
        }

        #[test]
        fn gsi_17_04_missing_dependency_rejects_postload_queue_admission() {
            let (rules, registry, ore_id) = tiberium_fixture();
            let mut sim = base_sim(ore_id, &[(4, 5)]);
            sim.production
                .ore_growth_state
                .reset_native_tiberium_classes(rules.tiberium_types.len(), 7);

            let overlays = sim.overlay_grid.take();
            assert!(matches!(
                sim.restore_map_authority_after_snapshot_load(&rules, &registry),
                Err(SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "OverlayGrid",
                })
            ));
            sim.overlay_grid = overlays;
            sim.resolved_terrain = None;
            assert!(matches!(
                sim.restore_map_authority_after_snapshot_load(&rules, &registry),
                Err(SnapshotRestoreError::MissingMapAuthorityComponent {
                    component: "ResolvedTerrainGrid",
                })
            ));
            let class = &sim
                .production
                .ore_growth_state
                .native_tiberium_state()
                .classes[0];
            assert_eq!(
                (class.growth_timer.start_frame, class.growth_timer.interval),
                (7, 0)
            );
        }
    }

    fn all_terrain_costs(terrain: &ResolvedTerrainGrid) -> BTreeMap<SpeedType, TerrainCostGrid> {
        let mut costs = BTreeMap::new();
        for speed_type in [
            SpeedType::Foot,
            SpeedType::Track,
            SpeedType::Wheel,
            SpeedType::Hover,
            SpeedType::Winged,
            SpeedType::Float,
            SpeedType::Amphibious,
            SpeedType::FloatBeach,
        ] {
            costs.insert(
                speed_type,
                TerrainCostGrid::from_resolved_terrain(terrain, speed_type),
            );
        }
        costs
    }

    fn rebuild_load_caches(sim: &mut Simulation, terrain: ResolvedTerrainGrid) {
        // Synthetic fixtures use unchecked snapshots and often bypass the
        // monotonic allocators. Rebuild their substrate caches directly; the
        // production path uses `restore_after_snapshot_load`.
        sim.substrate.entities.rebuild_owner_index();
        sim.rebuild_logic_membership();
        sim.substrate
            .occupancy
            .restore_memberships(&sim.substrate.entities)
            .expect("fixture has valid saved Cell references");
        sim.substrate.cell_occupation =
            crate::sim::occupancy::CellOccupationGrid::rebuild(&sim.substrate.entities);
        sim.rebuild_caches_after_load(
            terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            Vec::new(),
            Vec::new(),
        );
    }

    #[test]
    fn snapshot_restore_replays_overlay_passability_and_publishes_canonical_navigation() {
        use crate::map::overlay::OverlayEntry;
        use crate::map::overlay_types::OverlayTypeRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayGrid, recalc_overlay_passability};
        use crate::sim::pathfinding::zone_map::ZONE_INVALID;

        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n0=WALL\n\
             [WALL]\nWall=yes\nStrength=100\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("snapshot overlay rules");
        let registry = OverlayTypeRegistry::from_ini(&ini, None);

        let cleared_since_map_load = (0, 0);
        let runtime_wall = (2, 0);
        let mut map_terrain = flat_terrain(3, 1);
        let mut map_overlays = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: cleared_since_map_load.0,
                ry: cleared_since_map_load.1,
                overlay_id: 0,
                frame: 0,
            }],
            3,
            1,
        );
        map_overlays.retain_zero_wall_plane_for_tests();
        assert!(recalc_overlay_passability(
            &mut map_overlays,
            &mut map_terrain,
            &registry,
            cleared_since_map_load.0,
            cleared_since_map_load.1,
        ));
        assert!(
            map_terrain
                .cell(cleared_since_map_load.0, cleared_since_map_load.1)
                .expect("map wall terrain")
                .overlay_blocks
        );

        let mut sim = Simulation::new();
        let mut runtime_overlays = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: runtime_wall.0,
                ry: runtime_wall.1,
                overlay_id: 0,
                frame: 0x21,
            }],
            3,
            1,
        );
        // This fixture places a live wall but is not a blocker-count subject:
        // the zero plane only satisfies the map-authority restore gate. A real
        // wall always carries its `OverlayClass::Mark` increments.
        runtime_overlays.retain_zero_wall_plane_for_tests();
        sim.overlay_grid = Some(runtime_overlays);
        sim.install_resolved_terrain_for_new_map(map_terrain.clone());

        let bytes = GameSnapshot::save(&sim, 0, 0, "overlay_restore.map", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("snapshot with runtime overlay authority")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("stable snapshot identity");
        let authoritative_hash = restored.state_hash();
        restored.rebuild_caches_after_load(
            map_terrain,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            Vec::new(),
            Vec::new(),
        );

        let restore_output = restored
            .restore_map_authority_after_snapshot_load(&rules, &registry)
            .expect("restored overlay and navigation authority");
        let terrain = restored
            .resolved_terrain
            .as_ref()
            .expect("restored terrain");
        assert!(!terrain.cell(0, 0).expect("cleared cell").overlay_blocks);
        assert!(
            terrain
                .cell(2, 0)
                .expect("runtime wall cell")
                .overlay_blocks
        );
        let path = restored.path_grid().expect("canonical restored path grid");
        assert!(path.is_walkable(0, 0));
        assert!(!path.is_walkable(2, 0));
        assert_eq!(restored.terrain_costs[&SpeedType::Track].cost_at(0, 0), 100);
        assert_eq!(restored.terrain_costs[&SpeedType::Track].cost_at(2, 0), 0);
        let normal_zones = restored
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("canonical restored ground zones");
        assert_ne!(
            normal_zones.zone_at(0, 0, MovementLayer::Ground),
            ZONE_INVALID
        );
        assert_eq!(
            normal_zones.zone_at(2, 0, MovementLayer::Ground),
            ZONE_INVALID
        );
        assert_eq!(restore_output.occupied_overlays.len(), 1);
        assert_eq!(
            (
                restore_output.occupied_overlays[0].rx,
                restore_output.occupied_overlays[0].ry,
                restore_output.occupied_overlays[0].overlay_id,
                restore_output.occupied_overlays[0].frame,
            ),
            (2, 0, 0, 0x21)
        );
        assert_eq!(restore_output.native_tiberium_stats, Default::default());
        assert_eq!(restored.state_hash(), authoritative_hash);
    }

    #[test]
    fn snapshot_restore_rejects_truncated_overlay_cell_storage() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

        #[derive(serde::Serialize)]
        struct OverlayGridWire {
            width: u16,
            height: u16,
            cells: Vec<OverlayCell>,
            retained_neighbor_counts: Option<Vec<u8>>,
        }

        let malformed_bytes = bincode::serialize(&OverlayGridWire {
            width: 2,
            height: 1,
            cells: vec![OverlayCell::default()],
            retained_neighbor_counts: None,
        })
        .expect("malformed overlay wire fixture");
        let malformed: OverlayGrid =
            bincode::deserialize(&malformed_bytes).expect("wire-compatible OverlayGrid");
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("truncated-grid rules");
        let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(malformed);
        sim.resolved_terrain = Some(flat_terrain(2, 1));

        assert!(matches!(
            sim.restore_map_authority_after_snapshot_load(&rules, &registry),
            Err(SnapshotRestoreError::MapAuthorityCellStorageMismatch {
                expected: 2,
                found: 1,
            })
        ));
    }

    #[test]
    fn snapshot_restore_rejects_truncated_retained_wall_neighbor_storage() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

        #[derive(serde::Serialize)]
        struct OverlayGridWire {
            width: u16,
            height: u16,
            cells: Vec<OverlayCell>,
            retained_neighbor_counts: Option<Vec<u8>>,
        }

        let malformed_bytes = bincode::serialize(&OverlayGridWire {
            width: 2,
            height: 1,
            cells: vec![OverlayCell::default(); 2],
            retained_neighbor_counts: Some(vec![7]),
        })
        .expect("malformed retained wall-neighbor wire fixture");
        let malformed: OverlayGrid =
            bincode::deserialize(&malformed_bytes).expect("wire-compatible OverlayGrid");
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("truncated-retained-grid rules");
        let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(malformed);
        sim.resolved_terrain = Some(flat_terrain(2, 1));

        assert!(matches!(
            sim.restore_map_authority_after_snapshot_load(&rules, &registry),
            Err(SnapshotRestoreError::RetainedWallNeighborStorageMismatch {
                expected: 2,
                found: 1,
            })
        ));
    }

    /// `CellClass+0x122`'s wall contribution is written only by
    /// `OverlayClass::Mark` and cleared only by an explicit removal; gamemd
    /// never rebuilds it from final wall identities. Both production map
    /// authorities retain the plane, so a current-version state without one is
    /// rejected rather than silently rescanned.
    #[test]
    fn snapshot_restore_rejects_a_current_version_state_without_a_retained_wall_plane() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::{OverlayCell, OverlayGrid};

        #[derive(serde::Serialize)]
        struct OverlayGridWire {
            width: u16,
            height: u16,
            cells: Vec<OverlayCell>,
            retained_neighbor_counts: Option<Vec<u8>>,
        }

        let planeless_bytes = bincode::serialize(&OverlayGridWire {
            width: 2,
            height: 1,
            cells: vec![OverlayCell::default(); 2],
            retained_neighbor_counts: None,
        })
        .expect("plane-less retained wall wire fixture");
        let planeless: OverlayGrid =
            bincode::deserialize(&planeless_bytes).expect("wire-compatible OverlayGrid");
        let ini = IniFile::from_str(
            "[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
             [OverlayTypes]
",
        );
        let rules = RuleSet::from_ini(&ini).expect("plane-less grid rules");
        let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(planeless);
        sim.resolved_terrain = Some(flat_terrain(2, 1));

        assert!(matches!(
            sim.restore_map_authority_after_snapshot_load(&rules, &registry),
            Err(SnapshotRestoreError::MissingRetainedWallNeighborPlane)
        ));

        // The same shape with the plane retained clears this gate; any later
        // rejection comes from a different missing map-authority component.
        let mut retained = Simulation::new();
        retained.overlay_grid = Some(OverlayGrid::new_with_retained_wall_plane(2, 1));
        retained.resolved_terrain = Some(flat_terrain(2, 1));
        assert!(!matches!(
            retained.restore_map_authority_after_snapshot_load(&rules, &registry),
            Err(SnapshotRestoreError::MissingRetainedWallNeighborPlane)
        ));
    }

    fn cell_order(sim: &Simulation, rx: u16, ry: u16, layer: MovementLayer) -> Vec<u64> {
        sim.substrate
            .occupancy
            .get(rx, ry)
            .map(|occ| occ.iter_layer(layer).map(|o| o.entity_id).collect())
            .unwrap_or_default()
    }

    fn assert_zone_grids_equivalent(a: &ZoneGrid, b: &ZoneGrid) {
        assert_eq!(a.width, b.width);
        assert_eq!(a.height, b.height);
        for &mz in MovementZone::all_ground() {
            let map_a = a.map_for(mz).expect("zone map exists for movement zone");
            let map_b = b.map_for(mz).expect("zone map exists for movement zone");
            assert_eq!(map_a.zone_count, map_b.zone_count);
            for y in 0..a.height {
                for x in 0..a.width {
                    assert_eq!(
                        map_a.zone_at(x, y, MovementLayer::Ground),
                        map_b.zone_at(x, y, MovementLayer::Ground),
                        "ground zone mismatch for {mz:?} at ({x},{y})"
                    );
                    assert_eq!(
                        map_a.zone_at(x, y, MovementLayer::Bridge),
                        map_b.zone_at(x, y, MovementLayer::Bridge),
                        "bridge zone mismatch for {mz:?} at ({x},{y})"
                    );
                }
            }
            let adj_a = a
                .adjacency_for(mz)
                .expect("zone adjacency exists for movement zone");
            let adj_b = b
                .adjacency_for(mz)
                .expect("zone adjacency exists for movement zone");
            for zone in 0..=map_a.zone_count {
                assert_eq!(
                    adj_a.neighbors_of(zone),
                    adj_b.neighbors_of(zone),
                    "adjacency mismatch for {mz:?} zone {zone}"
                );
            }
        }
    }

    /// Prove snapshot round-trip preserves all authoritative state.
    ///
    /// 1. Create a Simulation, advance N ticks
    /// 2. Save snapshot -> bytes -> load snapshot
    /// 3. Advance both the loaded sim and a reference sim for M more ticks
    /// 4. Assert both reach the same state hash
    #[test]
    fn round_trip_preserves_state_hash() {
        // Create two identical simulations from the same seed.
        let mut sim_a = Simulation::new();
        let mut sim_b = Simulation::new();

        // Advance both for 50 ticks to build up some state.
        for _ in 0..50 {
            tick(&mut sim_a);
            tick(&mut sim_b);
        }

        // Native in-scenario load restarts Scenario RNG from Seed0. Put both
        // source/reference branches on that cursor before testing unrelated
        // snapshot persistence and continued deterministic execution.
        sim_a.scenario_rng = crate::sim::rng::SimRng::new(0);
        sim_b.scenario_rng = crate::sim::rng::SimRng::new(0);

        // Snapshot sim_a at tick 50.
        let hash_at_50 = sim_a.state_hash();
        let bytes = GameSnapshot::save(&sim_a, 0, 0, "test_map", 0);

        // Load the snapshot.
        let snapshot = GameSnapshot::load(&bytes).expect("load should succeed");
        let mut sim_loaded = snapshot.sim;

        // Verify the loaded sim has the same state hash as the original at tick 50.
        assert_eq!(
            sim_loaded.state_hash(),
            hash_at_50,
            "loaded snapshot must match original state hash at save point"
        );

        // Advance both the original and loaded sims for 50 more ticks.
        for _ in 0..50 {
            tick(&mut sim_a);
            tick(&mut sim_loaded);
        }

        // Both must reach the same state hash at tick 100.
        assert_eq!(
            sim_a.state_hash(),
            sim_loaded.state_hash(),
            "original and loaded sim must reach identical state after continued ticking"
        );

        // The reference sim (never serialized) must also match.
        for _ in 0..50 {
            tick(&mut sim_b);
        }
        assert_eq!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "reference sim (never serialized) must match serialized sim"
        );
    }

    #[test]
    fn version_mismatch_is_rejected() {
        let sim = Simulation::new();
        let mut bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);

        // Product magic and public envelope version occupy the first 12 bytes.
        bytes[12] = 255;

        assert!(matches!(
            GameSnapshot::load(&bytes),
            Err(SnapshotError::VersionMismatch {
                expected: SNAPSHOT_VERSION,
                found: 255,
            })
        ));
    }

    #[test]
    fn free_radar_rejects_v138_before_decoding_the_changed_session() {
        let bytes = bincode::serialize(&GameSnapshotHeader {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            version: 138,
            description: "pre-FreeRadar header only".into(),
            map_hash: 1,
            rules_hash: 2,
            tick: 0,
            save_timestamp: 0,
            map_name: "radar.map".into(),
        })
        .unwrap();
        assert!(matches!(
            GameSnapshot::load(&bytes),
            Err(SnapshotError::VersionMismatch {
                expected: SNAPSHOT_VERSION,
                found: 138
            })
        ));
    }

    #[test]
    fn gsi_04_07_v38_header_is_rejected_before_wall_owner_decode() {
        let bytes = bincode::serialize(&GameSnapshotHeader {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            version: 38,
            description: "v38 fixture".to_string(),
            map_hash: 1,
            rules_hash: 2,
            tick: 3,
            save_timestamp: 4,
            map_name: "v38-layout".to_string(),
        })
        .expect("serialize v38 header only");

        assert!(matches!(
            GameSnapshot::load(&bytes),
            Err(SnapshotError::VersionMismatch {
                expected: SNAPSHOT_VERSION,
                found: 38,
            })
        ));
    }

    #[test]
    fn current_header_with_missing_body_reports_deserialization_failure() {
        let bytes = bincode::serialize(&GameSnapshotHeader {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            version: SNAPSHOT_VERSION,
            description: "current fixture".to_string(),
            map_hash: 1,
            rules_hash: 2,
            tick: 3,
            save_timestamp: 4,
            map_name: "current-layout".to_string(),
        })
        .expect("serialize current header only");

        assert!(matches!(
            GameSnapshot::load(&bytes),
            Err(SnapshotError::DeserializeFailed(_))
        ));
    }

    #[test]
    fn gsi_17_02_header_roundtrip_carries_identity_versions_and_description() {
        let mut sim = Simulation::new();
        sim.session.map_name = "OFFICIAL.MAP".to_string();
        let bytes =
            GameSnapshot::save_validated(&sim, 0x1234, 0x5678, "Hold the northern ridge", 0x9abc);

        let header = GameSnapshot::read_header(&bytes).expect("current VERA header");
        assert_eq!(header.product_magic, SNAPSHOT_PRODUCT_MAGIC);
        assert_eq!(header.envelope_version, SNAPSHOT_ENVELOPE_VERSION);
        assert_eq!(header.version, SNAPSHOT_VERSION);
        assert_eq!(header.description, "Hold the northern ridge");
        assert_eq!(header.map_name, "OFFICIAL.MAP");
    }

    #[test]
    fn gsi_17_02_wrong_product_is_rejected_before_missing_body_decode() {
        let foreign_preamble = GameSnapshotPreamble {
            product_magic: *b"NOTVERA\0",
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            version: SNAPSHOT_VERSION,
        };
        let preamble_only = bincode::serialize(&foreign_preamble).expect("foreign preamble");

        assert!(matches!(
            GameSnapshot::load(&preamble_only),
            Err(SnapshotError::ProductMismatch { found }) if found == *b"NOTVERA\0"
        ));
    }

    #[test]
    fn gsi_17_02_public_and_internal_versions_gate_before_missing_body_decode() {
        let wrong_public = GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION + 1,
            version: SNAPSHOT_VERSION,
        };
        let preamble_only = bincode::serialize(&wrong_public).expect("public-version preamble");
        assert!(matches!(
            GameSnapshot::load(&preamble_only),
            Err(SnapshotError::EnvelopeVersionMismatch {
                expected: SNAPSHOT_ENVELOPE_VERSION,
                found: 2,
            })
        ));

        let wrong_internal = GameSnapshotPreamble {
            product_magic: SNAPSHOT_PRODUCT_MAGIC,
            envelope_version: SNAPSHOT_ENVELOPE_VERSION,
            version: SNAPSHOT_VERSION - 1,
        };
        let preamble_only = bincode::serialize(&wrong_internal).expect("schema-version preamble");
        assert!(matches!(
            GameSnapshot::load(&preamble_only),
            Err(SnapshotError::VersionMismatch {
                expected: SNAPSHOT_VERSION,
                found,
            }) if found == SNAPSHOT_VERSION - 1
        ));
    }

    /// Concurrent-slice ladder: radiation took 20 -> 21, ScenarioSession (SC-2)
    /// took 21 -> 22, S3 (per-object pre-death facing read + idle-Guard authority)
    /// took 22 -> 23, the S4a authoritative flip (per-object mission commit
    /// relocated to the AI host) took 23 -> 24, and S4b (the hashed
    /// `damage_particle_live_until` `+0x308`-equivalent field) took 24 -> 25,
    /// per-house native AI difficulty took 25 -> 26, and scheduler-owned
    /// animation persistence took 26 -> 27, and independent serialized lifecycle
    /// axes plus the pending-delete boundary took 27 -> 28, and exact Mission
    /// state/readiness schema took 28 -> 29, and the Harvest handler
    /// absorption (the miner FSM cursor retired into
    /// `MissionCom.handler_state`) took 29 -> 30. Particle systems reached 34;
    /// the consolidated Phase-0 persistence schema took 34 -> 35, and
    /// lifecycle target/animation identity state took 35 -> 36, and omission
    /// of process-global Main/MapGen RNG state took 36 -> 37, and serialized
    /// Drive occupation footprints took 37 -> 38, and authoritative wall
    /// ownership took 38 -> 39, and raw occupation bytes took 39 -> 40. Versions
    /// 41..46 are a FORK: dev assigned them to MultiplayPassive, passive-acquire
    /// bookkeeping, the infantry idle timer, the spawn-manager pool, and the
    /// anim-overlay unit change, while the foundations line assigned the same
    /// numbers to projectile state, trigger hashing, TeamClass state, piggyback
    /// persistence, and typed special locomotors. The merge unified both as 47.
    /// This pins it so a later accidental bump is caught. 47 -> 48 added
    /// `DriveLocomotionRuntime::occupation_handoff` mid-struct; 48 -> 49 added
    /// the serialized building hidden-occupation grid and per-entity profile;
    /// 49 -> 50 added Building base reservations and per-entity spacing;
    /// 50 -> 51 added airborne spatial-bucket membership and vector order;
    /// 51 -> 52 added the mutable per-Techno armor multiplier; 52 -> 53 added
    /// the Psychedelic berserk byte and signed timer; 53 -> 54 added the
    /// persistent WasAttackedByEnemy byte; 54 -> 55 added per-house CurrentIQ;
    /// 55 -> 56 added the Techno damage-Smoke ParticleSystem identity; 56 ->
    /// 57 added controller-owned CaptureManager capacity and victim links; 57
    /// -> 58 added per-house AngerStruct scores and selected enemy identity;
    /// 58 -> 59 replaced the lossy C4 timer with the shared signed
    /// C4/PostMortem timer and nullable retained source; 59 -> 60 added the
    /// wall-sale command plus house/game-mode receiver inputs; 60 -> 61 added
    /// ScenarioFlags no-damage; 61 -> 62 added exact object Z and the sole live
    /// low-bridge TubeMovement payload; 62 -> 63 added the immutable
    /// `Factory=BuildingType` house-edge callback profile; 63 -> 64 added the
    /// aggregate per-house SpySat-active latch; 64 -> 65 added persistent
    /// Scenario lighting transition authority and removed queued storm state;
    /// 65 -> 66 added synchronized projectile/Techno/locomotor state; 66 -> 67
    /// added Wave, aircraft-tail, guided-projectile, Cell visibility,
    /// BuildingLight, and Anim state; 67 -> 68 added projectile collision,
    /// Wave recorded-cell payloads, infantry owners, and fog/sensor state; 68
    /// -> 69 added per-cell cloak-owner words; 69 -> 70 removed the terrain,
    /// projectile, and wave local counters in favor of the global object-id
    /// source; 70 -> 71 added ProjectileTarget's native null discriminant and
    /// replaced frozen cell-target coordinates with stable CellClass identity;
    /// 71 -> 72 added explicit product/public-envelope identity and the save
    /// description to the common prefix; 72 -> 73 added persistent per-house
    /// accepted outcome kind, SavourDelay target, and expiry latch; 73 -> 74
    /// added the serialized pending-command EXIT payload; 74 -> 75 added the
    /// generic Building delayed-fire signed counter and saved weapon slot; 75
    /// -> 76 added AnimClass cell-drawer and terrain-attached bytes; 76 -> 77
    /// added the persistent Foot SHP body-frame counter and Ship-owned
    /// destination/target/current speed state used by its SHP movement slots;
    /// 77 -> 78 made living GameEntity Animation timing hash-authoritative;
    /// 78 -> 79 moved building-overlay finalization before the returned hash
    /// and made the already-serialized overlay component hash-authoritative;
    /// 79 -> 80 added the serialized/hash-authoritative terminal score snapshot;
    /// 80 -> 81 added the pending-command offline GameSpeed transition payload;
    /// 81 -> 82 made a serialized `AnimClass` coordinate owner-RELATIVE while
    /// its `owner_entity` is set, matching
    /// `AnimClass::SetOwnerObject @ 0x00424B50`. The layout is unchanged, so
    /// old bytes still decode — and would then be re-resolved through the owner
    /// a whole owner-coordinate away, moving every attached anim and the
    /// returned hash with it. Cross-version load is refused for that reason;
    /// 82 -> 83 collapsed `ParticleSystem`'s `marked_for_deletion` and
    /// `done_spawning` into the single `done_spawning`, which is what
    /// `ParticleSystemClass+0xF8` actually is — one byte set by the lifetime
    /// countdown, the spawn cutoff and the spark countdown alike, and read by
    /// both the spawn gate and the removal predicate. A serialized field is
    /// gone, so old bytes no longer decode; 84 -> 85 retained signed map Size
    /// height for action-40 normalization; 85 -> 86 added the mutable
    /// playfield revision; 86 -> 87 added Techno+0x3D5 membership; 87 -> 88
    /// added the exact historical sensor deposit needed for later removal;
    /// 88 -> 89 adds the `ProjectileTarget::DummyCell` pointer kind while the
    /// process-global dummy contents remain deliberately outside the payload;
    /// 89 -> 90 adds exact allocated real-cell 0x1180 values to Scenario
    /// persistence/hash plus the live dummy subset to the canonical hash,
    /// while retaining the native dummy reconstruct-to-zero load behavior;
    /// 90 -> 91 adds the held factory object's completion-accounted latch so
    /// delivery retries cannot replay completion after load; 91 -> 92 adds the
    /// persisted per-house BaseClass reservation bounds/vector while retaining
    /// the native reconstruct-cleared reservation dummy mask; 92 -> 93 adds
    /// House strategy emergency state; 93 -> 94 adds the House last-attacker
    /// index plus persistent Techno recruitment/archive/base-response state;
    /// 94 -> 95 adds TeamType priority/base-defence metadata and TeamClass
    /// response latches/timer state; 95 -> 96 adds the ordered AIMD/map static
    /// registries retained by TeamScriptVm; 96 -> 97 adds resolved ScriptType
    /// and TaskForce source provenance; 97 -> 98 adds the resolved typed
    /// AITrigger owner/object/scalar/mask/weight/difficulty payload; 98 -> 99
    /// removes the falsely retained AITrigger token-4 scalar after binary
    /// verification proved that token is required but discarded; 99 -> 100
    /// adds the three post-load TeamType zone-derivation fields; 100 -> 101
    /// adds category-distinct resolved TaskForce member identities; 101 -> 102
    /// adds category-distinct resolved AITrigger token-6 identities; 102 -> 103
    /// adds WaveClass state and destroyable-cliff replacement CellClass values;
    /// 103 -> 104 adds the persistent Techno constructor word and authored
    /// structure-upgrade parent/slot identity; 104 -> 105 adds active/stashed
    /// Drive/Ship slope-transition state; 105 -> 106 rejects saves that would
    /// resume under the corrected one-authority 104-lepton Cell ground formula
    /// despite unchanged wire shape; 106 -> 107 adds unconditional shared-
    /// dummy level/slope hash authority for active Spark queries; 107 -> 108
    /// adds the packed House alternate base cell; 108 -> 109 adds the ordered
    /// House BuildConst vector and immutable entity membership; 109 -> 110
    /// adds ordered BasePlan state and immutable BuildingType facts; 110 -> 111
    /// adds the distinct BaseClass plan center; 111 -> 112 adds the three House
    /// AI activation latches; 112 -> 113 adds House AutocreateAllowed;
    /// 113 -> 114 adds the raw 256-entry scenario-crate slot table and the
    /// per-Anim Convert palette selector used by startup crate CellAnim;
    /// 114 -> 115 adds retained wall-neighbor count authority and begins the
    /// live shared-dummy overlay pair in the current hash schema; 115 -> 116
    /// replaces the per-class tiberium heap vectors with the native queue
    /// stores (entry array, heap of references, capacity) and adds the
    /// `MapRect` the stores are sized from; 116 -> 117 folds the
    /// disguise-detect counter plane and the cached `DetectDisguiseRange=`
    /// deposit radius into the hash schema; 117 -> 118 inserts the
    /// `UnitClass+0x6AF` turret rotation latch and the `TechnoClass+0x120`
    /// last-fire frame mid-`GameEntity`, on every entity, which bincode cannot
    /// decode from a shorter v117 record; 118 -> 119 gives `ProjectileGuidance`
    /// the native `BulletClass` flight state — `Bullet+0x110` MaxSpeed,
    /// `BulletTypeClass+0x2D0` Acceleration, the launch-frozen
    /// `ProximityDetector` reference coordinate, and the `Bullet+0x118/+0x120`
    /// closing-rate pair — and appends the `Vertical` trajectory variant. All
    /// five guidance additions (`max_speed`, `acceleration`, `fuse_reference`,
    /// `closing_frames`, `closing_accumulator_bits`) sit at the END of the
    /// struct, but a v118 record still stops short of them, so bincode would
    /// run off the end of every in-flight projectile; 119 -> 120 adds the
    /// `VoxelAnimClass` debris registry to `ObjectSubstrate` and wires the
    /// death-side debris producer, which changes both the hash schema and the
    /// shared RNG cursor at every Techno death whose type authors a POSITIVE
    /// `MaxDebris=` — the 83 stock sections that author `MaxDebris=0` stop at
    /// `0x00702291` and still cost the stream nothing; 120 -> 123 moves combat
    /// explosions off the legacy `WorldEffect` list and into `AnimStore`, so a
    /// warhead `AnimList=` animation is now a serialized, checksum-folded
    /// object where it used to be neither. 121 and 122 are skipped because
    /// unmerged branches already stamp them — 121
    /// `origin/feature/phase3-map-spatial-close`, 122
    /// `origin/feature/phase3-map-spatial-integration` — and two branches
    /// sharing one version number defeat the mismatch guard entirely.
    /// 123 -> 126 adds the retained homing Inaccurate flag; 124 and 125 are
    /// reserved by the concurrent parasite and audio-lane changes.
    /// 130 -> 131 persists the session `TiberiumGrows`/`TiberiumSpreads`
    /// flag bits and the `OreGrowthConfig` growth-reload selector.
    /// 131 -> 132 persists the `HouseClass+0x242` harvester no-ore latch.
    /// 132 -> 133 persists the `HouseClass+0x57D4` funds-nag timer and the
    /// `[0xA8F040]` low-power guard.
    #[test]
    fn current_snapshot_version_includes_gap_operational_state() {
        // 133 -> 134: repair-depot docking layout (see the constant's comment).
        // 134 -> 135: GameEntity ProduceCash timer + drain link pair (GSI-09.01).
        // 135 -> 136: explicit retained Infantry terminal lifetime policy.
        // Separate v137: ordinary ground-coordinate resume invariants.
        // v138 combines both layouts without accepting prior local v137 saves.
        // 138 -> 139: map-owned ScenarioSession FreeRadar authority.
        // 139 -> 140: retained bridge record source Size for native zone lookup.
        // 140 -> 141: retained real CellClass gap flags400/800.
        // 141 -> 142: sustained sight and pending 120-frame gap conceal.
        // 142 -> 143: MCV pending and Drive previous-rotation latches.
        // 143 -> 144: per-Building operational edge and retained gap deposit.
        // 145 -> 146: pavement is owned by raw terrain flags, not bridge cells.
        // 146 -> 147: retain Scenario+214 for subsequent native constructors.
        // 150 -> 151: Foot also owns applied speed independently of its locomotor.
        // 151 -> 152: Foot occupation enable and pending fresh Apply1 obligation.
        // 161 -> 162: Infantry+6DC current-cell entry answer of the failed path.
        // 162 -> 163: Jumpjet linked type block and flight fields.
        // 164 -> 165: DriveTrackState::before_first_point, inserted mid-record.
        // 165 -> 166: Economy owns the sole house credit balance.
        // 170 -> 171: shared animation bounds and retained HasEngineer.
        // 173 -> 174: ProductionState drops the resource node map.
        // 174 -> 175: bridge collapse explosions join the AnimStore.
        // 180 -> 181: Foot+580 crate multiplier survives save/restore.
        // 184 -> 185: Fly owns its integer height target and takeoff/landing flags.
        // 185 -> 186: Aircraft pending ammo survives independently of Attack.
        // 186 -> 187: object burst position replaces the target-owned count.
        // 187 -> 188: retained Fly destination XYZ cannot be recovered from cells.
        // 189 -> 190: Foot neighbor history and retained live counters.
        // 190 -> 191: Fly cruise mode survives save and locomotor suspension.
        // 191 -> 192: Fly moving/landing latch/AirportBound and flight attitude.
        // 192 -> 193: ParasiteClass and the victim's parasite/paralysis fields.
        // 193 -> 194: Building+6E3 HasBeenCaptured.
        // 194 -> 195: death anims take the death producers' arguments.
        // 195 -> 196: mind-control nodes, overload state and victim links.
        // 196 -> 197: TemporalClass links and the warped object's chain head.
        // 197 -> 198: the house defeat counts.
        // 198 -> 199: the Crazy Ivan bomb on its carrier.
        // 199 -> 200: the Gattling stage, value and turret animation counter.
        // 200 -> 201: no `last_attacker_id`; TeamType `Suicide=`.
        // 201 -> 202: the rearm timer on the object.
        // 202 -> 203: no Simulation copy of CloseEnough.
        // 203 -> 204: bullet OnBridge, no owner house; house ROF bias; TeamType
        // `Aggressive=`.
        // 204 -> 205: bullet `Arcing=`; an anim's bounce body.
        // 205 -> 206: the retired Chrono dock phases; Techno+0x1F8.
        // 206 -> 207: the native ore field (Unit+0x6D2, the StageClass).
        // 207 -> 208: the crash latch and edge; the Fly fall counter.
        // 208 -> 209: the slave manager and the slave's links and Storage.
        // 209 -> 210: the retired Jumpjet type-block copy.
        assert_eq!(super::SNAPSHOT_VERSION, 210);
    }

    #[test]
    fn dock_indices_above_byte_range_survive_restore_and_release() {
        use crate::sim::aircraft::AircraftMission;
        use crate::sim::docking::aircraft_dock::AircraftAmmo;
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let type_id = sim.intern("PAD");
        let owner = sim.intern("Americans");
        for id in 1..=301 {
            assert_eq!(sim.allocate_stable_id(), id);
            let mut entity = GameEntity::test_default(id, "PAD", "Americans", 0, 0);
            entity.type_ref = type_id;
            entity.owner = owner;
            if id > 1 {
                let pad = sim
                    .production
                    .airfield_docks
                    .try_reserve(1, id, 300)
                    .unwrap();
                assert_eq!(u64::from(pad), id - 2);
                entity.aircraft_mission = Some(AircraftMission::DockedIdle {
                    airfield_id: 1,
                    pad_index: pad,
                });
                let mut ammo = AircraftAmmo::new(3);
                ammo.target_airfield = Some(1);
                ammo.target_pad = Some(pad);
                entity.aircraft_ammo = Some(ammo);
            }
            sim.substrate.entities.insert(entity);
        }
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let before = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "wide-dock-indices", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), before);
        let aircraft = restored.substrate.entities.get(301).unwrap();
        assert!(matches!(
            aircraft.aircraft_mission,
            Some(AircraftMission::DockedIdle {
                airfield_id: 1,
                pad_index: 299
            })
        ));
        assert_eq!(
            aircraft.aircraft_ammo.as_ref().unwrap().target_pad,
            Some(299)
        );
        assert_eq!(
            restored.production.airfield_docks.pad_for(301),
            Some((1, 299))
        );
        restored.production.airfield_docks.release(301);
        assert_eq!(
            restored.production.airfield_docks.pad_for(300),
            Some((1, 298))
        );
        assert_eq!(
            restored.production.airfield_docks.try_reserve(1, 301, 300),
            Some(299)
        );
    }

    #[test]
    fn signed_actual_and_estimated_health_survive_save_restore_independently() {
        for (actual, estimated) in [(-7, 100_000), (100_000, -19), (i32::MIN, i32::MAX)] {
            let mut sim = Simulation::new();
            assert_eq!(sim.allocate_stable_id(), 1);
            let mut entity =
                crate::sim::game_entity::GameEntity::test_default(1, "SIGNED", "Americans", 0, 0);
            entity.type_ref = sim.intern("SIGNED");
            entity.owner = sim.intern("Americans");
            entity.health.current = actual;
            entity.estimated_health =
                crate::sim::estimated_health::EstimatedHealth::from_raw(estimated);
            sim.substrate.entities.insert(entity);
            // Native load resets Scenario RNG to seed zero; isolate the health
            // roundtrip from that intentional load transition.
            sim.scenario_rng = crate::sim::rng::SimRng::new(0);
            let before = sim.state_hash();
            let bytes = GameSnapshot::save(&sim, 0, 0, "signed-health", 0);
            let mut restored = GameSnapshot::load(&bytes)
                .expect("signed health schema")
                .sim;
            restored
                .restore_after_snapshot_load()
                .expect("restore signed health");
            let entity = restored.substrate.entities.get(1).unwrap();
            assert_eq!(
                (entity.health.current, entity.estimated_health.get()),
                (actual, estimated)
            );
            assert_eq!(restored.state_hash(), before);
            restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .health
                .current = actual.wrapping_add(1);
            assert_ne!(restored.state_hash(), before);
            restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .health
                .current = actual;
            restored
                .substrate
                .entities
                .get_mut(1)
                .unwrap()
                .estimated_health =
                crate::sim::estimated_health::EstimatedHealth::from_raw(estimated.wrapping_add(1));
            assert_ne!(restored.state_hash(), before);
        }
    }

    #[test]
    fn combined_bridge_membership_history_schema_rejects_separate_layouts() {
        for version in 153..=174 {
            let preamble = GameSnapshotPreamble {
                product_magic: SNAPSHOT_PRODUCT_MAGIC,
                envelope_version: SNAPSHOT_ENVELOPE_VERSION,
                version,
            };
            let bytes = bincode::serialize(&preamble).expect("previous layout header");
            assert!(matches!(
                GameSnapshot::load(&bytes),
                Err(SnapshotError::VersionMismatch { expected: SNAPSHOT_VERSION, found }) if found == version
            ));
        }
    }

    #[test]
    fn current_house_is_saved_client_input_with_validated_reference() {
        let mut sim = Simulation::with_seed(0);
        let mut owners = Vec::new();
        for name in ["First", "Second"] {
            let owner = sim.intern(name);
            sim.houses.insert(
                owner,
                crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 1),
            );
            sim.session.house_order.push(owner);
            owners.push(owner);
        }
        let shared_hash = sim.state_hash();
        let rng = sim.rng_state();
        let mut saved_identities = Vec::new();
        for owner in owners.iter().copied() {
            sim.session.current_house = Some(owner);
            assert_eq!(sim.state_hash(), shared_hash);
            assert_eq!(sim.rng_state(), rng);
            let bytes = GameSnapshot::save(&sim, 0, 0, "current-house", 0);
            let mut restored = GameSnapshot::load(&bytes).expect("current schema").sim;
            restored
                .restore_after_snapshot_load()
                .expect("valid House input");
            assert_eq!(restored.session.current_house, Some(owner));
            assert_eq!(restored.state_hash(), shared_hash);
            saved_identities.push(restored.session.current_house);
            assert_eq!(restored.session.current_house, Some(owner));
            assert_eq!(restored.state_hash(), shared_hash);
            assert_eq!(restored.rng_state(), rng);
        }
        assert_ne!(saved_identities[0], saved_identities[1]);

        let missing = sim.intern("Missing");
        sim.session.current_house = Some(missing);
        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::InvalidCurrentHouse { owner: missing })
        );
        sim.session.current_house = Some(owners[0]);
        sim.session.house_order.retain(|owner| *owner != owners[0]);
        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::InvalidCurrentHouse { owner: owners[0] })
        );
    }

    #[test]
    fn infantry_terminal_restore_rejects_state_outside_its_admission() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::InfantryTerminal;
        for (category, dying, object_alive, in_limbo, terminal) in [
            (
                EntityCategory::Unit,
                true,
                true,
                true,
                Some(InfantryTerminal::RetireNextVisit),
            ),
            (
                EntityCategory::Infantry,
                false,
                true,
                true,
                Some(InfantryTerminal::RetireNextVisit),
            ),
            (
                EntityCategory::Infantry,
                true,
                true,
                true,
                Some(InfantryTerminal::AwaitingConsequences),
            ),
            (EntityCategory::Infantry, true, true, true, None),
            (
                EntityCategory::Infantry,
                true,
                false,
                true,
                Some(InfantryTerminal::RetireNextVisit),
            ),
            (
                EntityCategory::Infantry,
                true,
                true,
                false,
                Some(InfantryTerminal::RetireNextVisit),
            ),
        ] {
            let mut sim = Simulation::new();
            let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
            entity.category = category;
            entity.dying = dying;
            entity.lifecycle.object_alive = object_alive;
            entity.lifecycle.in_limbo = in_limbo;
            entity.infantry_terminal = terminal;
            sim.substrate.entities.insert(entity);
            let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
            let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
            assert_eq!(
                restored.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::InvalidInfantryTerminalState { object_id: 1 })
            );
        }
    }

    #[test]
    fn authored_wall_neighbor_authority_roundtrips_v115() {
        let mut sim = Simulation::new();
        sim.overlay_grid = Some(
            crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
                crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
                    2,
                    2,
                    vec![(-1, 0), (2, 3), (-1, 0), (-1, 0)],
                    vec![0, 7, 255, 4],
                ),
            ),
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "authored-wall-counts", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("current v115 snapshot")
            .sim;
        assert_eq!(
            restored
                .overlay_grid
                .as_ref()
                .expect("overlay authority")
                .retained_neighbor_counts(),
            Some(&[0, 7, 255, 4][..])
        );
    }

    #[test]
    fn phase14_crate_authority_roundtrips_visible_and_ghost_slots() {
        let mut sim = Simulation::with_seed(0);
        *sim.crate_authority.slot_mut(0) = crate::sim::crates::CrateSlot {
            start_frame: 17,
            aux: 0x40b5_1800,
            duration: 3375,
            cell_x: 6,
            cell_y: 9,
        };
        *sim.crate_authority.slot_mut(1) = crate::sim::crates::CrateSlot {
            start_frame: -2,
            aux: u32::MAX,
            duration: i32::MIN,
            cell_x: -4,
            cell_y: 12,
        };
        let expected = sim.crate_authority.clone();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let loaded = GameSnapshot::load(&bytes).expect("v114 crate authority snapshot");
        assert_eq!(loaded.sim.crate_authority, expected);
    }

    #[test]
    fn drive_ship_active_and_stashed_slope_phase_roundtrip_without_resample_or_rng() {
        use crate::map::entities::EntityCategory;
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotion::LocomotorRuntimePayload;
        use crate::sim::movement::locomotor::LocomotorState;

        // Full snapshot load restarts Scenario RNG from Seed0. Start on that
        // canonical cursor so the slope-only round trip can prove no draw.
        let mut sim = Simulation::with_seed(0);
        sim.session.binary_frame = 51;
        let mut entity = GameEntity::test_default(1, "SLOPE", "Americans", 0, 0);
        entity.owner = sim.intern("Americans");
        entity.type_ref = sim.intern("SLOPE");
        entity.category = EntityCategory::Unit;
        let mut locomotor = LocomotorState::for_test_kind_at_frame(LocomotorKind::Drive, 40);
        let drive = locomotor.active_slope_transition_mut().unwrap();
        drive.snap(2, 40);
        drive.sample_process_entry(7, 49);
        assert!(locomotor.begin_piggyback(LocomotorKind::Ship, MovementLayer::Ground, 50));
        let ship = locomotor.active_slope_transition_mut().unwrap();
        ship.snap(4, 40);
        ship.sample_process_entry(9, 49);
        entity.locomotor = Some(locomotor);
        sim.substrate.entities.insert(entity);

        let before_hash = sim.state_hash();
        let before_rng = sim.scenario_rng.logical_state();
        let before = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap();
        let active_before = before.runtime_payload.clone();
        let stashed_before = before.piggyback.clone();

        let bytes = GameSnapshot::save(&sim, 1, 2, "Drive Ship slope", 3);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("current v107 slope snapshot")
            .sim;
        let loaded = restored
            .substrate
            .entities
            .get(1)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap();
        assert_eq!(loaded.runtime_payload, active_before);
        assert_eq!(loaded.piggyback, stashed_before);
        assert_eq!(restored.session.binary_frame, 51);
        assert_eq!(restored.state_hash(), before_hash);
        assert_eq!(restored.scenario_rng.logical_state(), before_rng);
        assert_eq!(
            loaded.active_slope_transition().unwrap().hash_fields(),
            (4, 9, 49, 3),
            "the saved active timer starts two committed frames before session frame 51"
        );
        assert!(matches!(
            loaded.piggyback.as_deref().map(|runtime| &runtime.payload),
            Some(LocomotorRuntimePayload::Drive(state))
                if state.hash_fields() == (2, 7, 49, 3)
        ));

        let mut live_cell = clear_terrain_cell(0, 0);
        live_cell.slope_type = 12;
        let live_terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![live_cell]);
        restored.resolved_terrain = Some(live_terrain.clone());
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .active_slope_transition()
                .unwrap()
                .hash_fields(),
            (4, 9, 49, 3),
            "load/rebuild trusts the saved cache instead of resampling live terrain"
        );

        let mut sound_events = Vec::new();
        let mut lifecycle_requests = Vec::new();
        crate::sim::movement::tick_movement_with_grids(
            &mut restored.substrate.entities,
            Some(&[1]),
            None,
            &Default::default(),
            &Default::default(),
            &mut restored.substrate.occupancy,
            &mut restored.substrate.cell_occupation,
            &mut restored.substrate.raw_cell_occupation,
            &mut restored.substrate.next_occupancy_enter_order,
            &mut restored.scenario_rng,
            52,
            52,
            None,
            Some(&live_terrain),
            None,
            &crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            crate::util::fixed_math::SIM_ZERO,
            9,
            60,
            &mut restored.interner,
            None,
            &mut sound_events,
            &mut lifecycle_requests,
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .active_slope_transition()
                .unwrap()
                .hash_fields(),
            (9, 12, 52, 3),
            "only the next eligible Process restarts against live terrain"
        );
        assert_eq!(restored.scenario_rng.logical_state(), before_rng);
    }

    #[test]
    fn techno_constructor_word_and_upgrade_link_roundtrip_without_a_draw_and_hash() {
        let mut sim = Simulation::new();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "UP1", "Americans", 4, 5);
        entity.techno_ctor_random_word = 0xA55A;
        entity.structure_upgrade_link = Some(crate::sim::game_entity::StructureUpgradeLink {
            parent_stable_id: 77,
            slot: 2,
        });
        sim.substrate.entities.insert(entity);
        // Native load resets Scenario RNG to Seed(0); align the source before
        // comparing persistence/hash state unrelated to that separate rule.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let source_rng = sim.scenario_rng.logical_state();
        let source_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "techno-constructor", 0);
        let restored = GameSnapshot::load(&bytes).unwrap().sim;
        let restored_entity = restored.substrate.entities.get(1).unwrap();
        assert_eq!(restored.scenario_rng.logical_state(), source_rng);
        assert_eq!(restored_entity.techno_ctor_random_word, 0xA55A);
        assert_eq!(
            restored_entity.structure_upgrade_link,
            Some(crate::sim::game_entity::StructureUpgradeLink {
                parent_stable_id: 77,
                slot: 2,
            })
        );
        assert_eq!(restored.state_hash(), source_hash);

        let mut changed_word = GameSnapshot::load(&bytes).unwrap().sim;
        changed_word
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .techno_ctor_random_word ^= 1;
        assert_ne!(changed_word.state_hash(), source_hash);

        let mut changed_link = restored;
        changed_link
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .structure_upgrade_link
            .as_mut()
            .unwrap()
            .slot = 1;
        assert_ne!(changed_link.state_hash(), source_hash);
    }

    #[test]
    fn slave_manager_roundtrips_and_hashes_its_nodes_and_links() {
        use crate::sim::slave_manager::{ManagerState, SlaveManager, SlaveNode, SlaveState};
        let mut sim = Simulation::new();
        let mut parent =
            crate::sim::game_entity::GameEntity::test_default(1, "SMIN", "Americans", 4, 5);
        parent.techno_ctor_random_word = 0x1111;
        let slav = sim.intern("SLAV");
        parent.slave_manager = Some(SlaveManager::new(slav, [Some(2), Some(3)], 500, 25, 7));
        sim.substrate.entities.insert(parent);
        for (stable_id, word) in [(2, 0x2222), (3, 0x3333)] {
            let mut slave = crate::sim::game_entity::GameEntity::test_default(
                stable_id,
                "SLAV",
                "Americans",
                4,
                5,
            );
            slave.techno_ctor_random_word = word;
            // Slave 3 carries one level of gems.
            let cargo = (stable_id == 3)
                .then_some(crate::sim::miner::CargoBale {
                    resource_type: crate::sim::miner::ResourceType::Gem,
                    value: 50,
                })
                .into_iter()
                .collect();
            slave.slave = crate::sim::slave_manager::SlaveLink::for_test(Some(1), cargo);
            sim.substrate.entities.insert(slave);
        }
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let source_rng = sim.scenario_rng.logical_state();
        let source_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "slave-manager", 0);
        let restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(restored.scenario_rng.logical_state(), source_rng);
        assert_eq!(
            restored.substrate.entities.get(1).unwrap().slave_manager,
            sim.substrate.entities.get(1).unwrap().slave_manager
        );
        for (stable_id, word) in [(2, 0x2222), (3, 0x3333)] {
            let slave = restored.substrate.entities.get(stable_id).unwrap();
            assert_eq!(slave.techno_ctor_random_word, word);
            assert_eq!(slave.slave.owner(), Some(1));
        }
        assert_eq!(
            restored
                .substrate
                .entities
                .get(3)
                .unwrap()
                .slave
                .cargo()
                .len(),
            1
        );
        assert_eq!(restored.state_hash(), source_hash);

        let mut changed_node = GameSnapshot::load(&bytes).unwrap().sim;
        let manager = changed_node
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .slave_manager
            .as_mut()
            .unwrap();
        let mut nodes = manager.nodes().to_vec();
        nodes[0].state = SlaveState::Scanning;
        let timer = manager.ai_timer();
        manager.set_for_test(ManagerState::Ready, 0, nodes, timer);
        assert_ne!(changed_node.state_hash(), source_hash);

        let mut changed_order = GameSnapshot::load(&bytes).unwrap().sim;
        let manager = changed_order
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .slave_manager
            .as_mut()
            .unwrap();
        let mut nodes: Vec<SlaveNode> = manager.nodes().to_vec();
        nodes.reverse();
        let timer = manager.ai_timer();
        manager.set_for_test(ManagerState::Ready, 0, nodes, timer);
        assert_ne!(changed_order.state_hash(), source_hash);

        let mut changed_owner = restored;
        changed_owner.substrate.entities.get_mut(2).unwrap().slave =
            crate::sim::slave_manager::SlaveLink::for_test(Some(3), Vec::new());
        assert_ne!(changed_owner.state_hash(), source_hash);
    }

    #[test]
    fn house_ai_activation_all_combinations_roundtrip_v115() {
        use crate::sim::house_state::{HouseAiActivationLatches, HouseState};

        for bits in 0u8..16 {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Computer1");
            let latches = HouseAiActivationLatches {
                production: bits & 1 != 0,
                autocreate_allowed: bits & 2 != 0,
                ai_triggers_active: bits & 4 != 0,
                auto_base_building: bits & 8 != 0,
            };
            let mut house = HouseState::new(owner, 0, None, false, 0, 10);
            house.ai_activation = latches;
            sim.houses.insert(owner, house);
            sim.session.house_order.push(owner);

            let description = format!("house-ai-activation-{bits}");
            let bytes = GameSnapshot::save(&sim, 0, 0, &description, 0);
            assert_eq!(
                GameSnapshot::read_header(&bytes).unwrap().version,
                super::SNAPSHOT_VERSION
            );
            let restored = GameSnapshot::load(&bytes)
                .expect("current v115 snapshot")
                .sim;
            assert_eq!(restored.houses[&owner].ai_activation, latches);
        }
    }

    #[test]
    fn alternate_base_center_roundtrips_with_primary_center_and_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let country = sim.interner.intern("Americans");
        let mut house =
            crate::sim::house_state::HouseState::new(owner, 0, Some(country), false, 0, 10);
        house.base_center = Some((40, 50));
        house.alternate_base_center = (93, 106);
        house.base_plan_center = (19, 21);
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "alternate-base-center", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert_eq!(restored.houses[&owner].base_center, Some((40, 50)));
        assert_eq!(restored.houses[&owner].alternate_base_center, (93, 106));
        assert_eq!(restored.houses[&owner].base_plan_center, (19, 21));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn naval_build_const_order_and_membership_roundtrip_with_current_hash_only() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let country = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("GACNST");
        let house = crate::sim::house_state::HouseState::new(owner, 0, Some(country), false, 0, 10);
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            9,
            4,
            5,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 1000 },
            type_ref,
            crate::map::entities::EntityCategory::Structure,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(entity);

        let v108_default_hash = sim.state_hash_without_naval_build_const_v109();
        let v109_default_hash = sim.state_hash_without_base_plan_v110();
        assert_eq!(
            v109_default_hash, v108_default_hash,
            "empty BuildConst vectors and false membership preserve the v108 hash stream through v109"
        );
        assert_ne!(
            sim.state_hash(),
            v109_default_hash,
            "the v110 schema adds BasePlan authority even when BuildConst state is empty"
        );

        sim.houses.get_mut(&owner).unwrap().build_const_order = vec![9, 3];
        sim.substrate
            .entities
            .get_mut(9)
            .unwrap()
            .build_const_eligible = true;

        let ordered_hash = sim.state_hash();
        let v109_ordered_hash = sim.state_hash_without_base_plan_v110();
        let historical_pre_v109 = sim.state_hash_without_naval_build_const_v109();
        assert_ne!(ordered_hash, historical_pre_v109);
        let historical_pre_v28 = sim.state_hash_before_lifecycle_v28_and_mission_v29();
        let historical_pre_v29 = sim.state_hash_without_mission_v29();
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .build_const_order
            .swap(0, 1);
        assert_ne!(
            sim.state_hash(),
            ordered_hash,
            "stored vector order is hashed"
        );
        assert_ne!(
            sim.state_hash_without_base_plan_v110(),
            v109_ordered_hash,
            "the v109 provenance schema retained by the v110 probe hashes BuildConst order"
        );
        assert_eq!(
            sim.state_hash_without_naval_build_const_v109(),
            historical_pre_v109,
            "the v108 provenance schema excludes BuildConst acquisition order"
        );
        assert_eq!(
            sim.state_hash_before_lifecycle_v28_and_mission_v29(),
            historical_pre_v28,
            "the historical pre-v28 probe excludes current-schema state"
        );
        assert_eq!(
            sim.state_hash_without_mission_v29(),
            historical_pre_v29,
            "the historical pre-v29 probe excludes current-schema state"
        );
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .build_const_order
            .swap(0, 1);

        sim.substrate
            .entities
            .get_mut(9)
            .unwrap()
            .build_const_eligible = false;
        assert_ne!(
            sim.state_hash(),
            ordered_hash,
            "entity membership is hashed"
        );
        assert_ne!(
            sim.state_hash_without_base_plan_v110(),
            v109_ordered_hash,
            "the v109 provenance schema retained by the v110 probe hashes BuildConst membership"
        );
        assert_eq!(
            sim.state_hash_without_naval_build_const_v109(),
            historical_pre_v109,
            "the v108 provenance schema excludes immutable BuildConst membership"
        );
        assert_eq!(
            sim.state_hash_before_lifecycle_v28_and_mission_v29(),
            historical_pre_v28
        );
        assert_eq!(sim.state_hash_without_mission_v29(), historical_pre_v29);
        sim.substrate
            .entities
            .get_mut(9)
            .unwrap()
            .build_const_eligible = true;
        assert_eq!(sim.state_hash(), ordered_hash);
        assert_eq!(sim.state_hash_without_base_plan_v110(), v109_ordered_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "naval-build-const", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert_eq!(restored.houses[&owner].build_const_order, vec![9, 3]);
        assert!(
            restored
                .substrate
                .entities
                .get(9)
                .unwrap()
                .build_const_eligible
        );
        assert_eq!(restored.state_hash(), ordered_hash);
    }

    #[test]
    fn gsi_04_05_base_plan_and_building_facts_roundtrip_current_snapshot() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let type_ref = sim.interner.intern("GAPOWR");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
        house.base_plan.percent_built = -17;
        house.base_plan.nodes = vec![
            crate::sim::base_plan::BasePlanNode {
                type_or_control: 2,
                packed_cell: crate::sim::base_plan::pack_base_plan_cell(-4, 9),
                filled: true,
                retry_count: -8,
            },
            crate::sim::base_plan::BasePlanNode {
                type_or_control: -3,
                packed_cell: 0,
                filled: false,
                retry_count: 6,
            },
        ];
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            1,
            12,
            14,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 750 },
            type_ref,
            crate::map::entities::EntityCategory::Structure,
            0,
            5,
            false,
        );
        entity.base_plan_type_index = 2;
        entity.base_plan_is_defense = true;
        entity.base_plan_has_undeploy_target = true;
        sim.substrate.entities.insert(entity);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "base-plan", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert_eq!(restored.houses[&owner].base_plan.percent_built, -17);
        assert_eq!(restored.houses[&owner].base_plan.nodes.len(), 2);
        assert_eq!(restored.houses[&owner].base_plan.nodes[0].retry_count, -8);
        let restored_entity = restored.substrate.entities.get(1).unwrap();
        assert_eq!(restored_entity.base_plan_type_index, 2);
        assert!(restored_entity.base_plan_is_defense);
        assert!(restored_entity.base_plan_has_undeploy_target);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_05_house_and_techno_base_defense_state_roundtrip_with_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 10_000, 10);
        house.strategy_emergency.set_state_four();
        house.strategy_emergency.set_all_to_hunt_bias();
        house.strategy_emergency.note_building_attack(-17);
        house.strategy_emergency.note_building_attacker(3);
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        let mut responder =
            crate::sim::game_entity::GameEntity::test_default(1, "E1", "Computer1", 4, 5);
        responder.base_defense_response.recruitable_a = false;
        responder.base_defense_response.recruitable_b = true;
        responder.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(9)));
        responder.base_defense_response.cooldown_start_frame = -11;
        responder.base_defense_response.cooldown_duration_frames = 225;
        sim.substrate.entities.insert(responder);
        let script_id = sim.interner.intern("BaseDefenseScript");
        let task_force_id = sim.interner.intern("BaseDefenseTaskForce");
        let team_type_id = sim.interner.intern("BaseDefenseTeamType");
        let ai_trigger_id = sim.interner.intern("BaseDefenseAITrigger");
        let member_type = sim.interner.intern("E1");
        let member_identity = crate::sim::team_script_vm::TeamMemberTypeIdentity {
            category: crate::rules::object_type::ObjectCategory::Infantry,
            id: member_type,
        };
        sim.team_script_vm
            .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
                id: script_id,
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                    action_id: 2,
                    argument: 0,
                }],
            });
        sim.team_script_vm.register_task_force(
            crate::sim::team_script_vm::TeamTaskForceDefinition {
                id: task_force_id,
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                group: 7,
                entries: vec![crate::sim::team_script_vm::TeamTaskForceEntry {
                    member_type: member_identity,
                    count: 1,
                }],
            },
        );
        sim.team_script_vm
            .register_team_type(crate::sim::team_script_vm::TeamTypeDefinition {
                id: team_type_id,
                script_id,
                task_force_id,
                priority: 0,
                is_base_defense: true,
                suicide: false,
                aggressive: false,
                combined_movement_zone: crate::rules::locomotor_type::MovementZone::Amphibious,
                base_zone_relation_enforced: false,
                transport_crossing_required: true,
            });
        sim.team_script_vm.register_ai_trigger(
            crate::sim::team_script_vm::TeamAiTriggerDefinition {
                id: ai_trigger_id,
                tokens: std::array::from_fn(|index| format!("raw-token-{index}")),
                display_name: "Base defense trigger".to_string(),
                enabled: true,
                primary_team_type: Some(team_type_id),
                owner: Some(crate::sim::team_script_vm::TeamAiTriggerOwner::Country(
                    crate::rules::ruleset::CountryIdx(4),
                )),
                threshold: 7,
                condition: 6,
                object_type: Some(member_identity),
                comparison_mask: std::array::from_fn(|index| index as u8),
                weights: [
                    crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
                    crate::util::native_x87::NativeF64Bits::from_bits(2.5_f64.to_bits()),
                    crate::util::native_x87::NativeF64Bits::from_bits(3.5_f64.to_bits()),
                ],
                storage_flag_d0: true,
                storage_i32_ac: -9,
                storage_flag_d1: false,
                secondary_team_type: None,
                difficulty_enabled: [true, false, true],
                source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
            },
        );
        let team_id = sim.team_script_vm.create_team_from_type(
            owner,
            team_type_id,
            &[crate::sim::team_script_vm::TeamScriptMember {
                entity_id: 1,
                member_type: member_identity,
            }],
            None,
            sim.session.binary_frame as i32,
        );
        assert_eq!(
            sim.team_script_vm
                .suspend_teams_for_base_defense(owner, 1, -12, 1800),
            vec![1]
        );
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_strategy", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("v102 snapshot").sim;
        let emergency = &restored.houses[&owner].strategy_emergency;
        assert_eq!(emergency.mode(), 4);
        assert!(emergency.all_to_hunt_bias());
        assert_eq!(emergency.last_building_attack_frame(), -17);
        assert_eq!(emergency.last_attacker_house_index(), 3);
        let restored_responder = restored.substrate.entities.get(1).unwrap();
        let response = restored_responder.base_defense_response;
        assert!(!response.recruitable_a);
        assert!(response.recruitable_b);
        assert_eq!(
            restored_responder.archive_target(),
            Some(crate::sim::combat::TargetKind::Entity(9))
        );
        assert_eq!(response.cooldown_start_frame, -11);
        assert_eq!(response.cooldown_duration_frames, 225);
        let team = restored.team_script_vm.team(team_id).unwrap();
        assert!(team.members().is_empty());
        assert_eq!(restored.team_script_vm.registry_counts(), (1, 1, 1, 1));
        assert_eq!(restored.team_script_vm.team_type_order(), &[team_type_id]);
        let restored_team_type = restored
            .team_script_vm
            .team_type(team_type_id)
            .expect("persisted TeamType");
        assert_eq!(
            restored_team_type.combined_movement_zone,
            crate::rules::locomotor_type::MovementZone::Amphibious
        );
        assert!(!restored_team_type.base_zone_relation_enforced);
        assert!(restored_team_type.transport_crossing_required);
        assert_eq!(restored.team_script_vm.script_order(), &[script_id]);
        assert_eq!(restored.team_script_vm.task_force_order(), &[task_force_id]);
        assert_eq!(restored.team_script_vm.ai_trigger_order(), &[ai_trigger_id]);
        let restored_script = restored
            .team_script_vm
            .script(script_id)
            .expect("persisted ScriptType");
        assert_eq!(
            restored_script.source,
            crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(restored_script.actions.len(), 1);
        assert_eq!(restored_script.actions[0].action_id, 2);
        assert_eq!(restored_script.actions[0].argument, 0);
        let restored_task_force = restored
            .team_script_vm
            .task_force(task_force_id)
            .expect("persisted TaskForce");
        assert_eq!(
            restored_task_force.source,
            crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(restored_task_force.entries.len(), 1);
        assert_eq!(restored_task_force.entries[0].member_type, member_identity);
        assert_eq!(restored_task_force.entries[0].count, 1);
        assert_eq!(restored_task_force.group, 7);
        let restored_trigger = restored
            .team_script_vm
            .ai_trigger(ai_trigger_id)
            .expect("persisted typed AITriggerType");
        assert_eq!(restored_trigger.tokens[11], "raw-token-11");
        assert_eq!(restored_trigger.display_name, "Base defense trigger");
        assert!(restored_trigger.enabled);
        assert_eq!(restored_trigger.primary_team_type, Some(team_type_id));
        assert_eq!(
            restored_trigger.owner,
            Some(crate::sim::team_script_vm::TeamAiTriggerOwner::Country(
                crate::rules::ruleset::CountryIdx(4)
            ))
        );
        assert_eq!(restored_trigger.threshold, 7);
        assert_eq!(restored_trigger.condition, 6);
        assert_eq!(restored_trigger.object_type, Some(member_identity));
        assert_eq!(restored_trigger.comparison_mask[31], 31);
        assert_eq!(
            restored_trigger.weights,
            [
                crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
                crate::util::native_x87::NativeF64Bits::from_bits(2.5_f64.to_bits()),
                crate::util::native_x87::NativeF64Bits::from_bits(3.5_f64.to_bits()),
            ]
        );
        assert!(restored_trigger.storage_flag_d0);
        assert_eq!(restored_trigger.storage_i32_ac, -9);
        assert!(!restored_trigger.storage_flag_d1);
        assert_eq!(restored_trigger.secondary_team_type, None);
        assert_eq!(restored_trigger.difficulty_enabled, [true, false, true]);
        assert_eq!(
            restored_trigger.source,
            crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
        );
        assert_eq!(
            team.response_suspension_state(),
            (true, true, true, -12, 1800)
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn wave_active_inactive_dummy_links_roundtrip_with_identical_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::projectile::ProjectileCoord;
        use crate::sim::wave::{Wave, WaveRecordedCell, WaveUpdateContext};
        use crate::util::native_x87::NativeF64Bits;

        let mut sim = Simulation::new();
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("DLPH");
        let mut owner_ids = Vec::new();
        for ordinal in 1..=3 {
            let stable_id = sim.allocate_stable_id();
            owner_ids.push(stable_id);
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                stable_id,
                ordinal,
                2,
                0,
                0,
                owner,
                Health { current: 200 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            );
            entity.in_logic_vector = true;
            sim.substrate.entities.insert(entity);
            sim.substrate
                .logic
                .try_push(stable_id)
                .expect("owner Logic registration");
        }

        let active_id = sim.allocate_stable_id();
        let active_target = crate::sim::combat::TargetKind::Cell(8, 2);
        let active_source = ProjectileCoord::new(256, 512, 0);
        let active_endpoint = ProjectileCoord::new(2048, 512, 0);
        let mut active = Wave::new_owned(
            0,
            owner_ids[0],
            active_target,
            active_source,
            active_endpoint,
        );
        assert!(!active.initialize(
            WaveUpdateContext {
                owner_position: Some(active_source),
                owner_current_target: Some(active_target),
                target_position: Some(active_endpoint),
            },
            None,
        ));
        let constructor_direction = active.direction_octant;
        let moved_endpoint = ProjectileCoord::new(0, 512, 0);
        let _ = active.advance(
            WaveUpdateContext {
                owner_position: Some(active_source),
                owner_current_target: Some(active_target),
                target_position: Some(moved_endpoint),
            },
            None,
        );
        assert_eq!(
            active.direction_octant, constructor_direction,
            "live geometry refresh retains constructor-only +0x1CC"
        );
        let moved_edges = active.edge_geometry;
        sim.admit_wave(active_id, active);
        sim.active_wave_links.insert(owner_ids[0], active_id);

        let inactive_id = sim.allocate_stable_id();
        let mut inactive = Wave::new_owned(
            0,
            owner_ids[1],
            crate::sim::combat::TargetKind::Cell(9, 2),
            ProjectileCoord::new(512, 512, 0),
            ProjectileCoord::new(2304, 512, 50),
        );
        inactive.active_geometry = false;
        inactive.decaying = true;
        inactive.fade_in = NativeF64Bits::from_bits(0x3fe0_0000_0000_0000);
        inactive.fade_out = NativeF64Bits::from_bits(0x3fc9_9999_a000_0000);
        inactive.direction_octant = 6;
        inactive.replace_recorded_cells(vec![WaveRecordedCell::real(7, 2)]);
        sim.admit_wave(inactive_id, inactive);
        sim.active_wave_links.insert(owner_ids[1], inactive_id);

        let dummy_id = sim.allocate_stable_id();
        let mut dummy = Wave::new_owned(
            0,
            owner_ids[2],
            crate::sim::combat::TargetKind::Cell(10, 2),
            ProjectileCoord::new(768, 512, 0),
            ProjectileCoord::new(2560, 512, 50),
        );
        dummy.active_geometry = false;
        dummy.decaying = true;
        dummy.direction_octant = 2;
        dummy.replace_recorded_cells(vec![WaveRecordedCell::shared_dummy()]);
        sim.admit_wave(dummy_id, dummy);
        sim.active_wave_links.insert(owner_ids[2], dummy_id);

        let expected_hash = sim.state_hash();
        let bytes = GameSnapshot::save(&sim, 0, 0, "wave-state.map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("Wave snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("Wave identities restore structurally");

        assert_eq!(restored.state_hash(), expected_hash);
        assert_eq!(restored.active_wave_links, sim.active_wave_links);
        assert_eq!(restored.waves.get(active_id), sim.waves.get(active_id));
        assert_eq!(
            restored.waves.get(active_id).unwrap().direction_octant,
            constructor_direction
        );
        assert_eq!(
            restored.waves.get(active_id).unwrap().edge_geometry,
            moved_edges
        );
        assert_eq!(restored.waves.get(inactive_id), sim.waves.get(inactive_id));
        assert_eq!(restored.waves.get(dummy_id), sim.waves.get(dummy_id));
        assert_eq!(
            restored.waves.get(dummy_id).unwrap().recorded_cells,
            vec![WaveRecordedCell::shared_dummy()],
        );
    }

    #[test]
    fn building_anim_slot_restore_rejects_invalid_graph_before_mutation() {
        for case in 0..8 {
            let (mut sim, rules, owner) = crate::sim::building_art::slot_test_fixture();
            let anim = sim
                .set_building_anim_slot(owner, 3, false, false, 0, &rules)
                .unwrap();
            let expected = match case {
                0 | 1 => {
                    let target = if case == 0 { 999 } else { owner };
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .building_anim_slots[3] = Some(target);
                    SnapshotRestoreError::MissingBuildingSlotAnim {
                        owner_id: owner,
                        slot: 3,
                        anim_id: target,
                    }
                }
                2 => {
                    sim.substrate.entities.get_mut(owner).unwrap().category =
                        crate::map::entities::EntityCategory::Unit;
                    SnapshotRestoreError::InvalidBuildingAnimSlotOwner {
                        owner_id: owner,
                        slot: 3,
                    }
                }
                3 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .building_anim_slots[4] = Some(anim);
                    SnapshotRestoreError::DuplicateBuildingSlotAnim {
                        anim_id: anim,
                        first_owner: owner,
                        first_slot: 3,
                        second_owner: owner,
                        second_slot: 4,
                    }
                }
                4 => {
                    let second = sim.allocate_stable_id();
                    let mut entity = sim.substrate.entities.get(owner).unwrap().clone();
                    entity.stable_id = second;
                    sim.substrate.entities.insert(entity);
                    SnapshotRestoreError::DuplicateBuildingSlotAnim {
                        anim_id: anim,
                        first_owner: owner,
                        first_slot: 3,
                        second_owner: second,
                        second_slot: 3,
                    }
                }
                5 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .damage_fire_anim_ids[0] = Some(999);
                    SnapshotRestoreError::MissingBuildingSlotAnim {
                        owner_id: owner,
                        slot: 21,
                        anim_id: 999,
                    }
                }
                6 => {
                    sim.substrate
                        .entities
                        .get_mut(owner)
                        .unwrap()
                        .damage_fire_anim_ids[0] = Some(anim);
                    SnapshotRestoreError::DuplicateBuildingSlotAnim {
                        anim_id: anim,
                        first_owner: owner,
                        first_slot: 3,
                        second_owner: owner,
                        second_slot: 21,
                    }
                }
                _ => {
                    let entity = sim.substrate.entities.get_mut(owner).unwrap();
                    entity.building_anim_slots[3] = None;
                    entity.damage_fire_anim_ids[0] = Some(anim);
                    entity.category = crate::map::entities::EntityCategory::Unit;
                    SnapshotRestoreError::InvalidBuildingAnimSlotOwner {
                        owner_id: owner,
                        slot: 21,
                    }
                }
            };
            let before = GameSnapshot::save(&sim, 0, 0, "slot-rejection.map", 0);
            assert_eq!(
                sim.restore_after_snapshot_load(),
                Err(expected),
                "case {case}"
            );
            assert_eq!(
                GameSnapshot::save(&sim, 0, 0, "slot-rejection.map", 0),
                before,
                "restore must reject atomically: {case}"
            );
            assert_eq!(sim.anim(anim).unwrap().building_slot, Some((owner, 3)));
        }
    }

    #[test]
    fn building_anim_slot_restore_accepts_unattached_deferred_destroy() {
        let (mut sim, rules, owner) = crate::sim::building_art::slot_test_fixture();
        let anim = sim
            .set_building_anim_slot(owner, 3, false, false, 0, &rules)
            .unwrap();
        assert!(sim.anim(anim).unwrap().owner_entity.is_none());
        sim.destroy_anim(anim, &rules);
        assert!(sim.substrate.pending_delete.contains(&anim));
        assert_eq!(
            sim.entities().get(owner).unwrap().building_anim_slots[3],
            Some(anim)
        );
        sim.substrate.anims.get_mut(anim).unwrap().building_slot = None;
        sim.restore_after_snapshot_load().unwrap();
        assert_eq!(sim.anim(anim).unwrap().building_slot, Some((owner, 3)));
        assert!(sim.substrate.pending_delete.contains(&anim));
    }

    #[test]
    fn building_anim_slots_roundtrip_with_runtime_and_reverse_index() {
        let (mut sim, rules, id) = crate::sim::building_art::slot_test_fixture();
        let load_rng = crate::sim::rng::SimRng::new(0).logical_state();
        let anim = sim
            .set_building_anim_slot(id, 3, true, false, 0, &rules)
            .unwrap();
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime
            .current_frame = 17;
        sim.substrate.anims.get_mut(anim).unwrap().runtime.paused = true;
        let building = sim.substrate.entities.get_mut(id).unwrap();
        building.building_anim_effect_replay[16] = true;
        building.building_last_operational = true;
        building.building_stuff_enabled = false;
        building.building_storage.amounts = [
            crate::util::native_x87::NativeF32Bits::from_bits(0x7fc01234),
            crate::util::native_x87::NativeF32Bits::NEGATIVE_ZERO,
            crate::util::native_x87::NativeF32Bits::ONE,
            crate::util::native_x87::NativeF32Bits::from_bits(0xbf800000),
        ];
        building.building_storage.refinery_tier = -17;
        assert_ne!(sim.scenario_rng.logical_state(), load_rng);
        let bytes = GameSnapshot::save(&sim, 1, 2, "building-anim.map", 0);
        // Native Scenario load consumes the saved RNG bytes, then reseeds zero.
        // The RandomRate constructor draw remains observable before saving;
        // compare all saved slot/runtime state against the native load state.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected = sim.state_hash();
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            SNAPSHOT_VERSION
        );
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.scenario_rng.logical_state(), load_rng);
        assert_eq!(
            restored.entities().get(id).unwrap().building_anim_slots[3],
            Some(anim)
        );
        assert!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_damage_state_active
        );
        assert_eq!(restored.anim(anim).unwrap().runtime.current_frame, 17);
        assert!(restored.anim(anim).unwrap().runtime.paused);
        assert!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_anim_effect_replay[16]
        );
        assert!(
            restored
                .entities()
                .get(id)
                .unwrap()
                .building_last_operational
        );
        assert!(!restored.entities().get(id).unwrap().building_stuff_enabled);
        assert_eq!(
            restored.entities().get(id).unwrap().building_storage,
            sim.entities().get(id).unwrap().building_storage
        );
        assert_eq!(restored.anim(anim).unwrap().building_slot, Some((id, 3)));
        assert_eq!(restored.state_hash(), expected);
    }

    #[test]
    fn gsi_13_06_body_frame_counter_roundtrips_and_changes_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::components::{DriveCoord, Health, ShipLocomotionRuntime};
        use crate::sim::game_entity::GameEntity;
        use crate::util::fixed_math::{SIM_HALF, SIM_ONE};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Soviet");
        let type_ref = sim.interner.intern("DRON");
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                1,
                5,
                5,
                0,
                0,
                owner,
                Health { current: 100 },
                type_ref,
                EntityCategory::Unit,
                0,
                5,
                false,
            ));
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let zero_counter_hash = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .expect("Terror Drone")
            .body_frame_counter = u32::MAX;
        let populated_counter_hash = sim.state_hash();
        assert_ne!(populated_counter_hash, zero_counter_hash);

        let ship_head = DriveCoord::cell(6, 5, 0);
        let entity = sim.substrate.entities.get_mut(1).expect("SHP unit");
        entity.foot_speed.applied_fraction = SIM_HALF;
        entity.foot_speed.cached_current_speed = 10;
        entity.ship_locomotion = Some(ShipLocomotionRuntime {
            destination: Some(ship_head),
            head_to: Some(ship_head),
            target_speed_fraction: SIM_ONE,
            ..Default::default()
        });
        let populated_shp_state_hash = sim.state_hash();
        assert_ne!(populated_shp_state_hash, populated_counter_hash);

        let bytes = GameSnapshot::save(&sim, 1, 2, "counter.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v82 body-counter snapshot")
            .sim;
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .expect("restored Terror Drone")
                .body_frame_counter,
            u32::MAX
        );
        let restored_ship = restored
            .substrate
            .entities
            .get(1)
            .expect("restored SHP unit")
            .ship_locomotion
            .as_ref()
            .expect("restored Ship runtime");
        assert_eq!(restored_ship.destination, Some(ship_head));
        assert_eq!(restored_ship.head_to, Some(ship_head));
        assert_eq!(restored_ship.target_speed_fraction, SIM_ONE);
        let restored_owner_speed = &restored.substrate.entities.get(1).unwrap().foot_speed;
        assert_eq!(restored_owner_speed.applied_fraction, SIM_HALF);
        assert_eq!(restored_owner_speed.cached_current_speed, 10);
        assert_eq!(restored.state_hash(), populated_shp_state_hash);
    }

    #[test]
    fn gsi_05_10_pending_building_fire_roundtrips_and_changes_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::combat::combat_weapon::WeaponSlot;
        use crate::sim::components::Health;
        use crate::sim::game_entity::{GameEntity, PendingBuildingFire};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Soviet");
        let type_ref = sim.interner.intern("NATSLA");
        let entity = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health { current: 600 },
            type_ref,
            EntityCategory::Structure,
            0,
            8,
            false,
        );
        sim.substrate.entities.insert(entity);
        // Full snapshot load resets Scenario RNG to Seed0. Compare the
        // authoritative delayed-fire state on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let without_latch = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(1)
            .expect("Tesla Coil")
            .pending_building_fire = Some(PendingBuildingFire {
            remaining_ticks: 17,
            weapon_slot: WeaponSlot::Secondary,
        });
        let with_latch = sim.state_hash();
        assert_ne!(with_latch, without_latch);

        let bytes = GameSnapshot::save(&sim, 1, 2, "delay.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v75 delayed-fire snapshot")
            .sim;
        assert_eq!(
            restored
                .substrate
                .entities
                .get(1)
                .expect("restored Tesla Coil")
                .pending_building_fire,
            Some(PendingBuildingFire {
                remaining_ticks: 17,
                weapon_slot: WeaponSlot::Secondary,
            })
        );
        assert_eq!(restored.state_hash(), with_latch);
    }

    #[test]
    fn gsi_01_04_pending_exit_command_roundtrips_without_terminal_edge() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
        sim.session.house_order.push(owner);
        let hash_without_pending_input = sim.state_hash();
        sim.queue_command(CommandEnvelope::new(owner, 17, Command::ExitMatch));
        assert_eq!(
            sim.state_hash(),
            hash_without_pending_input,
            "pending EXIT follows the existing external-command hash convention"
        );
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 1, 2, "abort.map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("v75 EXIT snapshot").sim;

        assert_eq!(
            restored.pending_commands_for_tests(),
            sim.pending_commands_for_tests()
        );
        assert!(!restored.quit_requested);
        assert_eq!(restored.take_executed_exit_owner(), None);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn pending_game_speed_transition_roundtrips_in_current_version_and_executes_once() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::house_state::HouseState;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
        sim.session.house_order.push(owner);
        let hash_without_pending_input = sim.state_hash();
        sim.queue_command(CommandEnvelope::new(
            owner,
            1,
            Command::SetGameSpeed { speed: 4 },
        ));
        assert_eq!(sim.state_hash(), hash_without_pending_input);

        let bytes = GameSnapshot::save(&sim, 1, 2, "speed.map", 0);
        let header = GameSnapshot::read_header(&bytes).expect("current GameSpeed header");
        assert_eq!(header.version, SNAPSHOT_VERSION);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("current GameSpeed snapshot")
            .sim;
        assert_eq!(
            restored.pending_commands_for_tests(),
            sim.pending_commands_for_tests()
        );
        assert_eq!(restored.session.game_options.game_speed, 1);
        assert_eq!(restored.projected_in_game_options_speed(), Some(4));

        let due = restored.take_due_commands();
        let result = restored.advance_tick(
            &due,
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
        assert_eq!(result.executed_commands, 1);
        assert_eq!(restored.session.game_options.game_speed, 4);
        assert!(restored.pending_commands_for_tests().is_empty());
        assert_eq!(result.state_hash, restored.state_hash());

        let second = restored.advance_tick(
            &[],
            None,
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
        assert_eq!(second.executed_commands, 0);
        assert_eq!(restored.session.game_options.game_speed, 4);
    }

    #[test]
    fn gsi_01_04_mid_savour_snapshot_preserves_remaining_frames_without_replaying_eva() {
        use crate::sim::house_state::{HouseOutcomeKind, HouseState};
        use crate::sim::world::SimSoundEvent;

        let mut original = Simulation::with_seed(0x104);
        original.session.tick = 145;
        let owner = original.interner.intern("Americans");
        original
            .houses
            .insert(owner, HouseState::new(owner, 0, None, true, 10_000, 10));
        let before_outcome = original.state_hash();
        assert!(
            original
                .houses
                .get_mut(&owner)
                .expect("house")
                .flag_to_win(100, 90)
        );
        assert_ne!(
            original.state_hash(),
            before_outcome,
            "accepted outcome and Savour target are hash-relevant"
        );
        let accepted_hash = original.state_hash();
        original
            .houses
            .get_mut(&owner)
            .and_then(|house| house.outcome_state.as_mut())
            .expect("outcome")
            .savour_until_tick += 1;
        assert_ne!(
            original.state_hash(),
            accepted_hash,
            "the absolute Savour target itself is hash-relevant"
        );
        original
            .houses
            .get_mut(&owner)
            .and_then(|house| house.outcome_state.as_mut())
            .expect("outcome")
            .savour_until_tick -= 1;
        original.sound_events.push(SimSoundEvent::MatchOutcome {
            owner,
            kind: HouseOutcomeKind::Victory,
        });
        // Full snapshot load resets Scenario RNG to Seed0; compare hashes on
        // that canonical post-load cursor.
        original.scenario_rng = crate::sim::rng::SimRng::new(0);
        let saved_hash = original.state_hash();
        let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert!(
            restored.sound_events.is_empty(),
            "the already-issued outcome EVA edge must not survive save/load"
        );
        restored
            .restore_after_snapshot_load()
            .expect("mid-Savour outcome state is structurally valid");
        assert_eq!(restored.state_hash(), saved_hash);

        let restored_house = restored.houses.get(&owner).expect("restored house");
        let outcome = restored_house.outcome_state.expect("accepted outcome");
        assert_eq!(outcome.kind, HouseOutcomeKind::Victory);
        assert!(!outcome.exit_ready);
        assert_eq!(outcome.savour_until_tick - restored.session.tick, 45);

        let mut boundary = restored_house.clone();
        assert!(!boundary.advance_outcome_savour(189));
        assert!(boundary.advance_outcome_savour(190));
    }

    #[test]
    fn gsi_01_04_malformed_current_snapshot_naked_terminal_flags_are_rejected() {
        use crate::sim::house_state::HouseState;

        for terminal_flag in ["is_defeated", "has_won", "has_lost"] {
            let mut malformed = Simulation::new();
            let owner = malformed.interner.intern("Americans");
            let mut house = HouseState::new(owner, 0, None, true, 10_000, 10);
            match terminal_flag {
                "is_defeated" => house.is_defeated = true,
                "has_won" => house.has_won = true,
                "has_lost" => house.has_lost = true,
                _ => unreachable!(),
            }
            malformed.houses.insert(owner, house);

            let bytes = GameSnapshot::save(&malformed, 11, 22, "", 33);
            let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
            assert_eq!(
                restored.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                    owner,
                    reason: "terminal flags require serialized outcome authority",
                }),
                "naked {terminal_flag} must not enter a world whose app bridge can only consume outcome_state"
            );
        }
    }

    #[test]
    fn gsi_04_20_mid_fade_lighting_roundtrip_preserves_hash_and_next_rung() {
        let mut original = Simulation::with_seed(0x420);
        original.session.binary_frame = 20;
        original.session.lighting.normal.red_percent = 93;
        original.session.lighting.ion.blue_percent = 71;
        original.session.lighting.current_ambient = 95;
        original.session.lighting.target_ambient = 87;
        original.session.lighting.selected_profile =
            crate::sim::scenario_session::ScenarioLightingProfile::Ion;
        original.session.lighting.transition_timer = crate::sim::timer::CdTimer::from_raw(17, 3);

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // lighting persistence on that same post-load cursor.
        original.scenario_rng = crate::sim::rng::SimRng::new(0);
        let saved_hash = original.state_hash();
        let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("v65 lighting snapshot")
            .sim;

        assert_eq!(restored.session.lighting, original.session.lighting);
        assert_eq!(restored.state_hash(), saved_hash);

        let original_due = original
            .session
            .lighting
            .advance_transition_if_due(20, true, 180, 20);
        let restored_due = restored
            .session
            .lighting
            .advance_transition_if_due(20, true, 180, 20);
        assert_eq!(restored_due, original_due);
        assert_eq!(restored.session.lighting, original.session.lighting);
        assert_eq!(restored.state_hash(), original.state_hash());
    }

    /// `HouseClass+0x242` rides the raw House block in the native save; a
    /// v132 snapshot carries it and the restored hash still matches.
    #[test]
    fn house_harvester_no_ore_latch_roundtrips_at_v132() {
        let mut sim = Simulation::with_seed(0x242);
        let owner = sim.interner.intern("Americans");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 5_000, 10);
        house.harvester_no_ore = true;
        sim.houses.insert(owner, house);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "house_no_ore_latch", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes)
            .expect("v132 house latch snapshot")
            .sim;
        assert!(restored.houses[&owner].harvester_no_ore);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    /// `HouseClass+0x57D4` and the low-power guard ride the House block in
    /// the native save; a v133 snapshot carries them and the restored hash
    /// still matches.
    #[test]
    fn house_eva_advice_state_roundtrips_at_v133() {
        let mut sim = Simulation::with_seed(0x57D4);
        let owner = sim.interner.intern("Americans");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 5_000, 10);
        house.eva_funds_timer = crate::sim::house_state::HouseFrameTimer {
            start_frame: 1_234,
            duration: 2_880,
        };
        house.eva_low_power_guard = true;
        sim.houses.insert(owner, house);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "house_eva_advice", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes)
            .expect("v133 house EVA snapshot")
            .sim;
        assert_eq!(
            restored.houses[&owner].eva_funds_timer,
            crate::sim::house_state::HouseFrameTimer {
                start_frame: 1_234,
                duration: 2_880,
            }
        );
        assert!(restored.houses[&owner].eva_low_power_guard);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_18_spy_sat_latch_and_erased_map_knowledge_roundtrip_at_v65() {
        let mut sim = Simulation::with_seed(0x418);
        sim.fog.width = 12;
        sim.fog.height = 12;
        let owner = sim.interner.intern("Soviet");
        let gapper = sim.interner.intern("Americans");
        let mut house = crate::sim::house_state::HouseState::new(owner, 1, None, true, 10_000, 10);
        house.spy_sat_active = true;
        house.map_is_clear = true;
        sim.houses.insert(owner, house);
        sim.fog.reveal_all_for_owner(owner);
        crate::sim::vision::apply_gap_generators(&mut sim.fog, &[(gapper, 6, 6, 2)], &sim.interner);
        assert!(sim.fog.is_cell_revealed(owner, 0, 0));
        assert!(!sim.fog.is_cell_revealed(owner, 6, 6));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // SpySat/map-knowledge persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_18_shroud", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("v65 SpySat snapshot").sim;

        assert!(restored.houses[&owner].spy_sat_active);
        assert!(restored.houses[&owner].map_is_clear);
        assert!(restored.fog.is_cell_revealed(owner, 0, 0));
        assert!(!restored.fog.is_cell_revealed(owner, 6, 6));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_15_active_tube_detachment_and_exact_z_roundtrip_hash() {
        use crate::map::tube_facts::TubeId;
        use crate::sim::components::DriveCoord;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

        let mut sim = Simulation::new();
        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "Americans", 5, 7);
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = false;
        entity.in_logic_vector = true;
        entity.position.z = 3;
        entity.position.exact_z_leptons = Some(-37);
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(9),
            cursor: 2,
            target: DriveCoord {
                x: 1_536,
                y: 2_048,
                z: -19,
            },
        });
        sim.substrate.entities.insert(entity);
        sim.substrate
            .logic
            .try_push(entity_id)
            .expect("active TubeMovement fixture enters LogicClass order");
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // TubeMovement persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_15_tube", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let mut restored = GameSnapshot::load(&bytes)
            .expect("current tube snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("active detached tube object restores");

        let restored_entity = restored
            .substrate
            .entities
            .get(entity_id)
            .expect("tube mover restored");
        assert_eq!(restored_entity.position.z, 3);
        assert_eq!(restored_entity.position.exact_z_leptons, Some(-37));
        assert_eq!(
            restored_entity.low_bridge_tube_state,
            Some(LowBridgeTubeMovementState {
                tube_id: TubeId(9),
                cursor: 2,
                target: DriveCoord {
                    x: 1_536,
                    y: 2_048,
                    z: -19,
                },
            })
        );
        assert!(!restored_entity.lifecycle.cell_marked);
        assert!(
            !restored
                .substrate
                .occupancy
                .contains_entity(5, 7, entity_id)
        );
        assert_eq!(
            restored.substrate.cell_occupation.vehicle_bits(
                5,
                7,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            ),
            0
        );
        assert_eq!(restored.substrate.raw_cell_occupation.ground_bits(5, 7), 0);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_07_wall_sell_pending_command_house_mode_roundtrip_and_hash() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Receiver");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
        house.player_control = true;
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        sim.session.game_mode_nonzero = true;
        sim.queue_command(crate::sim::command::CommandEnvelope::new(
            owner,
            17,
            crate::sim::command::Command::SellWallAtCell { x: -3, y: 9 },
        ));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // pending-command/house-mode persistence on that post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        sim.houses.get_mut(&owner).unwrap().player_control = false;
        assert_ne!(sim.state_hash(), expected_hash);
        sim.houses.get_mut(&owner).unwrap().player_control = true;
        sim.session.game_mode_nonzero = false;
        assert_ne!(sim.state_hash(), expected_hash);
        sim.session.game_mode_nonzero = true;

        let bytes = GameSnapshot::save(&sim, 1, 2, "wall.map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v60 wall-sale snapshot")
            .sim;
        assert_eq!(
            restored.pending_commands_for_tests(),
            sim.pending_commands_for_tests()
        );
        assert!(restored.houses.get(&owner).unwrap().player_control);
        assert!(restored.session.game_mode_nonzero);
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_07_damage_v60_air_spatial_armor_berserk_hostile_hit_iq_smoke_capture_anger_and_delay_roundtrip_hash()
     {
        let mut sim = Simulation::new();
        let entity_id = sim.allocate_stable_id();
        let mut aircraft = crate::sim::game_entity::GameEntity::test_default(
            entity_id,
            "ORCA",
            "AMERICANS",
            11,
            7,
        );
        aircraft.air_spatial_bucket = Some(143);
        aircraft.air_spatial_enter_order = 91;
        aircraft.armor_multiplier =
            crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits());
        aircraft.berserk.active = true;
        aircraft.berserk.timer = -17;
        aircraft.was_attacked_by_enemy = true;
        aircraft.damage_smoke_system_id = Some(2);
        aircraft.capture_manager = Some(
            crate::sim::capture_manager::CaptureManagerState::with_victims_for_test(
                3,
                false,
                &[3, 4],
            ),
        );
        aircraft.pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
            start_frame: 11,
            duration_frames: 35,
            source_entity_id: Some(3),
        });
        sim.substrate.entities.insert(aircraft);
        sim.substrate
            .entities
            .insert(crate::sim::game_entity::GameEntity::test_default(
                3,
                "E1",
                "AMERICANS",
                12,
                7,
            ));
        sim.substrate
            .entities
            .insert(crate::sim::game_entity::GameEntity::test_default(
                4,
                "E1",
                "AMERICANS",
                13,
                7,
            ));
        sim.substrate
            .particle_systems
            .insert(crate::sim::particles::ParticleSystem {
                stable_id: 2,
                in_logic_vector: false,
                type_id: crate::rules::particle_system_type::ParticleSystemTypeId(0),
                coords: glam::IVec3::ZERO,
                offset: glam::IVec3::ZERO,
                particles: Vec::new(),
                spawn_timer: crate::util::fixed_math::SimFixed::from_num(1),
                lifetime: -1,
                spark_spawn_frames: 0,
                facing: 0x1D,
                directionless: true,
                attached_entity: None,
                owner_entity: Some(entity_id),
                target_coords: glam::IVec3::ZERO,
                owner_house: None,
                done_spawning: false,
            });
        sim.substrate.next_stable_object_id = 5;
        let owner = sim.interner.intern("ComputerIQ");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 51);
        house.current_iq = 2;
        let threat_peer = sim.interner.intern("ThreatPeer");
        house.grudge_scores.insert(threat_peer, 350);
        house.enemy_house = Some(threat_peer);
        sim.houses.insert(owner, house);
        sim.houses.insert(
            threat_peer,
            crate::sim::house_state::HouseState::new(threat_peer, 1, None, false, 0, 51),
        );
        sim.session.house_order.extend([owner, threat_peer]);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // damage-authority persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .air_spatial_enter_order = 92;
        assert_ne!(sim.state_hash(), expected_hash, "vector order is hashed");
        {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            entity.air_spatial_enter_order = 91;
            entity.air_spatial_bucket = Some(144);
        }
        assert_ne!(sim.state_hash(), expected_hash, "bucket identity is hashed");
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .air_spatial_bucket = Some(143);
        assert_eq!(sim.state_hash(), expected_hash);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .armor_multiplier = crate::util::native_x87::NativeF64Bits::ONE;
        assert_ne!(sim.state_hash(), expected_hash, "instance armor is hashed");
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .armor_multiplier =
            crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits());
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .berserk
            .timer = -18;
        assert_ne!(sim.state_hash(), expected_hash, "berserk timer is hashed");
        {
            let entity = sim.substrate.entities.get_mut(entity_id).unwrap();
            entity.berserk.timer = -17;
            entity.berserk.active = false;
        }
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "berserk active byte is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .berserk
            .active = true;
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .was_attacked_by_enemy = false;
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "hostile-hit latch is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .was_attacked_by_enemy = true;
        sim.houses.get_mut(&owner).unwrap().current_iq = 1;
        assert_ne!(sim.state_hash(), expected_hash, "CurrentIQ is hashed");
        sim.houses.get_mut(&owner).unwrap().current_iq = 2;
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(threat_peer, 351);
        assert_ne!(sim.state_hash(), expected_hash, "anger score is hashed");
        {
            let house = sim.houses.get_mut(&owner).unwrap();
            house.grudge_scores.insert(threat_peer, 350);
            house.enemy_house = None;
        }
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "selected enemy identity is hashed"
        );
        sim.houses.get_mut(&owner).unwrap().enemy_house = Some(threat_peer);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .damage_smoke_system_id = None;
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "damage-Smoke identity is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .damage_smoke_system_id = Some(2);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .capture_manager
            .as_mut()
            .unwrap()
            .reverse_nodes_for_test();
        assert_ne!(sim.state_hash(), expected_hash, "MCNode order is hashed");
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .capture_manager
            .as_mut()
            .unwrap()
            .reverse_nodes_for_test();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_07_air_spatial", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let mut restored = GameSnapshot::load(&bytes).expect("v60 snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("damage-Smoke pointer resolves through ParticleSystemStore");
        let entity = restored.substrate.entities.get(entity_id).unwrap();
        assert_eq!(entity.air_spatial_bucket, Some(143));
        assert_eq!(entity.air_spatial_enter_order, 91);
        assert_eq!(entity.armor_multiplier.bits(), 1.5_f64.to_bits());
        assert!(entity.berserk.active);
        assert_eq!(entity.berserk.timer, -17);
        assert!(entity.was_attacked_by_enemy);
        assert_eq!(entity.damage_smoke_system_id, Some(2));
        assert_eq!(
            entity.pending_c4_detonation,
            Some(crate::sim::components::PendingC4Detonation {
                start_frame: 11,
                duration_frames: 35,
                source_entity_id: Some(3),
            })
        );
        assert_eq!(
            entity
                .capture_manager
                .as_ref()
                .map(|manager| manager.victims().collect::<Vec<_>>()),
            Some(vec![3, 4])
        );
        assert_eq!(restored.houses.get(&owner).unwrap().current_iq, 2);
        assert_eq!(
            restored.houses[&owner].grudge_scores.get(&threat_peer),
            Some(&350)
        );
        assert_eq!(restored.houses[&owner].enemy_house, Some(threat_peer));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_12_raw_occupation_snapshot_roundtrip_preserves_both_planes() {
        let mut sim = Simulation::new();
        let ground_owner = sim.interner.intern("Americans");
        let deck_owner = sim.interner.intern("Russians");
        sim.substrate.raw_cell_occupation.mark_ground(17, 23, 0x23);
        sim.substrate.raw_cell_occupation.mark_deck(17, 23, 0xC4);
        sim.substrate
            .raw_cell_occupation
            .mark_ground_infantry(17, 23, 0x04, ground_owner);
        sim.substrate
            .raw_cell_occupation
            .mark_deck_infantry(17, 23, 0x08, deck_owner);
        sim.substrate.raw_cell_occupation.mark_ground(2, 31, 0x02);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // occupation-plane persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_12_raw_occupation", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .version,
            super::SNAPSHOT_VERSION
        );

        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore transient caches without replacing raw bytes");

        assert_eq!(
            restored.substrate.raw_cell_occupation.ground_bits(17, 23),
            0x27
        );
        assert_eq!(
            restored.substrate.raw_cell_occupation.deck_bits(17, 23),
            0xCC
        );
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .ground_infantry_owner(17, 23),
            Some(ground_owner)
        );
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .deck_infantry_owner(17, 23),
            Some(deck_owner)
        );
        assert_eq!(
            restored.substrate.raw_cell_occupation.ground_bits(2, 31),
            0x02
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    /// The cell `AltObject` slots (`CellClass+0xE0`) are a serialized authority:
    /// which owner holds a cell, and when it was released, cannot be rebuilt
    /// from entity positions or locomotor phase, so they must survive a save
    /// verbatim and reach the state hash.
    #[test]
    fn air_slot_snapshot_roundtrip_preserves_the_cell_holders() {
        let empty_hash = Simulation::new().state_hash();
        let mut sim = Simulation::new();
        assert!(sim.substrate.air_slots.claim(17, 23, 41));
        assert!(sim.substrate.air_slots.claim(2, 31, 99));
        // `0x00487D70` refuses a claim while another object holds the slot,
        // which is what keeps one hovering Jumpjet per cell.
        assert!(!sim.substrate.air_slots.claim(17, 23, 42));
        assert_eq!(sim.substrate.air_slots.holder(17, 23), Some(41));
        let expected_hash = sim.state_hash();
        assert_ne!(
            expected_hash, empty_hash,
            "held air slots must reach the state hash"
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "air_slot_roundtrip", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .version,
            super::SNAPSHOT_VERSION
        );

        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore transient caches without replacing the slots");

        assert_eq!(restored.substrate.air_slots.holder(17, 23), Some(41));
        assert_eq!(restored.substrate.air_slots.holder(2, 31), Some(99));
        assert_eq!(restored.substrate.air_slots.holder(5, 5), None);

        // The post-restore hash check is differential on purpose: a bare
        // `Simulation::new()` does not round-trip its own state hash, which a
        // no-slot control sim shows just as well, so comparing `restored`
        // against `expected_hash` would be measuring that instead. What matters
        // here is that the restored slots are still hash-visible.
        let control = GameSnapshot::save(&Simulation::new(), 0, 0, "air_slot_control", 0);
        let mut control = GameSnapshot::load(&control).expect("control snapshot").sim;
        control
            .restore_after_snapshot_load()
            .expect("restore the control");
        assert_ne!(
            restored.state_hash(),
            control.state_hash(),
            "restored air slots must still reach the state hash"
        );
    }

    #[test]
    fn cell_fog_sensor_and_cloak_state_roundtrips_with_hash() {
        use crate::map::entities::EntityCategory;
        use crate::sim::cloak_disguise::CloakRuntime;
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::sensor_lifecycle::SensorDeposit;

        let mut sim = Simulation::new();
        let viewer = sim.interner.intern("AMERICANS");
        let source_owner = sim.interner.intern("RUSSIANS");
        let type_ref = sim.interner.intern("SUB");
        sim.fog.width = 8;
        sim.fog.height = 8;
        sim.fog
            .insert_fogged_object_footprint(viewer, (3, 3), 91, vec![(3, 3), (4, 3)]);
        sim.fog.sensors_add_at(viewer, (3, 3), 2);
        assert!(sim.fog.set_cloaked_by_house(7, 3, 3));
        assert!(
            sim.fog
                .draw_objects_cloaked(Some(source_owner), source_owner, 7, 3, 3)
        );
        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            entity_id,
            3,
            3,
            0,
            0,
            source_owner,
            Health { current: 600 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        let mut cloak = CloakRuntime::new(0, 9);
        cloak.establish_unlimbo_fully_cloaked();
        entity.cloak = Some(cloak);
        entity.sensor_deposit = Some(SensorDeposit {
            owner: viewer,
            center: (3, 3),
            add_radius: 2,
            remove_radius: 2,
            building_array: false,
            detect_disguise_radius: 0,
        });
        sim.substrate.entities.insert(entity);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // fog/sensor/cloak persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "cell_fog_sensor_cloak", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

        assert_eq!(restored.state_hash(), expected_hash);
        assert_eq!(restored.fog.fogged_objects.len(), 1);
        assert!(restored.fog.has_sensor_for_house(viewer, 3, 3));
        assert!(restored.fog.is_cloaked_by_house(7, 3, 3));
        let restored_entity = restored.substrate.entities.get(entity_id).unwrap();
        assert_eq!(
            restored_entity.cloak,
            sim.substrate.entities.get(entity_id).unwrap().cloak
        );
        assert_eq!(
            restored_entity.sensor_deposit,
            sim.substrate
                .entities
                .get(entity_id)
                .unwrap()
                .sensor_deposit
        );
        let same_hash = restored.state_hash();
        restored
            .substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .sensor_deposit
            .as_mut()
            .unwrap()
            .center = (4, 3);
        assert_ne!(restored.state_hash(), same_hash);
    }

    #[test]
    fn gsi_04_05_hidden_snapshot_and_hash_preserve_counter_and_exit_profile() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        sim.session.map_width = 32;
        sim.session.map_height = 32;
        let entity_id = sim.allocate_stable_id();
        let mut building = GameEntity::test_default(entity_id, "GAREFN", "AMERICANS", 10, 10);
        building.category = EntityCategory::Structure;
        building.foundation = "4x3".to_string();
        let mut profile = crate::rules::object_type::BuildingHiddenOccupancyProfile::default();
        profile.add_occupy[0] = Some((-1, 0));
        profile.add_occupy[1] = Some((-1, -1));
        profile.remove_occupy[0] = Some((3, 1));
        building.building_hidden_occupancy = Some(profile);
        sim.substrate.entities.insert(building);
        sim.add_entity_occupancy(entity_id);

        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // hidden-occupation persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        sim.substrate
            .hidden_occupation
            .exit_building((10, 10), "4x3", profile, Some((32, 32)));
        assert_ne!(sim.state_hash(), expected_hash);
        sim.substrate
            .hidden_occupation
            .enter_building((10, 10), "4x3", profile, Some((32, 32)));
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .building_hidden_occupancy
            .as_mut()
            .unwrap()
            .occupy_height = 4;
        assert_ne!(sim.state_hash(), expected_hash);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .building_hidden_occupancy
            .as_mut()
            .unwrap()
            .occupy_height = 2;
        assert_eq!(sim.state_hash(), expected_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_hidden", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore skipped cell-list caches");

        assert_eq!(restored.substrate.hidden_occupation.count(9, 9), 1);
        assert_eq!(restored.substrate.hidden_occupation.count(13, 11), 0);
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .building_hidden_occupancy,
            Some(profile)
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_16_waypoint_edge_factory_profile_roundtrips_and_hashes() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let entity_id = sim.allocate_stable_id();
        let mut conyard = GameEntity::test_default(entity_id, "GACNST", "AMERICANS", 10, 10);
        conyard.category = EntityCategory::Structure;
        sim.substrate.entities.insert(conyard);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // waypoint-edge profile persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let without_profile = sim.state_hash();
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .determines_waypoint_edge = true;
        let expected_hash = sim.state_hash();
        assert_ne!(expected_hash, without_profile);

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_16_edge_profile", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .determines_waypoint_edge
        );
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn gsi_04_05_reservation_roundtrip_preserves_real_and_house_state_but_clears_dummy() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
        house.base_reservation.update_bounds(3, 4, 5, 6);
        house
            .base_reservation
            .append_perimeter_cell_if_absent(u32::from(3u16) | (u32::from(4u16) << 16));
        sim.houses.insert(owner, house);
        let entity_id = sim.allocate_stable_id();
        let mut building = crate::sim::game_entity::GameEntity::test_default(
            entity_id,
            "GAPOWR",
            "AMERICANS",
            3,
            4,
        );
        building.category = crate::map::entities::EntityCategory::Structure;
        building.owner = owner;
        building.base_reservation_spacing = Some(-3);
        sim.substrate.entities.insert(building);
        let mut reservation_terrain = flat_terrain(4, 5);
        reservation_terrain.test_set_native_allocated_cells(&[(3, 4)]);
        sim.resolved_terrain = Some(reservation_terrain);
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 3, 4, 0);
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 1, 0, 1);
        assert_eq!(
            sim.substrate
                .base_reservations
                .raw_mask(sim.resolved_terrain.as_ref(), 2, 0),
            1 << 1,
            "distinct valid-linear null slots share the dummy before save"
        );
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // base-reservation persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        sim.substrate
            .base_reservations
            .clear(sim.resolved_terrain.as_ref(), 3, 4, 0);
        assert_ne!(sim.state_hash(), expected_hash, "real mask is hashed");
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 3, 4, 0);
        sim.substrate
            .base_reservations
            .clear(sim.resolved_terrain.as_ref(), 2, 0, 1);
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "valid-linear native-unallocated shared dummy mask is hashed"
        );
        sim.substrate
            .base_reservations
            .reserve(sim.resolved_terrain.as_ref(), 1, 0, 1);
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .base_reservation_spacing = Some(9);
        assert_ne!(
            sim.state_hash(),
            expected_hash,
            "entity writer profile is hashed"
        );
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .base_reservation_spacing = Some(-3);
        assert_eq!(sim.state_hash(), expected_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_reservation", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().version,
            super::SNAPSHOT_VERSION
        );
        sim.substrate
            .base_reservations
            .clear(sim.resolved_terrain.as_ref(), 2, 0, 1);
        let expected_loaded_hash = sim.state_hash();

        let mut restored = GameSnapshot::load(&bytes)
            .expect("reservation snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore transient caches without rebuilding reservations");
        assert_eq!(restored.substrate.base_reservations.raw_mask(None, 3, 4), 1);
        assert_eq!(restored.substrate.base_reservations.dummy_mask(), 0);
        let restored_house = restored.houses.get(&owner).expect("restored owner");
        assert_eq!(restored_house.base_reservation.bounds(), (3, 4, 5, 6));
        assert_eq!(
            restored_house.base_reservation.perimeter_cells(),
            &[u32::from(3u16) | (u32::from(4u16) << 16)]
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .unwrap()
                .base_reservation_spacing,
            Some(-3)
        );
        assert_eq!(restored.state_hash(), expected_loaded_hash);
    }

    #[test]
    fn gsi_04_05_v40_roundtrip_restores_drive_footprint_and_cell_occupation() {
        use crate::sim::components::{DriveLocomotionRuntime, DriveOccupationFootprint};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::occupancy::{CellOccupationGrid, VEHICLE_OCCUPATION_BIT};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("MTNK");
        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 2, 2);
        entity.owner = owner;
        entity.type_ref = type_ref;
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(entity_id);

        let footprint = DriveOccupationFootprint {
            rx: 3,
            ry: 2,
            layer: MovementLayer::Ground,
        };
        sim.substrate
            .entities
            .get_mut(entity_id)
            .expect("Drive unit")
            .drive_locomotion = Some(DriveLocomotionRuntime {
            occupation_head_to: Some(footprint),
            ..Default::default()
        });
        sim.substrate
            .entities
            .get_mut(entity_id)
            .unwrap()
            .foot_occupation_enabled = false;
        sim.substrate.cell_occupation = CellOccupationGrid::rebuild(&sim.substrate.entities);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // Drive footprint persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .version,
            super::SNAPSHOT_VERSION
        );

        let mut restored_a = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        let mut restored_b = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored_a
            .restore_after_snapshot_load()
            .expect("first deterministic rebuild");
        restored_b
            .restore_after_snapshot_load()
            .expect("second deterministic rebuild");

        for restored in [&restored_a, &restored_b] {
            let drive = restored
                .substrate
                .entities
                .get(entity_id)
                .expect("restored Drive unit")
                .drive_locomotion
                .as_ref()
                .expect("restored Drive runtime");
            assert_eq!(drive.occupation_head_to, Some(footprint));
            assert!(
                !restored
                    .substrate
                    .entities
                    .get(entity_id)
                    .unwrap()
                    .foot_occupation_enabled
            );
            assert_eq!(restored.state_hash(), expected_hash);
            assert_eq!(
                restored
                    .substrate
                    .cell_occupation
                    .vehicle_bits(2, 2, MovementLayer::Ground),
                0
            );
            assert_eq!(
                restored.substrate.cell_occupation.vehicle_bits(
                    footprint.rx,
                    footprint.ry,
                    footprint.layer
                ),
                VEHICLE_OCCUPATION_BIT
            );
        }
    }

    #[test]
    fn gsi_04_07_v40_roundtrip_restores_wall_owner() {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        overlays.place_owned_wall(3, 4, 2, 0x1A, owner);
        sim.overlay_grid = Some(overlays);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // wall-owner persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();
        let mut unowned = Simulation::new();
        unowned.scenario_rng = crate::sim::rng::SimRng::new(0);
        let _ = unowned.interner.intern("AMERICANS");
        let mut unowned_overlays = crate::sim::overlay_grid::OverlayGrid::new(8, 8);
        unowned_overlays.place_overlay(3, 4, 2, 0x1A);
        unowned.overlay_grid = Some(unowned_overlays);
        assert_ne!(
            unowned.state_hash(),
            expected_hash,
            "wall ownership participates in deterministic state"
        );

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_07", 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes)
                .expect("current snapshot header")
                .version,
            super::SNAPSHOT_VERSION
        );
        let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        let cell = restored
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(3, 4);
        assert_eq!(cell.overlay_id, Some(2));
        assert_eq!(cell.overlay_data, 0x1A);
        assert_eq!(cell.wall_owner, Some(owner));
        assert_eq!(restored.state_hash(), expected_hash);
    }

    #[test]
    fn validated_load_requires_exact_content_and_session_metadata() {
        use crate::sim::command::{Command, CommandEnvelope};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("AMERICANS");
        sim.session.map_name = "MAP01.MAP".to_string();
        sim.session.tick = 0x1_0000_0007;
        sim.session.binary_frame = u32::MAX - 2;
        sim.session.total_sim_ms = 12_345;
        sim.session.house_order.push(owner);
        sim.queue_command(CommandEnvelope::new(
            owner,
            sim.session.tick + 3,
            Command::Stop { entity_id: 71 },
        ));
        sim.scatter_rng().next_u32();
        sim.weapon_spread_rng().next_u32();
        sim.mapgen_rng.next_u32();
        let process_default = crate::sim::rng::SimRng::new(0).logical_state();
        assert_ne!(sim.rng_state().scenario, process_default);

        let bytes = GameSnapshot::save_validated(&sim, 0x11, 0x22, "Campaign foothold", 0x33);
        let restored =
            GameSnapshot::load_validated(&bytes, 0x11, 0x22, "MAP01.MAP").expect("exact metadata");
        assert_eq!(restored.tick, sim.session.tick);
        assert_eq!(restored.map_name, sim.session.map_name);
        assert_eq!(restored.sim.session.binary_frame, u32::MAX - 2);
        assert_eq!(restored.sim.session.total_sim_ms, 12_345);
        assert_eq!(restored.sim.session.house_order, vec![owner]);
        assert_eq!(
            restored.sim.pending_commands_for_tests(),
            sim.pending_commands_for_tests()
        );
        assert_eq!(
            restored.sim.scenario_rng.logical_state(),
            process_default,
            "native post-read Scenario reinitialization must reset the saved cursor"
        );
        assert_eq!(restored.sim.main_rng.logical_state(), process_default);
        assert_eq!(restored.sim.mapgen_rng.logical_state(), process_default);

        assert!(matches!(
            GameSnapshot::load_validated(&bytes, 0x12, 0x22, "MAP01.MAP"),
            Err(SnapshotError::MapMismatch { .. })
        ));
        assert!(matches!(
            GameSnapshot::load_validated(&bytes, 0x11, 0x23, "MAP01.MAP"),
            Err(SnapshotError::RulesMismatch { .. })
        ));
        assert!(matches!(
            GameSnapshot::load_validated(&bytes, 0x11, 0x22, "MAP02.MAP"),
            Err(SnapshotError::MapNameMismatch { .. })
        ));

        let mut inconsistent = GameSnapshot::load_unchecked(&bytes).expect("current snapshot");
        inconsistent.tick = inconsistent.tick.wrapping_add(1);
        let inconsistent_bytes = bincode::serialize(&inconsistent).expect("corrupt tick metadata");
        assert!(matches!(
            GameSnapshot::load_validated(&inconsistent_bytes, 0x11, 0x22, "MAP01.MAP"),
            Err(SnapshotError::TickMetadataMismatch { .. })
        ));

        inconsistent.tick = inconsistent.sim.session.tick;
        inconsistent.sim.session.map_name = "OTHER.MAP".to_string();
        let inconsistent_bytes = bincode::serialize(&inconsistent).expect("corrupt map metadata");
        assert!(matches!(
            GameSnapshot::load_validated(&inconsistent_bytes, 0x11, 0x22, "MAP01.MAP"),
            Err(SnapshotError::MapNameMetadataMismatch { .. })
        ));
    }

    #[test]
    fn restore_validates_references_then_rebuilds_logic_particles_and_cells() {
        use crate::rules::particle_system_type::ParticleSystemTypeId;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::particles::ParticleSystem;
        use crate::util::fixed_math::SimFixed;
        use glam::IVec3;

        let mut sim = Simulation::new();
        sim.session.map_name = "RESTORE.MAP".to_string();
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("MTNK");

        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 5, 6);
        entity.owner = owner;
        entity.type_ref = type_ref;
        entity.pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
            start_frame: -1,
            duration_frames: 0,
            source_entity_id: Some(999),
        });
        sim.substrate.entities.insert(entity);
        sim.add_entity_occupancy(entity_id);

        let particle_id = sim.allocate_stable_id();
        sim.substrate.particle_systems.insert(ParticleSystem {
            stable_id: particle_id,
            in_logic_vector: false,
            type_id: ParticleSystemTypeId(0),
            coords: IVec3::ZERO,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(0),
            lifetime: -1,
            spark_spawn_frames: 0,
            facing: 0x1d,
            directionless: false,
            attached_entity: Some(entity_id),
            owner_entity: Some(entity_id),
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
        });
        sim.set_logic_order_for_test(vec![particle_id, entity_id]);

        let bytes = GameSnapshot::save_validated(&sim, 7, 8, "Restore fixture", 9);
        let mut restored = GameSnapshot::load_validated(&bytes, 7, 8, "RESTORE.MAP")
            .expect("strict snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("valid restored object graph");

        assert_eq!(
            restored.live_object_order_snapshot(),
            vec![particle_id, entity_id]
        );
        assert!(
            restored
                .substrate
                .particle_systems
                .get(particle_id)
                .expect("particle system")
                .in_logic_vector
        );
        assert!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .expect("entity")
                .in_logic_vector
        );
        assert!(
            restored
                .substrate
                .occupancy
                .contains_entity(5, 6, entity_id)
        );
        assert_eq!(
            restored
                .substrate
                .entities
                .get(entity_id)
                .expect("entity")
                .pending_c4_detonation
                .and_then(|pending| pending.source_entity_id),
            None,
            "a successful restore cleans a dangling weak reference"
        );
        let system = restored
            .substrate
            .particle_systems
            .get(particle_id)
            .expect("particle system");
        assert_eq!(system.owner_entity, Some(entity_id));
        assert_eq!(system.attached_entity, Some(entity_id));
    }

    #[test]
    fn restore_rejects_unresolved_native_pointer_before_weak_cleanup() {
        use crate::rules::particle_system_type::ParticleSystemTypeId;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::particles::ParticleSystem;
        use crate::util::fixed_math::SimFixed;
        use glam::IVec3;

        let mut sim = Simulation::new();
        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "MTNK", "AMERICANS", 5, 6);
        entity.pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
            start_frame: -1,
            duration_frames: 0,
            source_entity_id: Some(999),
        });
        sim.substrate.entities.insert(entity);

        let particle_id = sim.allocate_stable_id();
        sim.substrate.particle_systems.insert(ParticleSystem {
            stable_id: particle_id,
            in_logic_vector: false,
            type_id: ParticleSystemTypeId(0),
            coords: IVec3::ZERO,
            offset: IVec3::ZERO,
            particles: Vec::new(),
            spawn_timer: SimFixed::from_num(0),
            lifetime: -1,
            spark_spawn_frames: 0,
            facing: 0x1d,
            directionless: false,
            attached_entity: Some(999),
            owner_entity: Some(entity_id),
            target_coords: IVec3::ZERO,
            owner_house: None,
            done_spawning: false,
        });

        assert_eq!(
            sim.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::UnresolvedObjectReference {
                source_registry: "ParticleSystemStore",
                source_id: particle_id,
                field: "attached_entity",
                target_registry: "EntityStore",
                target_id: 999,
            })
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(entity_id)
                .expect("entity")
                .pending_c4_detonation
                .and_then(|pending| pending.source_entity_id),
            Some(999),
            "failed restoration must not clean a later weak reference"
        );
        assert_eq!(
            sim.substrate
                .particle_systems
                .get(particle_id)
                .expect("particle system")
                .attached_entity,
            Some(999),
            "failed restoration must not sanitize the unresolved strong reference"
        );
    }

    #[derive(Debug, Clone, Copy)]
    struct Gsi1703Ids {
        parent: u64,
        child: u64,
        target: u64,
        projectile: u64,
    }

    fn gsi_17_03_projectile(
        source_id: u64,
        target: crate::sim::projectile::ProjectileTarget,
    ) -> crate::sim::projectile::ProjectileSpawn {
        use crate::sim::intern::InternedId;
        use crate::sim::projectile::{
            ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
            ProjectileTrajectory, ProjectileVelocity, ProjectileVisualState, TargetExpiryPolicy,
        };

        ProjectileSpawn {
            flat: false,
            source_id,
            origin: ProjectileCoord::new(0, 0, 0),
            target,
            initial_target_position: ProjectileCoord::new(256, 256, 0),
            payload: ProjectilePayload {
                base_damage: 1,
                warhead: InternedId::from_index(0),
                weapon: InternedId::from_index(0),
            },
            speed_leptons_per_frame: 64,
            velocity: ProjectileVelocity::new(64, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        }
    }

    fn gsi_17_03_reference_fixture() -> (Simulation, Gsi1703Ids) {
        use crate::sim::combat::TargetKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::projectile::ProjectileTarget;
        use crate::sim::spawn_manager::{
            SpawnManagerMode, SpawnManagerState, SpawnSlot, SpawnSlotState, SpawnTimer,
        };

        let mut sim = Simulation::new();
        let parent = sim.allocate_stable_id();
        let child = sim.allocate_stable_id();
        let target = sim.allocate_stable_id();

        let mut parent_entity = GameEntity::test_default(parent, "CARRIER", "AMERICANS", 1, 1);
        parent_entity.pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
            start_frame: -1,
            duration_frames: 0,
            source_entity_id: Some(9_999),
        });
        parent_entity.spawn_manager = Some(SpawnManagerState {
            spawn_type: sim.interner.intern("HORNET"),
            missile_family: None,
            regen_rate: 45,
            reload_rate: 20,
            kamikaze_wait_frames: 0,
            slots: vec![
                SpawnSlot {
                    spawn: Some(child),
                    state: SpawnSlotState::ReadyDocked,
                    timer: SpawnTimer::ready(),
                    is_missile_spawn: false,
                },
                // A second saved pointer to the same child deliberately proves
                // that swizzle aliases are valid, not duplicate ownership.
                SpawnSlot {
                    spawn: Some(child),
                    state: SpawnSlotState::ReadyDocked,
                    timer: SpawnTimer::ready(),
                    is_missile_spawn: false,
                },
            ],
            update_timer: SpawnTimer::ready(),
            reload_timer: SpawnTimer::ready(),
            current_target: Some(TargetKind::Entity(target)),
            queued_target: Some(TargetKind::Entity(target)),
            mode: SpawnManagerMode::Launching,
        });
        sim.substrate.entities.insert(parent_entity);

        let mut child_entity = GameEntity::test_default(child, "HORNET", "AMERICANS", 2, 1);
        child_entity.spawn_owner_id = Some(parent);
        sim.substrate.entities.insert(child_entity);
        sim.substrate
            .entities
            .insert(GameEntity::test_default(target, "E1", "SOVIET", 3, 1));
        sim.register_live_object(parent);
        sim.register_live_object(child);
        sim.register_live_object(target);

        let projectile = sim.allocate_stable_id();
        sim.admit_projectile(
            projectile,
            gsi_17_03_projectile(target, ProjectileTarget::Entity(target)),
        );

        (
            sim,
            Gsi1703Ids {
                parent,
                child,
                target,
                projectile,
            },
        )
    }

    #[test]
    fn gsi_17_03_each_spawn_and_projectile_pointer_role_must_resolve_atomically() {
        #[derive(Debug, Clone, Copy)]
        enum MissingRole {
            SpawnSlot,
            CurrentTarget,
            QueuedTarget,
            SpawnOwner,
            ProjectileSource,
            ProjectileTarget,
        }

        let cases = [
            MissingRole::SpawnSlot,
            MissingRole::CurrentTarget,
            MissingRole::QueuedTarget,
            MissingRole::SpawnOwner,
            MissingRole::ProjectileSource,
            MissingRole::ProjectileTarget,
        ];
        for role in cases {
            let (mut sim, ids) = gsi_17_03_reference_fixture();
            let (source_registry, source_id, field) = match role {
                MissingRole::SpawnSlot => {
                    sim.substrate
                        .entities
                        .get_mut(ids.parent)
                        .unwrap()
                        .spawn_manager
                        .as_mut()
                        .unwrap()
                        .slots[0]
                        .spawn = Some(9_999);
                    ("EntityStore", ids.parent, "spawn_manager.slots.spawn")
                }
                MissingRole::CurrentTarget => {
                    sim.substrate
                        .entities
                        .get_mut(ids.parent)
                        .unwrap()
                        .spawn_manager
                        .as_mut()
                        .unwrap()
                        .current_target = Some(crate::sim::combat::TargetKind::Entity(9_999));
                    ("EntityStore", ids.parent, "spawn_manager.current_target")
                }
                MissingRole::QueuedTarget => {
                    sim.substrate
                        .entities
                        .get_mut(ids.parent)
                        .unwrap()
                        .spawn_manager
                        .as_mut()
                        .unwrap()
                        .queued_target = Some(crate::sim::combat::TargetKind::Entity(9_999));
                    ("EntityStore", ids.parent, "spawn_manager.queued_target")
                }
                MissingRole::SpawnOwner => {
                    sim.substrate
                        .entities
                        .get_mut(ids.child)
                        .unwrap()
                        .spawn_owner_id = Some(9_999);
                    ("EntityStore", ids.child, "spawn_owner_id")
                }
                MissingRole::ProjectileSource => {
                    sim.projectiles.get_mut(ids.projectile).unwrap().source_id = 9_999;
                    ("ProjectileStore", ids.projectile, "source_id")
                }
                MissingRole::ProjectileTarget => {
                    sim.projectiles.get_mut(ids.projectile).unwrap().target =
                        crate::sim::projectile::ProjectileTarget::Entity(9_999);
                    ("ProjectileStore", ids.projectile, "target")
                }
            };

            assert_eq!(
                sim.restore_after_snapshot_load(),
                Err(SnapshotRestoreError::UnresolvedObjectReference {
                    source_registry,
                    source_id,
                    field,
                    target_registry: "EntityStore",
                    target_id: 9_999,
                }),
                "missing {role:?} must reject the snapshot"
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(ids.parent)
                    .unwrap()
                    .pending_c4_detonation
                    .and_then(|pending| pending.source_entity_id),
                Some(9_999),
                "{role:?} rejection must precede later weak-reference cleanup"
            );
        }
    }

    #[test]
    fn gsi_17_03_null_and_cell_reference_forms_need_no_object_identity() {
        use crate::sim::combat::{RAD_NO_ATTACKER, TargetKind};
        use crate::sim::projectile::ProjectileTarget;

        let (mut sim, ids) = gsi_17_03_reference_fixture();
        let manager = sim
            .substrate
            .entities
            .get_mut(ids.parent)
            .unwrap()
            .spawn_manager
            .as_mut()
            .unwrap();
        for slot in &mut manager.slots {
            slot.spawn = None;
        }
        manager.current_target = Some(TargetKind::Cell(7, 8));
        manager.queued_target = Some(TargetKind::Cell(9, 10));
        sim.substrate
            .entities
            .get_mut(ids.child)
            .unwrap()
            .spawn_owner_id = None;
        let projectile = sim.projectiles.get_mut(ids.projectile).unwrap();
        projectile.source_id = RAD_NO_ATTACKER;
        projectile.target = ProjectileTarget::Cell { rx: 11, ry: 12 };

        let null_target_projectile = sim.allocate_stable_id();
        sim.admit_projectile(
            null_target_projectile,
            gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::None),
        );

        assert_eq!(sim.restore_after_snapshot_load(), Ok(()));
    }

    #[test]
    fn gsi_04_01_snapshot_handoff_retains_live_process_dummy_without_serializing_it() {
        use crate::sim::combat::RAD_NO_ATTACKER;
        use crate::sim::projectile::ProjectileTarget;
        use crate::sim::{movement::locomotor::MovementLayer, occupancy::RawCellKey};

        let mut live = Simulation::new();
        let process_dummy = live.shared_cell_dummy.clone();
        process_dummy.set_level_slope(-7, 11);
        process_dummy.stamp_coord(7, 9);

        let before_neighbor_count = live.state_hash();
        process_dummy.adjust_neighbor_count(true);
        assert_ne!(before_neighbor_count, live.state_hash());

        let owner = live.intern("DummyOccupationOwner");
        let hash_before_raw = live.state_hash();
        live.substrate.raw_cell_occupation.write_infantry(
            RawCellKey::Dummy,
            MovementLayer::Ground,
            4,
            owner,
            true,
        );
        let ground_hash = live.state_hash();
        assert_ne!(hash_before_raw, ground_hash);
        live.substrate.raw_cell_occupation.write_infantry(
            RawCellKey::Dummy,
            MovementLayer::Bridge,
            8,
            owner,
            true,
        );
        assert_ne!(
            ground_hash,
            live.state_hash(),
            "the fallback cell's two raw planes are future-affecting"
        );
        let projectile_id = live.allocate_stable_id();
        live.admit_projectile(
            projectile_id,
            gsi_17_03_projectile(RAD_NO_ATTACKER, ProjectileTarget::DummyCell),
        );

        let hash_at_a = live.state_hash();
        process_dummy.stamp_coord(8, 10);
        assert_ne!(
            hash_at_a,
            live.state_hash(),
            "a retained Bullet pointer makes the live dummy snapshot future behavior"
        );
        process_dummy.stamp_coord(7, 9);

        let bytes = GameSnapshot::save(&live, 0, 0, "shared-dummy.map", 0);
        let cold = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert!(
            cold.substrate
                .raw_cell_occupation
                .dummy_for_hash()
                .is_none(),
            "process-global occupation is not saved Scenario payload"
        );
        assert_eq!(
            cold.projectiles.get(projectile_id).unwrap().target,
            ProjectileTarget::DummyCell
        );
        assert_eq!(
            cold.shared_cell_dummy.snapshot().coord,
            (0, 0),
            "the process-global CellClass bytes are not Scenario payload"
        );
        assert!(!cold.shared_cell_dummy.same_identity(&process_dummy));
        assert_eq!(cold.shared_cell_dummy.neighbor_count(), 0);

        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored.retain_in_scenario_process_state_from(&live);
        assert_eq!(
            restored.substrate.raw_cell_occupation.dummy_for_hash(),
            live.substrate.raw_cell_occupation.dummy_for_hash()
        );
        assert!(restored.shared_cell_dummy.same_identity(&process_dummy));
        assert_eq!(restored.shared_cell_dummy.neighbor_count(), 1);
        assert_eq!(
            restored.shared_cell_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (7, 9),
                level: -7,
                slope_type: 11,
                bridge_flags_0x1180: 0,
            },
            "fallible candidate preparation retains identity without mutating the live process"
        );
        restored
            .restore_after_snapshot_load()
            .expect("dummy CellClass target needs no Object identity fixup");

        let terrain_template = flat_terrain(2, 2);
        restored.rebuild_caches_after_load(
            terrain_template,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            Vec::new(),
            Vec::new(),
        );
        let rebuilt_dummy = restored
            .resolved_terrain
            .as_ref()
            .expect("rebuilt terrain")
            .shared_cell_dummy();
        assert!(rebuilt_dummy.same_identity(&process_dummy));
        assert_eq!(rebuilt_dummy.snapshot().coord, (7, 9));

        restored.reconstruct_cellclass_dummy_for_map_resize();
        assert!(
            restored
                .substrate
                .raw_cell_occupation
                .dummy_for_hash()
                .is_none(),
            "Cell47BBF0 reconstructs both raw bytes and owners"
        );
        assert_eq!(
            rebuilt_dummy.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );

        process_dummy.stamp_coord(-2, 7);
        assert_eq!(
            rebuilt_dummy.snapshot().coord,
            (-2, 7),
            "the restored DummyCell target retains the same live identity after reconstruction"
        );
    }

    #[test]
    fn gsi_04_01_successful_load_restores_real_flags_and_zeroes_only_dummy() {
        use crate::map::bridge_facts::{
            BRIDGE_FLAG_STRUCTURAL, BridgeFlagStamp, BridgeStampSlot,
            MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::map::overlay_types::OverlayTypeRegistry;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::overlay_grid::OverlayGrid;

        let mut live = Simulation::new();
        let mut map_load_terrain = flat_terrain(4, 3);
        map_load_terrain.test_set_native_allocated_cells(&[(0, 0), (1, 1)]);
        {
            let pristine_bridge_cell = map_load_terrain.cell_mut(1, 1).unwrap();
            pristine_bridge_cell.level = 2;
            pristine_bridge_cell.slope_type = 1;
            pristine_bridge_cell.bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        }
        let pristine_load_template = map_load_terrain.clone();
        let expected_ground_z = {
            let target =
                crate::sim::projectile::cell_target_coord(Some(&pristine_load_template), 1, 1);
            let cell = pristine_load_template.cell(1, 1).unwrap();
            crate::util::lepton::ground_height_leptons(
                cell.level,
                cell.slope_type,
                target.x,
                target.y,
            )
            .expect("fixture uses a native-supported CellClass slope")
        };
        live.install_resolved_terrain_for_new_map(map_load_terrain);
        live.overlay_grid = Some(OverlayGrid::new_with_retained_wall_plane(4, 3));
        assert_ne!(
            live.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & BRIDGE_FLAG_STRUCTURAL,
            0,
            "the pristine allocated CellClass starts on the high bridge target surface"
        );
        let hash_with_pristine_bridge = live.state_hash();

        // The real anchor is allocated. Every neighbor is unallocated or
        // missing, so this live clear updates both serialized real authority
        // and the process dummy through their distinct native targets before
        // the snapshot is written.
        live.apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new((1, 1), 0, false));
        let process_dummy = live.effective_shared_cell_dummy();
        assert_eq!(
            live.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "the live runtime setter clears the allocated CellClass before save"
        );
        assert_eq!(
            live.real_cell_bridge_flags_0x1180,
            live.resolved_terrain
                .as_ref()
                .unwrap()
                .capture_real_cell_bridge_flags_0x1180(),
            "serialized real-cell authority records the runtime-cleared value"
        );
        process_dummy.reconstruct_for_map_resize();
        let hash_after_runtime_clear = live.state_hash();
        assert_ne!(
            hash_after_runtime_clear, hash_with_pristine_bridge,
            "the future-affecting real-cell value authority is hashed even with a clear dummy"
        );
        process_dummy.stamp_coord(-23, 17);
        process_dummy.set_level_slope(-7, 11);
        process_dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        let dirty_dummy_before_load = process_dummy.snapshot();
        assert_ne!(dirty_dummy_before_load.bridge_flags_0x1180, 0);

        let bytes = GameSnapshot::save(&live, 0, 0, "bridge-dummy.map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        restored.retain_in_scenario_process_state_from(&live);
        restored
            .restore_after_snapshot_load()
            .expect("snapshot references restore");
        restored.rebuild_caches_after_load(
            pristine_load_template,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            Vec::new(),
            Vec::new(),
        );
        let rebuilt_terrain = restored.resolved_terrain.as_ref().unwrap();
        assert_eq!(
            rebuilt_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            "candidate cache rebuild starts from the nonzero pristine map template"
        );
        assert_eq!(
            crate::sim::projectile::cell_target_coord(Some(rebuilt_terrain), 1, 1).z,
            expected_ground_z + crate::util::lepton::BRIDGE_HEIGHT_DELTA_LEPTONS as i32,
            "before direct value restore the pristine raw 0x100 selects the +416 target surface"
        );
        let pristine_candidate_authority = rebuilt_terrain.capture_real_cell_bridge_flags_0x1180();
        let serialized_cleared_authority = restored.real_cell_bridge_flags_0x1180.clone();
        assert_ne!(serialized_cleared_authority, pristine_candidate_authority);
        let hash_with_serialized_clear = restored.state_hash();
        restored.real_cell_bridge_flags_0x1180 = pristine_candidate_authority.clone();
        let hash_if_pristine_template_were_authority = restored.state_hash();
        assert_ne!(
            hash_with_serialized_clear, hash_if_pristine_template_were_authority,
            "the canonical hash distinguishes the saved clear from the pristine nonzero template"
        );
        restored.real_cell_bridge_flags_0x1180 = serialized_cleared_authority.clone();
        assert_eq!(restored.state_hash(), hash_with_serialized_clear);
        let candidate_dummy_before_authority_restore =
            restored.effective_shared_cell_dummy().snapshot();
        let ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[OverlayTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("empty map-authority rules");
        restored
            .restore_map_authority_after_snapshot_load(&rules, &OverlayTypeRegistry::empty())
            .expect("serialized real CellClass values restore directly");

        let candidate_dummy = restored.effective_shared_cell_dummy();
        assert!(candidate_dummy.same_identity(&process_dummy));
        assert_eq!(
            candidate_dummy.snapshot(),
            candidate_dummy_before_authority_restore,
            "direct real-cell restoration must not lookup, stamp, or mutate the dummy"
        );
        assert_eq!(candidate_dummy.snapshot(), dirty_dummy_before_load);
        let restored_terrain = restored.resolved_terrain.as_ref().unwrap();
        assert_eq!(
            restored_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags & BRIDGE_FLAG_STRUCTURAL,
            0,
            "the saved clear removes the pristine real CellClass structural bit"
        );
        assert_eq!(
            restored_terrain.cell(1, 1).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "direct CellClass value restore replaces the complete pristine modeled mask"
        );
        assert_eq!(
            crate::sim::projectile::cell_target_coord(Some(restored_terrain), 1, 1).z,
            expected_ground_z,
            "restored raw 0x100 clear removes +416 while retaining the 104-lepton ground kernel"
        );
        assert_eq!(
            restored.state_hash(),
            hash_with_serialized_clear,
            "direct CellClass cache restoration leaves the saved-cleared authority hash unchanged"
        );
        assert_ne!(
            restored.state_hash(),
            hash_if_pristine_template_were_authority,
            "the restored state hash must not adopt the pristine nonzero template authority"
        );
        assert_eq!(
            restored_terrain.cell(0, 0).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "untouched allocated cells retain their exact saved zero"
        );
        assert!(restored_terrain.cell(1, 0).is_none());
        assert_eq!(
            restored_terrain.cells[1].bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "unallocated storage is not promoted into real-cell authority"
        );

        // MouseClass::Load reaches Resize only at accepted commit. That ctor
        // zeros the same dummy identity and does not rewrite loaded real cells.
        restored.reconstruct_cellclass_dummy_for_map_resize();

        let reconstructed = restored.effective_shared_cell_dummy();
        assert!(reconstructed.same_identity(&process_dummy));
        assert_eq!(
            reconstructed.snapshot(),
            crate::map::resolved_terrain::SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );
        assert_eq!(
            restored
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "dummy reconstruction must not resurrect pristine real CellClass flags"
        );
        assert_eq!(
            crate::sim::projectile::cell_target_coord(restored.resolved_terrain.as_ref(), 1, 1,).z,
            expected_ground_z,
            "accepted Resize keeps the restored real CellClass on the 104-lepton ground surface"
        );
        assert_eq!(
            restored.real_cell_bridge_flags_0x1180, serialized_cleared_authority,
            "accepted Resize clears only the dummy and retains saved real-cell hash authority"
        );
    }

    #[test]
    fn gsi_17_03_repeated_aliases_do_not_imply_reciprocal_spawn_ownership() {
        let (mut sim, ids) = gsi_17_03_reference_fixture();
        sim.substrate
            .entities
            .get_mut(ids.child)
            .unwrap()
            .spawn_owner_id = Some(ids.target);

        assert_eq!(sim.restore_after_snapshot_load(), Ok(()));
        assert_eq!(
            sim.substrate
                .entities
                .get(ids.parent)
                .unwrap()
                .spawn_manager
                .as_ref()
                .unwrap()
                .slots
                .iter()
                .map(|slot| slot.spawn)
                .collect::<Vec<_>>(),
            vec![Some(ids.child), Some(ids.child)]
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(ids.child)
                .unwrap()
                .spawn_owner_id,
            Some(ids.target)
        );
    }

    #[test]
    fn restore_recreates_active_move_sound_without_rng_or_countdown_mutation() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::SimSoundEvent;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n\
             [VehicleTypes]\n0=TEST\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TEST]\nMoveSound=VMove\n",
        ))
        .expect("move-sound rules");
        let mut sim = Simulation::new();
        sim.session.map_name = "MOVESOUND.MAP".to_string();
        let owner = sim.interner.intern("AMERICANS");
        let type_ref = sim.interner.intern("TEST");
        let sound_id = sim.interner.intern("VMove");
        let entity_id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(entity_id, "TEST", "AMERICANS", 7, 9);
        entity.owner = owner;
        entity.type_ref = type_ref;
        entity.move_sound_active = true;
        entity.move_sound_countdown = 2;
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save_validated(&sim, 17, 18, "Move sound fixture", 19);
        let mut restored = GameSnapshot::load_validated(&bytes, 17, 18, "MOVESOUND.MAP")
            .expect("strict snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("valid restored object graph");
        restored.resolve_type_handles(&rules);
        let rng_before = restored.rng_state();
        restored
            .restore_move_sound_handles_after_load(&rules)
            .expect("active move sound resolves");

        assert_eq!(restored.rng_state(), rng_before);
        let entity = restored.substrate.entities.get(entity_id).expect("entity");
        assert!(entity.move_sound_active);
        assert_eq!(entity.move_sound_countdown, 2);
        assert!(matches!(
            restored.sound_events.as_slice(),
            [SimSoundEvent::AnimationStarted {
                anim_id,
                sound_id: restored_sound,
                ..
            }] if *anim_id == entity_id && *restored_sound == sound_id
        ));
    }

    #[test]
    fn exact_mission_schema_round_trips_raw_ids_leaves_archives_and_locomotors() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::combat::TargetKind;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::mission::state::MissionTestFixture;
        use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionLeafState};
        use crate::sim::movement::locomotor::LocomotorState;

        let leaves = [
            MissionLeafState::unit_raw_for_test(1, 2, 3, 4),
            MissionLeafState::infantry_raw_for_test(5, 41),
            MissionLeafState::aircraft_raw_for_test(6, 7, true),
            MissionLeafState::building_raw_for_test(8),
            MissionLeafState::unit_raw_for_test(9, 10, 11, 12),
            MissionLeafState::infantry_raw_for_test(13, -1),
        ];

        let mut sim = Simulation::new();
        for index in 0..6 {
            let id = index as u64 + 1;
            let mut entity = GameEntity::test_default(id, "MTNK", "Americans", 5, 5);
            entity.mission_leaf = leaves[index];
            entity.suspended_attack_target = Some(if index & 1 == 0 {
                TargetKind::Entity(100 + id)
            } else {
                TargetKind::Cell(index as u16, (index + 1) as u16)
            });
            entity.set_object_is_falling_down_for_test(index as u8 + 1);
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
            if index == 0 {
                entity.mission.apply_test_fixture(MissionTestFixture {
                    current: MissionId::from_raw(i32::MIN),
                    suspended: MissionId::from_raw(0x1234_5678),
                    queued: MissionId::from_raw(i32::MAX),
                    movement_bypass_latch: 0xa5,
                    handler_state: 0x1122_3344,
                    mission_start_frame: 0x5566_7788,
                    ai_counter: 0x99aa_bbcc,
                    dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
                });
            }
            sim.substrate.entities.insert(entity);
        }

        let bytes = GameSnapshot::save(&sim, 1, 2, "mission-schema", 3);
        let loaded = GameSnapshot::load(&bytes).expect("load exact Mission schema");

        for index in 0..6 {
            let entity = loaded
                .sim
                .substrate
                .entities
                .get(index as u64 + 1)
                .expect("restored Mission fixture");
            let id = index as u64 + 1;
            let expected_suspended_target = Some(if index & 1 == 0 {
                TargetKind::Entity(100 + id)
            } else {
                TargetKind::Cell(index as u16, (index + 1) as u16)
            });
            assert_eq!(entity.mission_leaf, leaves[index]);
            assert_eq!(
                entity.suspended_attack_target, expected_suspended_target,
                "suspended TargetKind variant and payload must round-trip"
            );
            assert_eq!(entity.object_is_falling_down, index as u8 + 1);
        }

        let first = loaded.sim.substrate.entities.get(1).unwrap();
        assert_eq!(first.mission.current(), MissionId::from_raw(i32::MIN));
        assert_eq!(first.mission.suspended(), MissionId::from_raw(0x1234_5678));
        assert_eq!(first.mission.queued(), MissionId::from_raw(i32::MAX));
        assert_eq!(
            first.mission.dispatch_timer(),
            MissionDispatchTimer::from_raw(-17, -29)
        );
    }

    #[test]
    fn entity_construction_frame_round_trips_as_dispatch_start() {
        use crate::map::entities::EntityCategory;
        use crate::sim::components::Health;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern("MTNK");
        let entity = GameEntity::new_at_frame_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            true,
            37,
        );
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save(&sim, 0, 0, "frame-37", 0);
        let loaded = GameSnapshot::load(&bytes).expect("load frame-37 entity");
        assert_eq!(
            loaded
                .sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .mission
                .dispatch_timer()
                .start_frame(),
            37
        );
    }

    #[test]
    fn house_difficulty_round_trips_through_snapshot() {
        use crate::sim::house_state::{HouseDifficulty, HouseState};

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.difficulty = HouseDifficulty::Easy;
        sim.houses.insert(owner, house);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // house-difficulty persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let expected_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let loaded = GameSnapshot::load(&bytes).expect("load should succeed");

        assert_eq!(
            loaded.sim.houses.get(&owner).map(|house| house.difficulty),
            Some(HouseDifficulty::Easy),
        );
        assert_eq!(loaded.sim.state_hash(), expected_hash);
    }

    /// `AttackTarget::for_cell` survives serialize → deserialize as the same
    /// `TargetKind::Cell` variant (regression for SNAPSHOT_VERSION 4 → 5).
    #[test]
    fn cell_attack_target_round_trips_through_snapshot() {
        use crate::sim::combat::{AttackTarget, TargetKind};
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.attack_target = Some(AttackTarget::for_cell(50, 50));
        sim.substrate.entities.insert(entity);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let loaded = GameSnapshot::load(&bytes).expect("load should succeed");
        let restored = loaded
            .sim
            .substrate
            .entities
            .get(1)
            .expect("entity should be restored")
            .attack_target
            .as_ref()
            .expect("attack_target should be restored");
        assert!(matches!(restored.target, TargetKind::Cell(50, 50)));
    }

    /// Reveal registers at the tail; a stored-but-unrevealed (limbo) object is
    /// absent from the active order until revealed. (DRIFT 2 / ledger 9)
    #[test]
    fn limbo_object_registers_only_on_reveal() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        // Stored but not revealed: present in the store, absent from the order.
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        assert!(sim.substrate.entities.contains(1));
        assert!(!sim.live_object_order_snapshot().contains(&1));
        // Reveal both: tail-append in reveal order, not sorted.
        sim.substrate
            .entities
            .insert(GameEntity::test_default(2, "MTNK", "Americans", 6, 6));
        sim.register_live_object(2);
        sim.register_live_object(1);
        assert_eq!(sim.live_object_order_snapshot(), vec![2, 1]);
    }

    /// The active order is serialized directly and restored verbatim — not
    /// re-derived, not sorted. (ledger 13)
    #[test]
    fn saveload_restores_live_object_order_verbatim() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        for id in [10u64, 20, 30] {
            sim.substrate
                .entities
                .insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
            sim.register_live_object(id);
        }
        // Force an order whose sequence differs from stable-id order.
        sim.set_logic_order_for_test(vec![20, 10, 30]);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        assert_eq!(restored.live_object_order_snapshot(), vec![20, 10, 30]);
    }

    /// After load, membership is rebuilt from the order; a restored member
    /// unregisters exactly once (no stale entry) and re-registers without
    /// duplicating (no double-add). Avoids the §3.4 hazard. (ledger 14)
    #[test]
    fn saveload_restored_member_removes_cleanly() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        sim.register_live_object(1);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        // Real load-path step: membership flags are false straight after deserialize.
        restored.rebuild_logic_membership();

        // Unregister removes exactly once — no stale entry left behind.
        restored.unregister_live_object(1);
        assert!(!restored.live_object_order_snapshot().contains(&1));
        // Re-register appends once — no double-add.
        restored.register_live_object(1);
        assert_eq!(
            restored
                .live_object_order_snapshot()
                .iter()
                .filter(|&&x| x == 1)
                .count(),
            1
        );
    }

    /// Rust snapshots preserve a dead-limbo object's independent state and the
    /// ordered pending-delete boundary instead of reconstructing either from
    /// global storage or LogicVector membership.
    #[test]
    fn lifecycle_authority_pending_boundary_roundtrips_queue_and_state() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(7, "MTNK", "Americans", 5, 5);
        entity.lifecycle.object_alive = false;
        entity.lifecycle.in_limbo = true;
        entity.lifecycle.cell_marked = false;
        entity.in_logic_vector = false;
        entity.dying = true;
        entity.dirty_rect_eligible = true;
        entity.destruction_recorded = true;
        sim.substrate.entities.insert(entity);
        sim.substrate.pending_delete.extend([7, 3, 7]);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // lifecycle-boundary persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash_before = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        let restored_entity = restored.substrate.entities.get(7).expect("entity restored");

        assert!(!restored_entity.lifecycle.object_alive);
        assert!(restored_entity.lifecycle.in_limbo);
        assert!(!restored_entity.lifecycle.cell_marked);
        assert!(!restored_entity.in_logic_vector);
        assert!(restored_entity.dying);
        assert!(restored_entity.dirty_rect_eligible);
        assert!(restored_entity.destruction_recorded);
        assert_eq!(restored.substrate.pending_delete, vec![7, 3, 7]);
        assert_eq!(restored.state_hash(), hash_before);
    }

    #[test]
    fn lifecycle_authority_logic_rebuild_does_not_rederive_limbo_or_mark() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let mut off_cell_member = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        off_cell_member.lifecycle.in_limbo = false;
        off_cell_member.lifecycle.cell_marked = false;
        sim.substrate.entities.insert(off_cell_member);
        sim.substrate
            .logic
            .try_push(1)
            .expect("logic fixture append");

        let mut marked_non_member = GameEntity::test_default(2, "MTNK", "Americans", 6, 6);
        marked_non_member.lifecycle.in_limbo = false;
        marked_non_member.lifecycle.cell_marked = true;
        sim.substrate.entities.insert(marked_non_member);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        restored.rebuild_logic_membership();

        let member = restored.substrate.entities.get(1).expect("member restored");
        assert!(member.in_logic_vector);
        assert!(!member.lifecycle.in_limbo);
        assert!(!member.lifecycle.cell_marked);

        let non_member = restored
            .substrate
            .entities
            .get(2)
            .expect("non-member restored");
        assert!(!non_member.in_logic_vector);
        assert!(!non_member.lifecycle.in_limbo);
        assert!(non_member.lifecycle.cell_marked);
    }

    #[test]
    fn lifecycle_authority_bookkeeping_facts_roundtrip_and_change_state_hash() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // lifecycle bookkeeping persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let default_hash = sim.state_hash();

        let entity = sim.substrate.entities.get_mut(1).expect("fixture entity");
        entity.dirty_rect_eligible = true;
        entity.destruction_recorded = true;
        let changed_hash = sim.state_hash();
        assert_ne!(default_hash, changed_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        let restored_entity = restored.substrate.entities.get(1).expect("entity restored");
        assert!(restored_entity.dirty_rect_eligible);
        assert!(restored_entity.destruction_recorded);
        assert_eq!(restored.state_hash(), changed_hash);
    }

    #[test]
    fn techno_playfield_membership_roundtrips_and_changes_state_hash_v87() {
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let default_hash = sim.state_hash();
        sim.substrate.entities.get_mut(1).unwrap().in_playfield = true;
        let member_hash = sim.state_hash();
        assert_ne!(default_hash, member_hash);

        let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
        let restored = GameSnapshot::load(&bytes)
            .expect("v87 membership loads")
            .sim;
        assert!(restored.substrate.entities.get(1).unwrap().in_playfield);
        assert_eq!(restored.state_hash(), member_hash);
    }

    #[test]
    fn saveload_occupancy_list_order_matches_incremental() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");

        let mut structure = GameEntity::test_default(100, "GAPOWR", "Americans", 5, 5);
        structure.owner = owner;
        structure.category = EntityCategory::Structure;
        sim.substrate.entities.insert(structure);
        sim.add_entity_occupancy(100);

        let mut older_mobile = GameEntity::test_default(50, "MTNK", "Americans", 5, 5);
        older_mobile.owner = owner;
        older_mobile.category = EntityCategory::Unit;
        sim.substrate.entities.insert(older_mobile);
        sim.add_entity_occupancy(50);

        let mut newer_mobile = GameEntity::test_default(10, "HTNK", "Americans", 5, 5);
        newer_mobile.owner = owner;
        newer_mobile.category = EntityCategory::Unit;
        sim.substrate.entities.insert(newer_mobile);
        sim.add_entity_occupancy(10);

        let incremental = cell_order(&sim, 5, 5, MovementLayer::Ground);
        assert_eq!(incremental, vec![10, 50, 100]);
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // occupancy-order persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash_at_save = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "order_test", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
        rebuild_load_caches(&mut restored, flat_terrain(8, 8));

        assert_eq!(
            cell_order(&restored, 5, 5, MovementLayer::Ground),
            incremental,
            "rebuilt occupancy cache must match the incremental CellClass list order"
        );
        assert_eq!(
            restored.state_hash(),
            hash_at_save,
            "cache rebuild must not change authoritative save state"
        );
    }

    #[test]
    fn gsi_04_10_destroyed_terrain_snapshot_rebuild_does_not_resurrect_cache() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::pathfinding::zone_map::ZONE_INVALID;
        use crate::sim::terrain_object::{
            TerrainDamageResult, TerrainObjectLifecycle, TerrainObjectState,
            damage_terrain_object_at_cell, mark_terrain_occupation, mark_terrain_raw_occupation,
        };
        use crate::sim::terrain_spawn::TerrainSpawnerState;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nTreeStrength=10\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n0=DUMMY\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [TerrainTypes]\n0=TREE01\n1=TIBTRE01\n\
             [DUMMY]\nPrimary=Gun\nStrength=100\nArmor=heavy\n\
             [Gun]\nDamage=10\nWarhead=WH\n\
             [WH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\
             [TREE01]\nStrength=10\nArmor=wood\nImmune=no\nTemperateOccupationBits=7\n\
             [TIBTRE01]\nStrength=10\nArmor=wood\nImmune=yes\nSpawnsTiberium=yes\nTemperateOccupationBits=7\n",
        ))
        .expect("terrain damage rules");
        let mut sim = Simulation::new();
        let damaged_cell = (0, 0);
        let destroyed_cell = (1, 1);
        let spawner_cell = (2, 2);
        let damaged_id = 1;
        let destroyed_id = 2;
        let spawner_id = 3;
        let tree_type = sim.interner.intern("TREE01");
        let spawner_type = sim.interner.intern("TIBTRE01");
        let damaged = TerrainObjectState {
            stable_id: damaged_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: tree_type,
            rx: damaged_cell.0,
            ry: damaged_cell.1,
            health: 6,
            max_health: 10,
            occupation_bits: 7,
            lifecycle: TerrainObjectLifecycle::Live,
        };
        let destroyed = TerrainObjectState {
            stable_id: destroyed_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: sim.interner.intern("TREE01"),
            rx: destroyed_cell.0,
            ry: destroyed_cell.1,
            health: 10,
            max_health: 10,
            occupation_bits: 7,
            lifecycle: TerrainObjectLifecycle::Live,
        };
        let spawner = TerrainObjectState {
            stable_id: spawner_id,
            native_unique_id: None,
            in_logic_vector: false,
            type_ref: spawner_type,
            rx: spawner_cell.0,
            ry: spawner_cell.1,
            health: 10,
            max_health: 10,
            occupation_bits: 7,
            lifecycle: TerrainObjectLifecycle::Live,
        };
        for terrain in [&damaged, &destroyed, &spawner] {
            sim.production
                .terrain_objects
                .insert(terrain.stable_id, terrain.clone());
            assert!(sim.register_terrain_object(terrain.stable_id, None));
            sim.production
                .terrain_object_cells
                .insert(terrain.cell(), terrain.stable_id);
            mark_terrain_raw_occupation(
                &mut sim.substrate.raw_cell_occupation,
                terrain.cell(),
                terrain.occupation_bits,
            );
        }
        sim.substrate.next_stable_object_id = spawner_id + 1;
        sim.production.terrain_spawners.insert(
            spawner_cell,
            TerrainSpawnerState::new(spawner_type, 3_000, 3, 22),
        );
        sim.production
            .tiberium_spawning_terrain_cells
            .insert(spawner_cell);

        let mut original_occupied_grid = flat_terrain(3, 3);
        for terrain in [&damaged, &destroyed, &spawner] {
            mark_terrain_occupation(
                &mut sim.production,
                terrain,
                Some(&mut original_occupied_grid),
            );
        }
        let stale_original_grid = original_occupied_grid.clone();
        assert!(
            !PathGrid::from_resolved_terrain(&stale_original_grid)
                .is_walkable(destroyed_cell.0, destroyed_cell.1)
        );
        assert_eq!(
            all_terrain_costs(&stale_original_grid)[&SpeedType::Track]
                .cost_at(destroyed_cell.0, destroyed_cell.1),
            0,
            "the caller fixture must carry the original blocked cost cache"
        );
        sim.terrain_costs = all_terrain_costs(&original_occupied_grid);
        sim.resolved_terrain = Some(original_occupied_grid);

        let result = damage_terrain_object_at_cell(
            &mut sim.production,
            &mut sim.substrate.raw_cell_occupation,
            &rules,
            &sim.interner,
            destroyed_cell,
            10,
            rules.warhead("WH").expect("wood warhead"),
            sim.resolved_terrain.as_mut(),
        );
        assert_eq!(result, TerrainDamageResult::Destroyed);
        assert!(sim.unregister_non_entity_object(destroyed_id));
        sim.substrate.pending_delete.push(destroyed_id);
        sim.terrain_costs =
            crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(
                sim.resolved_terrain
                    .as_ref()
                    .expect("post-destruction terrain"),
            );
        // Native in-scenario load restarts Scenario RNG from Seed0; isolate
        // destroyed-terrain persistence on that same post-load cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let authoritative_hash = sim.state_hash();

        let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_10_destroyed_terrain", 0);
        let mut restored = GameSnapshot::load(&bytes)
            .expect("destroyed Terrain snapshot")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("restore serialized authority");
        restored.rebuild_caches_after_load(
            stale_original_grid,
            crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(
            restored.production.terrain_objects[&destroyed_id].lifecycle,
            TerrainObjectLifecycle::Destroyed
        );
        assert!(
            !restored
                .production
                .terrain_object_cells
                .contains_key(&destroyed_cell)
        );
        let restored_damaged = &restored.production.terrain_objects[&damaged_id];
        assert_eq!(restored_damaged.health, 6);
        assert_eq!(restored_damaged.lifecycle, TerrainObjectLifecycle::Live);
        assert_eq!(restored_damaged.cell(), damaged_cell);
        assert_eq!(
            restored.production.terrain_object_cells[&damaged_cell],
            damaged_id
        );
        let restored_spawner = &restored.production.terrain_objects[&spawner_id];
        assert_eq!(restored_spawner.health, 10);
        assert_eq!(restored_spawner.lifecycle, TerrainObjectLifecycle::Live);
        assert_eq!(restored_spawner.cell(), spawner_cell);
        assert_eq!(
            restored.production.terrain_object_cells[&spawner_cell],
            spawner_id
        );
        assert_eq!(
            restored.production.terrain_spawners[&spawner_cell].type_ref,
            spawner_type
        );
        assert!(
            restored
                .production
                .tiberium_spawning_terrain_cells
                .contains(&spawner_cell)
        );
        assert_eq!(restored.substrate.next_stable_object_id, spawner_id + 1);
        let reconciled = restored
            .resolved_terrain
            .as_ref()
            .expect("reconciled terrain");
        let source = reconciled
            .cell(destroyed_cell.0, destroyed_cell.1)
            .expect("source cell");
        assert_eq!(source.terrain_object_occupation, None);
        assert!(!source.terrain_object_blocks);
        assert!(!source.ground_walk_blocked);
        assert_eq!(source.zone_type, zone_class::GROUND);
        for (cell, bits) in [(damaged_cell, 7), (spawner_cell, 7)] {
            let live = reconciled.cell(cell.0, cell.1).expect("live Terrain cell");
            assert_eq!(live.terrain_object_occupation, Some(bits));
            assert!(live.terrain_object_blocks);
            assert!(live.ground_walk_blocked);
            let raw_mask = crate::sim::terrain_object::terrain_raw_occupation_mask(bits);
            assert_eq!(
                restored
                    .substrate
                    .raw_cell_occupation
                    .ground_bits(cell.0, cell.1)
                    & raw_mask,
                raw_mask
            );
        }
        assert_eq!(
            restored
                .substrate
                .raw_cell_occupation
                .ground_bits(destroyed_cell.0, destroyed_cell.1)
                & crate::sim::terrain_object::terrain_raw_occupation_mask(
                    destroyed.occupation_bits,
                ),
            0
        );
        assert_eq!(
            restored.terrain_costs[&SpeedType::Track].cost_at(destroyed_cell.0, destroyed_cell.1),
            100,
            "canonical post-load costs must ignore the stale caller cache"
        );

        let path = PathGrid::from_resolved_terrain_with_bridges(
            reconciled,
            restored.bridge_state.as_ref(),
        );
        assert!(path.is_walkable(destroyed_cell.0, destroyed_cell.1));
        assert!(!path.is_walkable(damaged_cell.0, damaged_cell.1));
        assert!(!path.is_walkable(spawner_cell.0, spawner_cell.1));
        restored.rebuild_zone_grid(&path);
        assert_ne!(
            restored
                .zone_grid
                .as_ref()
                .and_then(|zones| zones.map_for(MovementZone::Normal))
                .expect("normal zone map")
                .zone_at(destroyed_cell.0, destroyed_cell.1, MovementLayer::Ground),
            ZONE_INVALID
        );
        assert_eq!(restored.state_hash(), authoritative_hash);

        let mut changed_health = GameSnapshot::load(&bytes)
            .expect("health-mutation control snapshot")
            .sim;
        changed_health
            .production
            .terrain_objects
            .get_mut(&damaged_id)
            .expect("damaged Terrain authority")
            .health = 5;
        assert_ne!(changed_health.state_hash(), authoritative_hash);

        let mut changed_lifecycle = GameSnapshot::load(&bytes)
            .expect("lifecycle-mutation control snapshot")
            .sim;
        changed_lifecycle
            .production
            .terrain_objects
            .get_mut(&spawner_id)
            .expect("spawning Terrain authority")
            .lifecycle = TerrainObjectLifecycle::Limbo;
        assert_ne!(changed_lifecycle.state_hash(), authoritative_hash);
    }

    #[test]
    fn saveload_rebuild_is_deterministic() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;

        let terrain = flat_terrain(8, 8);
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        for (stable_id, type_id, category, rx, ry) in [
            (3, "GAPOWR", EntityCategory::Structure, 2, 2),
            (1, "MTNK", EntityCategory::Unit, 3, 2),
            (2, "E1", EntityCategory::Infantry, 3, 2),
        ] {
            let mut entity = GameEntity::test_default(stable_id, type_id, "Americans", rx, ry);
            entity.owner = owner;
            entity.category = category;
            if category == EntityCategory::Infantry {
                entity.sub_cell = Some(2);
            }
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(stable_id);
        }
        let bytes = GameSnapshot::save(&sim, 0, 0, "deterministic_rebuild", 0);

        let mut a = GameSnapshot::load(&bytes)
            .expect("first load should succeed")
            .sim;
        let mut b = GameSnapshot::load(&bytes)
            .expect("second load should succeed")
            .sim;
        rebuild_load_caches(&mut a, terrain.clone());
        rebuild_load_caches(&mut b, terrain);

        assert_eq!(a.terrain_costs, b.terrain_costs);
        assert_eq!(cell_order(&a, 3, 2, MovementLayer::Ground), vec![2, 1]);
        assert_eq!(
            cell_order(&a, 3, 2, MovementLayer::Ground),
            cell_order(&b, 3, 2, MovementLayer::Ground)
        );

        let path_a = PathGrid::from_resolved_terrain_with_bridges(
            a.resolved_terrain.as_ref().expect("terrain restored"),
            a.bridge_state.as_ref(),
        );
        let path_b = PathGrid::from_resolved_terrain_with_bridges(
            b.resolved_terrain.as_ref().expect("terrain restored"),
            b.bridge_state.as_ref(),
        );
        assert_eq!(path_a, path_b);

        a.rebuild_zone_grid(&path_a);
        b.rebuild_zone_grid(&path_b);
        assert_zone_grids_equivalent(
            a.zone_grid.as_ref().expect("zone grid rebuilt"),
            b.zone_grid.as_ref().expect("zone grid rebuilt"),
        );
        assert_eq!(a.state_hash(), b.state_hash());
    }

    // --- Slice 1: reveal/conceal/unlimbo/uninit lifecycle chokepoint ---

    /// `reveal` adds a member; `conceal` removes it from the order but keeps the
    /// store slot (limbo).
    #[test]
    fn reveal_then_conceal_roundtrips_membership() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "MTNK", "Americans", 5, 5));
        sim.reveal(1);
        assert!(sim.substrate.entities.get(1).unwrap().in_logic_vector);
        assert_eq!(sim.live_object_order_snapshot(), vec![1]);
        sim.conceal(1);
        assert!(!sim.substrate.entities.get(1).unwrap().in_logic_vector);
        assert!(sim.live_object_order_snapshot().is_empty());
        assert!(sim.substrate.entities.get(1).is_some()); // conceal keeps the store slot
    }

    /// Slice 3: `unlimbo(ge)` places the entity into BOTH the active order and
    /// occupancy in one atomic call — a caller can never observe it in `logic`
    /// without occupancy, because the method returns only after both. The
    /// house tracks it (Add_Tracking, Added_To_Game). (No-op collapse: same
    /// end state as the old 4-step.)
    #[test]
    fn unlimbo_ge_places_into_logic_and_occupancy_atomically() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let mut ge = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        // `place_spawned` resolves the owner against `sim.interner`; re-intern so
        // the id is valid there (test_default uses the thread-local test interner).
        ge.owner = sim.interner.intern("Americans");
        let (id, outcome) = sim.unlimbo(ge);
        assert!(matches!(outcome, RevealOutcome::Revealed { .. }));

        let e = sim.substrate.entities.get(id).expect("entity in store");
        assert!(e.in_logic_vector, "must be in the active order");
        assert!(!e.lifecycle.in_limbo);
        assert!(e.lifecycle.cell_marked);
        assert_eq!(sim.live_object_order_snapshot(), vec![id]);
        assert!(
            sim.substrate.occupancy.contains_entity(5, 5, id),
            "must be registered in its foundation cell",
        );
        #[cfg(debug_assertions)]
        sim.debug_assert_lifecycle_consistent();
    }

    /// Slice 3: `create_limbo(ge)` stores the entity and its house tracks it
    /// (Add_Tracking) but leaves it OUT of the active order and OUT of occupancy (born InLimbo).
    #[test]
    fn create_limbo_leaves_entity_out_of_logic_and_occupancy() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let mut ge = GameEntity::test_default(2, "E1", "Americans", 6, 6);
        // `place_spawned` resolves the owner against `sim.interner`; re-intern so
        // the id is valid there (test_default uses the thread-local test interner).
        ge.owner = sim.interner.intern("Americans");
        let id = sim.create_limbo(ge);

        let e = sim.substrate.entities.get(id).expect("entity in store");
        assert!(!e.in_logic_vector, "limbo object is not an active member");
        assert!(e.lifecycle.in_limbo);
        assert!(!e.lifecycle.cell_marked);
        assert!(sim.live_object_order_snapshot().is_empty());
        assert!(
            !sim.substrate.occupancy.contains_entity(6, 6, id),
            "limbo object must not occupy a cell",
        );
    }

    /// `uninit` conceals then frees the store slot.
    #[test]
    fn uninit_conceals_then_frees_store_slot() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut ge = GameEntity::test_default(2, "MTNK", "Americans", 4, 4);
        ge.owner = owner;
        sim.substrate.entities.insert(ge);
        sim.reveal(2);
        sim.uninit(2);
        // Two-phase: resolvable-but-Dying until the drain, off the logic order now.
        assert!(sim.substrate.entities.get(2).is_some_and(|e| e.dying));
        assert!(sim.live_object_order_snapshot().is_empty());
        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(2).is_none());
    }

    /// `despawn_entity` is retained and delegates to `uninit`.
    #[test]
    fn despawn_entity_delegates_to_uninit() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let mut ge = GameEntity::test_default(3, "MTNK", "Americans", 6, 6);
        ge.owner = owner;
        sim.substrate.entities.insert(ge);
        sim.reveal(3);
        sim.despawn_entity(3);
        // Two-phase: resolvable-but-Dying until the drain, off the logic order now.
        assert!(sim.substrate.entities.get(3).is_some_and(|e| e.dying));
        assert!(sim.live_object_order_snapshot().is_empty());
        sim.flush_pending_delete();
        assert!(sim.substrate.entities.get(3).is_none());
    }

    /// The membership invariant holds across a mix of reveal/conceal/uninit.
    #[test]
    #[cfg(debug_assertions)]
    fn lifecycle_keeps_membership_invariant() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        for id in [1u64, 2, 3] {
            let mut ge = GameEntity::test_default(id, "MTNK", "Americans", 5, 5);
            ge.owner = owner;
            sim.substrate.entities.insert(ge);
            sim.reveal(id);
        }
        sim.conceal(2);
        sim.uninit(1);
        sim.debug_assert_logic_membership_consistent();
        assert_eq!(sim.live_object_order_snapshot(), vec![3]);
    }

    // --- LogicClass live count-reload pass (scheduler contract) ---

    /// Insert an entity into the store and append it to the active order.
    fn spawn_and_register(sim: &mut Simulation, id: u64) {
        use crate::sim::game_entity::GameEntity;
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "MTNK", "Americans", 5, 5));
        sim.register_live_object(id);
    }

    /// An object the body tail-appends during the pass is ticked later in the
    /// SAME pass, because the live length is re-read after each body call.
    #[test]
    fn logic_scheduler_append_during_pass_ticks_new_tail_same_tick() {
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        spawn_and_register(&mut sim, 1); // A
        spawn_and_register(&mut sim, 2); // B
        // C exists in the store but is NOT yet in the active order.
        sim.substrate
            .entities
            .insert(GameEntity::test_default(3, "MTNK", "Americans", 6, 6));
        assert!(!sim.live_object_order_snapshot().contains(&3));

        let mut visited = Vec::new();
        sim.for_each_live_object(|sim, id| {
            visited.push(id);
            if id == 1 {
                // A's body reveals C at the tail.
                sim.register_live_object(3);
            }
        });

        // C ran in the same pass, after the old tail.
        assert_eq!(visited, vec![1, 2, 3]);
        assert_eq!(sim.live_object_order_snapshot(), vec![1, 2, 3]);
    }

    /// Registering the same object twice is a no-op: the order keeps one entry
    /// and the body runs for it exactly once.
    #[test]
    fn logic_scheduler_duplicate_registration_is_idempotent() {
        let mut sim = Simulation::new();
        spawn_and_register(&mut sim, 1);
        sim.register_live_object(1); // duplicate
        assert_eq!(sim.live_object_order_snapshot(), vec![1]);

        let mut visits = 0;
        sim.for_each_live_object(|_, id| {
            if id == 1 {
                visits += 1;
            }
        });
        assert_eq!(visits, 1);
    }

    /// When the current object unregisters itself, compaction shifts its
    /// successor into the just-processed slot; the cursor still advances, so
    /// that successor is skipped this pass (no index repair).
    #[test]
    fn logic_scheduler_self_unregister_uses_compacting_index_semantics() {
        let mut sim = Simulation::new();
        spawn_and_register(&mut sim, 1); // A
        spawn_and_register(&mut sim, 2); // B
        spawn_and_register(&mut sim, 3); // C

        let mut visited = Vec::new();
        sim.for_each_live_object(|sim, id| {
            visited.push(id);
            if id == 2 {
                sim.unregister_live_object(2); // B removes itself
            }
        });

        // A and B were visited; C (shifted into B's slot) is skipped this pass.
        assert_eq!(visited, vec![1, 2]);
        // Order is compacted, order-preserving — B gone, C retained.
        assert_eq!(sim.live_object_order_snapshot(), vec![1, 3]);
    }

    /// Premise: a snapshot walk MISSES a same-pass append that the live pass
    /// catches. This is the drift the live pass exists to remove.
    #[test]
    fn logic_scheduler_snapshot_walk_misses_same_pass_append() {
        use crate::sim::game_entity::GameEntity;

        // Snapshot path: appended object is invisible to this pass.
        let mut sim = Simulation::new();
        spawn_and_register(&mut sim, 1);
        spawn_and_register(&mut sim, 2);
        sim.substrate
            .entities
            .insert(GameEntity::test_default(3, "MTNK", "Americans", 6, 6));
        let order = sim.live_object_order_snapshot();
        let mut snapshot_visited = Vec::new();
        for &id in &order {
            snapshot_visited.push(id);
            if id == 1 {
                sim.register_live_object(3);
            }
        }
        assert_eq!(snapshot_visited, vec![1, 2]); // C missed

        // Live path on an equivalent setup: appended object is visited.
        let mut sim2 = Simulation::new();
        spawn_and_register(&mut sim2, 1);
        spawn_and_register(&mut sim2, 2);
        sim2.substrate
            .entities
            .insert(GameEntity::test_default(3, "MTNK", "Americans", 6, 6));
        let mut live_visited = Vec::new();
        sim2.for_each_live_object(|sim, id| {
            live_visited.push(id);
            if id == 1 {
                sim.register_live_object(3);
            }
        });
        assert_eq!(live_visited, vec![1, 2, 3]); // C caught

        assert_ne!(snapshot_visited, live_visited);
    }

    /// `Command::ForceAttackCell` is serializable (replay/snapshot back-compat).
    #[test]
    fn force_attack_cell_command_serializes() {
        use crate::sim::command::Command;
        let cmd = Command::ForceAttackCell {
            attacker_id: 7,
            target_rx: 100,
            target_ry: 200,
        };
        let bytes = bincode::serialize(&cmd).expect("serialize should succeed");
        let restored: Command = bincode::deserialize(&bytes).expect("deserialize should succeed");
        assert!(matches!(
            restored,
            Command::ForceAttackCell {
                attacker_id: 7,
                target_rx: 100,
                target_ry: 200
            }
        ));
    }

    /// Literal saved Cell lists survive re-entry order and actual restore.
    /// Restore hydrates metadata; it does not replay the historical enter stamps.
    #[test]
    fn saveload_occupancy_list_order_survives_reentry() {
        use crate::map::entities::EntityCategory;
        use crate::sim::game_entity::GameEntity;
        let mut sim = Simulation::new();
        for _ in 0..3 {
            let id = sim.allocate_stable_id();
            let mut entity = GameEntity::test_default(id, "E1", "Americans", 5, 5);
            entity.category = EntityCategory::Infantry;
            entity.lifecycle.in_limbo = false;
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(id);
        }
        sim.interner = crate::sim::intern::test_interner();
        sim.remove_entity_occupancy(1);
        sim.add_entity_occupancy(1);
        let list = |sim: &Simulation| -> Vec<u64> {
            sim.substrate
                .occupancy
                .get(5, 5)
                .unwrap()
                .iter_layer(MovementLayer::Ground)
                .map(|o| o.entity_id)
                .collect()
        };
        assert_eq!(list(&sim), vec![1, 3, 2]);
        // Snapshot deserialization intentionally executes native Scenario Seed0.
        // Compare this state-only roundtrip on that same admitted RNG cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let saved = GameSnapshot::save(&sim, 0, 0, "re-entry list", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(list(&restored), vec![1, 3, 2]);
        assert_eq!(restored.state_hash(), hash);
        restored.remove_entity_occupancy(3);
        assert_eq!(list(&restored), vec![1, 2]);
        restored.add_entity_occupancy(3);
        assert_eq!(list(&restored), vec![3, 1, 2]);
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(list(&restored), vec![3, 1, 2], "fixup is idempotent");
    }
}
