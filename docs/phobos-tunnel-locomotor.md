# Tunnel locomotor

`Locomotor={4A582743-9839-11D1-B709-00A024DDAFD1}` is Tiberian Sun's
`TunnelLocomotionClass`, which gamemd still registers
(`GetClassID 0x72A110` returns that CLSID). Retail YR never selects it, so the
class was recorded as dormant and the CLSID fell back to the Teleport
constructor seed. A mod can select it: the installed
`F:/Chinese Dream 2.0/rulesmd.ini` gives `[ZDWZ]`, `[SAPC2]`, `[ZDDTRUCK]` and
`[TANKCHONGT]` this CLSID together with `MovementZone=Subterrannean`, and under
the old table those units teleport-moved instead of burrowing.

## The class

Object size `0x3C`: `State(+0x18)`, `Coords(+0x1c)`, `DigTimer(+0x28)`,
`bool38(+0x38)`. Vtable `0x7F5A24`.

| State | Handler | Behaviour |
|---|---|---|
| 0 `Idle` | — | `Is_Moving` false, no fire override |
| 1 `PreDigIn` | `0x7291F0` | turn the hull (`0x4C9480` gates while rotating, `0x4C93D0` compares, `Do_Turn` otherwise); on alignment `DigTimer = round(64.0 / Speed / frame_scale)` (binary64 `0x7F5B18` = 64.0, `GetType()->+0x71c`), `State = 2` |
| 2 `DiggingIn` | `0x729370` | wait `DigTimer` (`0x7E1718` = 1.0 fraction); `Set_AnimRate(1.0)`, `State = 3` |
| 3 `DugIn` | `0x728F86` | `z -= max(5, round(Speed * frame_scale))`, clamped at `0xFFFFFF00` (-256); at -256 → `State = 4` (Phobos marks Underground at `0x728F9A`/`0x729021`) |
| 4 `Digging` | `0x729580` | straight-line travel to `Coords` at `TunnelMovementSpeed` (`0x7F5B20` = 19.0); the 2-D distance under `0x14` takes the arrival branch at `0x7297F6` → `State = 5` (Phobos unmarks) |
| 5 `PreDigOut` | `0x7298F0` | raise `z` with the same `max(5, …)` step to the surface |
| 6 `DiggingOut` | `0x729AA0` | dig-out timer |
| 7 `DugOut` | `0x729480` | last timer, then `State = 0` |

Pure answers:

- `In_Which_Layer 0x72A1A0`: `Underground (0)` **only** in state 4, `Ground (2)`
  otherwise — so the layer is derived from `State`, never cached.
- `Visual_Character 0x7291D0`: state 4 answers `4 + (raw ? 1 : 0)`; the unit is
  not drawn while underground.
- `Is_To_Have_Shadow 0x72A060`: false in states 3, 4, 5.
- `Can_Fire 0x72A1C0`: base `0x55AD00` first, then any `State != 0` answers 7.
- `Is_Surfacing 0x72A1E0`: states 5 and 6.
- `Mark_All_Occupation_Bits` is the empty base `0x4B6620` (slot 39): a Tunnel
  unit never claims or releases occupation, so it must not enter the Drive
  track.
- `Move_To 0x728AF0` stores the destination **cell's centre**, and an object
  already below ground (`[+0x1c8] < 0`) skips straight to state 4.

The layer answer is what the package-93 `AU` chain consumes:
`display_registry::entity_is_underground` reads `LocomotorState::layer()`,
`target_layer()` returns 0, and `fire_error.rs`'s `0x6FC749` gate plus the
`0x6F37EB` selection arm admit only `AU=yes` weapons.

## VERA mapping

- `LocomotorKind::Tunnel`, appended last so every existing discriminant,
  `table_index()` row and serialized order is unchanged.
- `LocomotorRuntimePayload::Tunnel(TunnelRuntime)`
  (`src/sim/movement/tunnel_locomotion.rs`): `State`, `DigTimer.start`,
  `DigTimer.duration`. The world hash folds all three under tag 11.
- `LocomotorState::layer()` answers the class's `In_Which_Layer` for Tunnel and
  the cached layer otherwise.
- `Simulation::process_tunnel_locomotor` (`src/sim/world/object_turn.rs`)
  dispatches the class Process once per object turn with the type's `Speed=`.
- `navcom`'s stop path aborts an armed dig to Idle.

## Residuals

- The dig and dig-out **animations** (`0x1c8`-byte Anim, ctor `0x421EA0`,
  `0x600` flags) are not created; only the timers that gate them are ported.
- `ProcessPreDigOut 0x7298F0` raises Z to the cell's ground height
  (`MapClass::0x578080` at `0x72991D`, every frame) by
  `max(5, round(rate * frame_scale))` and arms the dig-out timer with
  `round(64.0 / Speed)` at `0x729A54`. VERA passes the same canonical cell
  query the other movers build and re-asks it each frame, so a tunnel that
  travels under a ramp surfaces on the destination cell's ground; the
  dig-in sample survives only as the answer when the caller has no terrain.
- `ProcessDugIn` (inline at `0x728F86`) reads no ground at all: the entry
  guard compares the Location Z against `0xFFFFFF00` (`0x728FC4`,
  `jle 0x729021`) and the step clamp writes the same constant
  (`0x729001`), so every tunnel sinks to one shared plane whatever ground it
  dug in from.
- `Can_Enter_Cell 0x72A090` answers 7 only when `0x486FF0` rejects the cell —
  that body tests `0x578540` (map validity), the overlay table `+0x305` and the
  flags at `+0x11c`/`+0x140`, i.e. a void cell, not terrain, walls or
  buildings. Nothing in VERA's tunnel chain calls it (the controller writes
  coordinates directly), so it stays unported by design.
- `Z_Adjust 0x729E50` is draw-only and belongs to the presentation layer.
- `Stop_Moving 0x728C00`'s table `0x728E08` is ported by its targets: states
  1/2/3 abort the dig to Idle, state 4 (`0x728C1E`) surfaces in place, states
  5/6/7 finish surfacing. The bodies at `0x728DD0`, `0x728DD9` and `0x728DE0`
  are unread.
- `Is_Moving_Now 0x728A60` answers `State == 1` (`PreDigIn`) only;
  `ready_producer::ready_state_for` maps the class state to
  `LocomotorReadyState::Tunnel { state }`, so the Mission gate sees a unit that
  has just taken a Tunnel `Move_To` as moving until the hull finishes turning.
  Every dig/underground/surfacing state answers false, as it does natively.
  `motion_query::is_moving_now` keeps answering from the same class state for
  the other callers.
- `[General] TunnelSpeed` is **not** this class's input: the class travels at
  the fixed binary64 `0x7F5B20` = 19.0. The key itself is read once, by the
  `[General]` rules block, and nothing consumes the field: the literal
  `TunnelSpeed` sits at VA `0x0083CA34` between `HoverDampen 0x0083CA28` and
  `CrewEscape 0x0083CA40`, its only `imm32` reference is the `push 0x83CA34` at
  `0x0066EDA1`, and `CCINIClass::ReadDouble`-alike `0x005283D0` then stores a
  binary64 at `RulesClass+0x20` (`fstp qword ptr [esi+0x20]`, `0x0066EDAE`).
  A scan of the whole `.text` finds one non-stack 8-byte `[reg+0x20]` read
  (`0x0042E276`, an unrelated object) and none inside
  `0x728E30..0x72A200`, so retail's `TunnelSpeed=1` (`rulesmd.ini:365`) is a
  dormant Tiberian Sun leftover. VERA's `RuleSet::general.tunnel_speed`
  mirrors that read and likewise has no consumer.
