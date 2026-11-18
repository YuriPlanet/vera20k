# Hover vehicle drown death (`HoverDrownable`)

Whether a hover Unit that shut down over water dies when it lands. The type's
`Nullable<bool>` overrides the global, and the landing arm picks the damage
packet from it.

## Keys

| Key | Type | Default |
|---|---|---|
| `HoverDrownable=` | `Nullable<bool>` | unset |
| `[General] HoverDrownable=` | `bool` | `yes` |

Read: `UnitTypeExt::LoadFromINI` (`Ext/UnitType/Body.cpp:87`), declaration
`Ext/UnitType/Body.h:72`, constructor `Body.h:133`. The global is read at
`Ext/Rules/Body.cpp:475`, declared `Ext/Rules/Body.h:385`, constructed `yes` at
`Body.h:890`.

VERA reads the type key in `src/rules/phobos_hover_drown.rs`, owner
`ObjectType::phobos_hover_drown`; the global is
`RuleSet::phobos_globals.hover_drownable`.

## Behaviour

Two halves, both on the same flag.

1. `HoverLocomotionClass_Process` hook `0x514C07`
   (`Ext/Techno/Hooks.cpp:997-1006`) drops the techno as a bomb and then raises
   `TechnoExt::HoverShutdown` (`Ext/Techno/Body.h:81`, saved at
   `Ext/Techno/Body.cpp:1200`, constructed `false` at `Body.h:135`):

   ```cpp
   pTechno->DropAsBomb();
   TechnoExt::Fetch(pTechno)->HoverShutdown = true;
   ```

   VERA's owner is `Simulation::hover_spin_tail`
   (`src/sim/movement/hover_process.rs`): the existing water gate calls
   `drop_as_bomb`, and the `phobos` build then marks the entity
   (`GameEntity::mark_hover_shutdown`, `ObjectFallState::hover_shutdown`).

2. `ObjectClass_Update_FallingDown_ToDead` hook `0x5F4021`
   (`Ext/Techno/Hooks.cpp:1014-1043`) reads and clears the flag inside the
   `IsABomb && IsAlive` arm, before it picks the damage:

   ```cpp
   if (pThis->IsABomb && pThis->IsAlive) {
       const bool hoverShutdown = pExt->HoverShutdown;
       pExt->HoverShutdown = false;
       if (hoverShutdown) {
           if (HoverDrownable.Get(Global()->HoverDrownable)) {
               damage = pThis->Health;            // drown
               ReceiveDamage(&damage, 0, C4Warhead, ...);
               pThis->IsABomb = false;
               return;
           }
           // else falls through to the falling-down damage model
       }
       ...
   }
   ```

   VERA's owner is the landing arm of `Simulation::object_ai_fall`
   (`src/sim/movement/parachute_descent.rs:145-156`). The forced C4 receiver is
   the existing stock arm; the `phobos` build only chooses its amount:

   - `HoverDrownable` true — the whole current `Health`, the stock packet;
   - `HoverDrownable` false — the shutdown falls through into the canopy arm's
     `FallingDownDamage` ratio block (`Ext/Techno/Hooks.cpp:1038-1084`), so the
     packet is `Strength * ratio` (the default `1.0` packets the whole
     `Strength`), `Health * |ratio|` for a negative ratio, and `.Water` on a
     bridge-less water/beach cell. Regression test
     `a_non_drowning_hover_shutdown_takes_the_falling_down_ratio`. Before the
     ratio block, native packets the whole `Health` when the landing cell is not
     `IsClearToMove` (`:1043-1055`); VERA's `landing_cell_clear` reads the cell's
     reduced `ZoneType` against the mover's `MovementZone` and the `LandType`
     row's speed for its `SpeedType`, so a blocked landing dies before any ratio
     applies (test `a_blocked_landing_cell_takes_the_whole_health_packet`).

   The flag is hashed while set (`src/sim/world/world_hash.rs`) because it
   gates that amount; a cleared flag leaves the idle hash untouched.

## Residuals

