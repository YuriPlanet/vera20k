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

- The dig and dig-out **animations** are ported (2026-11-19). Native
  constructs one `AnimClass` at each of the four transitions —
  `PreDigIn→DiggingIn` (`0x00729360`), `DiggingIn→DugIn` (`0x00728F6F`),
  `PreDigOut→DiggingOut` (`0x00729A2F`) and `DiggingOut→DugOut`
  (`0x00729459`) — after a `0x1C8`-byte allocation (`PUSH 0x1C8; CALL
  0x007C8E17`, `TEST EAX,EAX` guarded), all with the identical ctor row
  `(type, &Owner->Location, 0, 1, 0x600, 0, 0)` at
  `AnimClass::AnimClass @ 0x00421EA0`: `PUSH 0x0`, `PUSH 0x0`, `PUSH 0x600`,
  `PUSH 0x1`, `PUSH 0x0`, the by-value `CoordStruct` copied from
  `Owner->Location` (`+0x9C` of `[this+8]` / `[this+0xc]`), then `PUSH type`.
  The type is `RulesClass+0x294`, the **`[AudioVisual] Dig=`** key:
  `0x0066A593` pushes the key string `0x0083A5CC` (`Dig`) with the section
  `0x00889F64`, the `AnimType` lookup `0x00428B80` resolves the stored name at
  `0x0066A5A8`, and `0x0066A5BA` writes it back; the `RulesClass` constructor
  default is null (`0x00665A10`). VERA carries the name as
  `RuleSet::general.dig_anim` (retained by the `[AudioVisual]` Process pass,
  `RulesLayerKind` ordered), has `TunnelRuntime::process` return
  `TunnelStep { moved, dig_anim }` marking exactly those four state pairs, and
  builds the anim from `Simulation::process_tunnel_locomotor` through the
  shared `(delay 0, loop 1, drawFlags 0x600, zAdjust 0, reverse 0)` row
  (`sim::superweapon::spawn_super_anim`) at the owner's Location. Retail
  `rulesmd.ini` has no `Dig=` (only the commented `DigSound=` at line 672), so
  the pointer stays null and nothing is constructed either way — the arm is a
  dormant TS leftover, and a mod that sets the key gets the animation. Tests:
  `tunnel_locomotion::tests::the_dig_anim_marks_exactly_the_four_transitions`
  and `native_processing::tests::audio_visual_dig_retains_its_anim_type_across_layers`.
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
- `Z_Adjust 0x729E50` is draw-only and implemented (2026-11-19) in
  `render::foot_depth::tunnel_z_adjust` + `app/presentation/instances/foot_depth.rs`.
  Its consumer is `FootClass`'s z composite `0x004DAFC0`: it stores the loco
  answer in `[esp+8]` (`0x4DAFF3`), sums
  `0x00704350` (`0x4DB08C`) and the fudge maximum (`0x4DB091`), and adds the
  stored loco term last (`0x4DB098`), so the locomotion answer is a third
  additive term beside `additional_z`, not a replacement. It transforms
  `Owner->Location` (`Owner` `[this+8]`, Location `+0x9c`) through `0x6D2140`
  (`ecx = [0x887324]`), asks `0x578080` for that cell's ground and transforms
  the ground point the same way, so `edi` is the sprite's pixel height above
  its ground — the same quantity `0x00704350` derives, which is why a Tunnel
  hull counts its height twice. It then builds
  `fraction = (total - remaining) / total` from the dig timer (`[+0x30]`
  total, `[+0x24]` start, `[+0x2c]` duration, `[0xa8ed84]` current frame;
  `fraction = 1.0` when `total` is 0) and `ecx = (((owner+0x388` current
  facing `>> 7) + 1) >> 1) & 0xFF`, `owner+0x388` being the hull
  `FacingClass` whose interpolated value `0x4C93D0` returns. The switch on
  `State - 2` (`0x72A000` = `0x729F74/0x729FCC/0x729F40/0x729FDE/0x729F52`)
  adds the dword constant `45` (`0x844A9C`, `0x844A98`):
  `DiggingIn` adds 45 when `70 < ecx < 140`, otherwise `ftol(45 * fraction)`,
  and latches `[this+0x34] = 1` once `fraction > 0.4` (`0x7E3550`) — that byte
  has no other reader, so the latch has no visible effect; `DugIn`, `Digging`
  and `PreDigOut` add 45; `DiggingOut` adds `ftol(45 * (1 - fraction))`; every
  other state falls through to the base class `0x55ABA0` and contributes
  nothing. VERA's `unit_z_adjust_in_runtime` now supplies that term whenever
  the entity carries a Tunnel runtime, using the frame's animated
  `body_facing` and `DigTimer` as above.
- Residual: the native `edi` comes from the `0x6D2140` projection while
  `additional_z` comes from `0x6D20E0`; VERA evaluates both with
  `adjust_for_z_standard`, so a tunnel hull's doubled height term matches
  gamemd only as far as those two projections agree (they differ by at most
  the standard-adjust correction on a non-flat cell). The `[+0x34]` latch is
  not stored.
- `Stop_Moving 0x728C00` dispatches on `State - 1` through table `0x728E08`,
  read by target:
  - states 1/5/6/7 → `0x728DE0` (unchanged) after writing the null coordinate
    (`[0xB0F910..0xB0F918]`, BSS zeros) over the stored `+0x18` coord. State 1
    then leaves at the next `ProcessPreDigIn`, whose null-coordinate arm is
    `Idle`;
  - state 2 (`DiggingIn`) → `0x728DD9` sets `DugOut` (7): the timer never
    finished, so nothing sank;
  - state 3 (`DugIn`) → `0x728DD0` sets `PreDigOut` (5): rise from the Z
    reached so far;
  - state 4 (`Digging`) → `0x728C1E`, which measures the location against the
    stored coord (`0x4CAC40`) and surfaces in place.
  `TunnelRuntime::stop_moving` now maps exactly that table (an earlier port
  folded states 2 and 3 into `Idle`, which left a half-sunk hull buried).
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
