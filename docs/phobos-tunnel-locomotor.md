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
- `ProcessPreDigOut 0x7298F0`'s `DigTimer` re-arm is unread, so the dig-out
  timer reuses the dig-in duration formula.
- `Can_Enter_Cell 0x72A090` (`0x486FF0` cell test) and `Z_Adjust 0x729E50`
  are read only far enough to confirm they do not change the state chain.
- `Stop_Moving 0x728C00`'s state jump table `0x728E08` is ported as "abort to
  Idle"; its per-state bodies are unread.
- `Is_Moving_Now 0x728A60` (state 1 only) is not reported by
  `ready_producer::ready_state_for`, which answers `None` for Tunnel.
- `[General] TunnelSpeed` is **not** this class's input: the class travels at
  the fixed binary64 `0x7F5B20` = 19.0. Where the rules key is read stays open.