- **`FallingDownDamage` / `.Water` / `.AllowEMP` are ported** (settled
  2026-11-18). The three `Nullable`s are read per type
  (`Ext/TechnoType/Body.cpp:1140-1142`) at `src/rules/object_type.rs:6523-6533`
  (fields `:1520-1529`); the `[CombatDamage]` globals are the fallbacks
  (`src/rules/combat_damage.rs:255-257`). Their consumer is
  `Simulation::phobos_falling_damage` (`src/sim/movement/parachute_descent.rs`),
  reached both from the canopy landing arm (`:154`) and from a non-drowning
  hover shutdown, and it implements the ratio tiers (`Hooks.cpp:1062-1084`):
  `Health * |ratio|` for a negative ratio, `Strength * ratio` up to `1.0`,
  the value itself above `1.0`, with `.Water` on a bridge-less water/beach cell
  and `AllowEMP=no` keeping the vanilla full-health packet on an EMP-locked
  unit. Regression tests `src/sim/movement/parachute_descent.rs:1078-1199`
  cover the type value, the global fallback, `.Water`, the negative ratio, the
  `AllowEMP=no` arm and the hover-shutdown fall-through. No residual on the
  ratio itself: a rules set that lowers it lowers the packet.
- **`IsClearToMove` landing pre-check is ported** (`Ext/Techno/Hooks.cpp:1043-1055`).
  `Simulation::landing_cell_clear` answers false when the cell is outside the
  playfield, when `MOVEMENT_ZONE_PASSABILITY[zone_type]` rejects the mover's
  `MovementZone`, or when the `LandType` row gives its `SpeedType` a zero speed;
  the whole current `Health` then goes before the ratio block. An unknown cell,
  type, `SpeedType` or an unbound land row answers clear, keeping the previous
  packet.
  `onBridge` is the cell's `ContainsBridge()` and, with level `-1`, moves the
  call onto the deck (`0x0048353C..0x0048354E`): that branch is ported too —
  a bridge cell skips both ground arms and reads the deck occupation byte
  (`+0x128`) masked by the two ignore flags (`0xE0` then `0x5F`, leaving
  `0x40`). So a mover lands on a deck over water its `SpeedType` could not
  cross, and an occupied deck still refuses the landing. Regression test
  `a_bridge_deck_landing_reads_the_deck_plane_not_the_water_ground`
  (`src/sim/movement/parachute_descent.rs`). No residual on the deck arm: the
  ground arms below the deck are deliberately not consulted, as native does.
- **`HoverDrownable=no` still kills at the default rules**, because the
  default ratio is `1.0` and that packets the whole `Strength`. The key is a
  dependency of `FallingDownDamage`; it only prevents death once that ratio is
  below the health/strength ratio. This is native behaviour, not a VERA
  choice — the page records it because the feature is easy to read as
  "`no` survives".
- **The drop itself is modelled as stock.** Native only reaches `DropAsBomb`
  through the Phobos hook `0x514C07`; VERA's hover water gate predates this
  feature and drops in both builds. The mark is the only `phobos` half.

## Tests

- `src/rules/phobos_hover_drown_tests.rs` — the `Nullable`, the global
  fallback, and the `<default>`/blank reset.
- `src/sim/movement/hover_process.rs` —
  `the_hover_shutdown_marks_the_techno_it_drops`: a spin ending over water
  drops and marks; one ending over clear ground does neither.
- `src/sim/movement/parachute_descent.rs` —
  `only_the_bomb_landing_arm_takes_the_hover_shutdown`: the bomb arm reads and
  clears the flag, a canopy landing never reads it. In this workspace the test
  panics on `intern.rs:201` (its `GameEntity::test_default` carries an id the
  `test_interner` never produced); the same panic reproduces on the clean
  baseline, so it is an environment red, not a behaviour check.
- `src/sim/movement/parachute_descent.rs` —
  `a_non_drowning_hover_shutdown_takes_the_falling_down_ratio`: with
  `HoverDrownable=no` and `FallingDownDamage=0.25` the landing packet is
  `Strength * 0.25` (the unit survives), while the default ratio still kills.
- `src/sim/movement/parachute_descent.rs` —
  `a_blocked_landing_cell_takes_the_whole_health_packet`: a Water-column cell
  under `MovementZone Normal` kills outright even with `FallingDownDamage.Water`
  at `0.1`, while a ground-classed cell takes that ratio.
