# Building turret animations (`TurretAnim.*`)

Phobos lets a BuildingType drive its SHP turret animation (the
`BuildingAnimSlot::Turret`, slot 9) with more than one frame per facing, plus
separate low-power and firing blocks. Keys are read from the type's
`rulesmd.ini` section, **not** ART (`Ext/BuildingType/Body.cpp:260-265`); the
constructor defaults are `Ext/BuildingType/Body.h:232-237`.

```ini
[SOMEBUILDING]                     ; BuildingType
TurretAnim.IdleFrames=1            ; integer, default 1
TurretAnim.LowPowerIdleFrames=0    ; integer, default 0
TurretAnim.FiringFrames=0          ; integer, default 0
TurretAnim.LowPowerFiringFrames=0  ; integer, default 0
TurretAnim.IdleRate=1              ; integer, game frames per animation frame
TurretAnim.FiringRate=1            ; integer, game frames per animation frame
```

All six are `Valueable<int>` and are read with VERA's Ares reader
(`read_ares_int`), including its `$dd` / `ddh` forms. Phobos' own serializer
duplicates `FiringFrames` and drops `FiringRate`
(`Ext/BuildingType/Body.cpp:482`); VERA reads the type section directly, so it
does not reproduce that bug.

## Behavior

`BuildingExt::GetTurretFrame` (`Ext/Building/Body.cpp:488-569`) is called once
per `BuildingClass::AnimationAI` frame by hook `0x451242`
(`Ext/Building/Hooks.cpp:1318-1331`), which writes
`pAnim->Animation.Value = GetTurretFrame(pThis)` and clears
`Animation.Step`, and only when a slot-9 animation exists. That hook is the
single call site; calling it twice per frame would double the rate.

- `facing = PrimaryFacing.Current().GetValue<5>()`, `shapeFacing =
  ObjectClass::BodyShape[facing]` (`Body.cpp:492-493`, 0..31).
- `isLowPower = !StuffEnabled || !IsPowerOnline()`, `isFiring =
  TurretAnimFiringFrame != -1` (`Body.cpp:495-496`).
- The SHP holds four blocks of `32 * Frames` in the order Idle,
  LowPowerIdle, Firing, LowPowerFiring; `baseOffset` sums the blocks before
  the selected one (`Body.cpp:498-528`).
- Block choice: low power + firing + `LowPowerFiringFrames>0` →
  LowPowerFiring; low power + `LowPowerIdleFrames>0` → LowPowerIdle; normal +
  firing + `FiringFrames>0` → Firing; otherwise Idle. A block with 0 frames is
  skipped (documented in `docs/Fixed-or-Improved-Logics.md:1254-1270`).
- Frame advance: `RateTick++`; only at `RateTick >= Rate` does the animation
  frame advance and the tick clear. `Rate` is a frame interval, so `Rate<=0`
  advances every frame. Firing runs to `framesPerFacing` then resets
  `FiringFrame=-1`, `IdleFrame=0`, `RateTick=0`; the idle branch advances only
  when `framesPerFacing > 1` (`Body.cpp:530-566`).
- Return `baseOffset + shapeFacing * framesPerFacing + animFrame`
  (`Body.cpp:568`).

`BuildingClass::Mission_Attack` hook `0x44B6C7`
(`Ext/Building/Hooks.cpp:1333-1357`) arms the firing animation at the firing
instant: with `HasTurret()`, a slot-9 animation, the active `FiringFrames>0`
and `FiringFrame == -1`, it sets `FiringFrame=0, RateTick=0`. Firing frames
start when attacking and the weapon can fire; they do not wait for a projectile
(`docs/Fixed-or-Improved-Logics.md:1258`).

The building instance keeps exactly three fields (`Ext/Building/Body.h:28-30`,
constructor `Body.h:47-49`, serialization `Body.cpp:603-605`):
`TurretAnimIdleFrame=0`, `TurretAnimFiringFrame=-1`,
`TurretAnimRateTick=0`. Idle and firing share one `RateTick`; there is no
separate firing-rate tick and no `UpdateTypeData` reset.

## Where VERA20K owns it

`rules::phobos_turret_anim` owns the six keys (`PhobosTurretAnim`), the
instance state (`TurretAnimState`) and the `GetTurretFrame`-equivalent pure
function `get_turret_frame` / `arm_firing`. `ObjectType` carries the type data,
and `GameEntity::phobos_turret_anim` carries the instance state
(`#[serde(default)]`); it is presentation state and
is deliberately left out of `world_hash`, like the voxel `TurretAnimCounter`
(`src/sim/combat/gattling.rs:528`).

`src/sim/world/techno_ai/building_missions.rs` is the owner:
`update_animation` (the `UpdateAnimation @ 0x004509D0` path) calls
`update_turret_anim_frame` once per frame and writes the absolute frame through
`set_anim_frame_and_z_adjust`; `attack_arm`'s OK tail calls
`arm_turret_anim_firing` (the native `0x44B6C7` is not on the Facing or REARM
arms). Both helpers gate on a present slot-9 animation and `HasTurret()`
(vt+0x3FC, `0x4527D0`); the frame helper also skips `TurretAnimIsVoxel` types
and uses `DRAGON_FRAME_TABLE` (`0x007F4890`, `(28 - i) & 31`) as
`ObjectClass::BodyShape`. Low power is `!StuffEnabled ||
!IsPowerOnline()`, with `Simulation::building_operational_state`
(`0x004555D0`) supplying the power-ratio, EMP, health and mission terms.
Stock builds compile these paths out entirely.

Drawing: `sync_rules_turret_anim` (`src/sim/building_art.rs:798`) seeds slot 9
from the rules `TurretAnim=` name at every Grand_Opening (`0x00446662`), and
`rules_turret_anim_config` (`src/app/presentation/instances/shp.rs:1828`)
synthesizes the slot-9 `BuildingAnimConfig` for `emit_building_anims`. The
rules key takes precedence over ART's legacy `TurretAnim` entry so the slot has
one owner; ART still supplies the slot for types whose rules section leaves the
key unset. `TurretAnimIsVoxel=yes` types draw through
`emit_building_turret_vxl` instead.

The unused-key case yields one frame per facing (`shapeFacing * 1`), matching
the documented default.

## Residuals

- **Damaged/garrisoned slot-9 names are not read.** `0x004644FD` and
  `0x00464594` select `TurretAnimDamaged=` and the image's
  `TurretAnimGarrisoned=`; the slot keeps the normal name instead
  (`src/sim/building_art.rs:798`). Trigger: a type that authors either key.
  Effect: the wrong anim plays while damaged or garrisoned. Frequency: retail
  dormant. Risk: visual-only.
- **ART `TurretAnim=` remains a fallback.** Rules owns slot 9 when the type
  declares `TurretAnim=` (`rules_turret_anim_config`,
  `src/app/presentation/instances/shp.rs:1828`); a type whose rules section
  leaves the key unset still draws the ART-declared slot, which native does
  not. Trigger: mod ART with `TurretAnim=` but rules without it. Effect: an
  extra slot-9 image. Frequency: retail dormant. Risk: low.
- **`Animation.Step=0` is not written.** The hook clears the slot-9 anim's
  Step (`Hooks.cpp:1324`); VERA's AnimClass AI keeps the type's configured
  step while advancing the frame. Trigger: an anim type with `Step` above 0.
  Effect: the anim's own frame advance is not stopped. Frequency: only such a
  type. Risk: visual-only.
