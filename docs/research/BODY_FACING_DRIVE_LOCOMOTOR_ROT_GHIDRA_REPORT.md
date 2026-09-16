# Body Facing ROT & Drive-Locomotor In-Place Rotation — Ghidra Report

Date: 2026-07-19
Confidence: HIGH on body/turret facing identity, ROT source, and duration formula
(each backed by ≥2 independent binary consumers). MEDIUM on the exact turret-vs-barrel
split of the +0x370 facing (irrelevant to the body question). PARTIAL on the exact
in-place-vs-arc translation threshold (located, not fully decoded).

Corrects: [FRAME_BASIS_MOVEMENT_TURRET_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FRAME_BASIS_MOVEMENT_TURRET_GHIDRA_REPORT.md) §5.2 / Handoff H-1 and
`UNITCLASS_TURRET_TRACKING_AND_FIRE_TIMING_GHIDRA_REPORT.md` §1.2 — both claim the
**body** facing is the FacingClass at TechnoClass+0x370 with a constructor-default
**ROT=3**. That is **WRONG**. See "Verdict" below.

## Summary verdict

The driving/drawn **hull (body) facing is TechnoClass+0x388 with ROT = the unit's rules
`ROT=` value** (not 3, not +0x370). The drive locomotor turns +0x388; the hull is drawn
from +0x388. The +0x370 facing (ROT=3) is a separate turret/barrel VISUAL facing that no
movement code ever touches.

## The three FacingClass instances on a TechnoClass

All three are constructed in `TechnoClass::Constructor` (0x006f2b40):
- `disassemble_function 0x006f2b40`:
  - `006f2ee0 LEA ECX,[ESI+0x370]; 006f2ee6 CALL 0x004c91e0` with `006f2ede PUSH 0x3`
    → FacingClass ctor **with ROT arg = 3** at **+0x370**.
  - `006f2eeb LEA ECX,[ESI+0x388]; 006f2ef1 CALL 0x004c91c0` → ctor **no ROT** at **+0x388**.
  - `006f2ef6 LEA ECX,[ESI+0x3a0]; 006f2efc CALL 0x004c91c0` → ctor **no ROT** at **+0x3a0**.
- `decompile_function 0x004c91e0` (ctor-with-ROT) and `0x004c91c0` (ctor-no-ROT): both zero
  the facing and set StartFrame=g_CurrentFrameCounter; the ROT variant stores `ROT<<8` at +0x14.

`UnitClass::Constructor` (0x007353c0) then `SetROT`s **only +0x388 and +0x3a0** to the rules
ROT, leaving +0x370 at its ctor default 3:
- `disassemble_function 0x007353c0`: `00735570 MOV EAX,[EAX+0x71c]; MOV ECX,EBP(=+0x388); CALL
  0x004c9680` and `00735584 MOV EDX,[ECX+0x71c]; MOV ECX,EBX(=+0x3a0); CALL 0x004c9680`.
- `get_function_callers 0x004c9680` (SetROT): only Aircraft/Building/Infantry/Unit ctors — the
  base Techno/Foot ctors never SetROT, so +0x370 keeps ROT=3.

| Offset | ROT after ctor | Role | Evidence |
|--------|---------------|------|----------|
| **+0x388** | **rules ROT** | **BODY / hull (PrimaryFacing)** | Drive locomotor `Do_Turn` sets it; turret matrix uses it as body base; Facing_Update realigns turret to it |
| +0x3a0 | rules ROT | Turret tracking (SecondaryFacing) | Facing_Update Set_Desires it toward target for turreted units |
| +0x370 | **3** | Turret/barrel VISUAL facing | Read by turret VXL matrix + unit turret draw; set at Unlimbo; **never touched by movement** |

### Why +0x388 = BODY (three independent consumers)
1. **Drive locomotor** — `DriveLocomotionClass::Do_Turn` (0x004b0ef0),
   `disassemble_function 0x004b0ef0`: `MOV ECX,[EDX+0x8] (linked object); ADD ECX,0x388; CALL
   0x004c9220 (Set_Desired)`. The locomotor turns **+0x388** — this is the hull heading for driving.
2. **Turret matrix base** — `BuildVXLTurretMatrix` (0x00458810), `disassemble_function`:
   `0045886f LEA ECX,[ESI+0x388]; CALL 0x004c93d0 (Current)` used as the ground-plane base
   rotation, then +0x370 composed on top. `get_function_callers 0x00458810` = GetTurretDrawPosition,
   FUN_0043da80 (turret draw helpers). Turret world orientation = hull(+0x388) ∘ turret(+0x370).
3. **Facing_Update** — `UnitClass::Facing_Update` (0x00736990), `disassemble_function 0x00736990`:
   turreted units (TypeClass+0xca1) rotate **+0x3a0** toward the target and realign +0x3a0 to
   **+0x388.Current()** when idle; turretless-weapon units (TypeClass+0x67c==1) rotate **+0x388**
   itself to aim. So +0x388 is the body, +0x3a0 the turret.

### Why +0x370 is NOT the body
`search_instructions LEA operand=0x370` (whole program, 1.15M insns): the only readers/writers
are BuildingClass ctor/Sell/Mission_Missile, FUN_0043da80/0043e940, **BuildVXLTurretMatrix**,
TechnoClass ctor, **TechnoClass::Unlimbo** (0x006f6ca0), **UnitClass::Deploy**, **UnitClass::DrawPips**,
**UnitClass::Draw_Body_And_Turret** (0x0073ca98). **No locomotor / movement / Do_Turn / Process
function appears.** A facing the drive system never touches cannot be the moving hull. In the unit
draw, +0x370 is copied to a local and adjusted by weapon+0x29e (barrel logic) — turret/barrel visual.

## FacingClass primitive (verified layout & math)

16-bit DirStruct, full circle = 0x10000 (8-bit facing byte = 16-bit >> 8).
Fields (from ctor 0x004c91e0 and Current/Set below):
- +0x00 (u16) `current`  = destination/end facing
- +0x04 (u16) `prev`     = start facing (interpolation origin)
- +0x08 (i32) `start_frame` (g_CurrentFrameCounter at 0x00a8ed84; -1 = none)
- +0x10 (i32) `duration_frames`
- +0x14 (u16) `rot`      = `ROT_byte << 8`

- **SetROT** (0x004c9680), `disassemble_function`: clamp arg to ≤0x7f, store `arg<<8` at +0x14.
- **Current()** (0x004c93d0, decompiler label `RateTimer__Current`), `decompile_function`:
  returns `current - (diff/step)*remaining` where `diff = current - prev`,
  `step = abs(diff)/rot`, `remaining = duration - (frame - start)`. rot≤0 ⇒ returns current.
- **Set_Desired** (0x004c9220, decompiler label `RateTimer__Set`), `decompile_function`:
  snapshots the live Current() into `prev`, writes `current = new`, `start = g_CurrentFrameCounter`,
  and **`duration = abs(new - prev) / rot`** (integer IDIV). This is the ONLY gradual setter.
  `get_function_callers 0x004c9220` includes `DriveLocomotionClass::Do_Turn`.
  (Note: 0x004c9300 is an *instant* Set_Facing — duration 0 — used to snap turret to body at spawn.)

### Duration formula
`duration_frames = abs(delta_16bit) / (rulesROT << 8) = delta_8bit / rulesROT` (integer trunc),
where delta_8bit is the turn in 1/256-circle units (90° = 0x40, 180° = 0x80). Start = current frame.

## TypeClass+0x71c IS the rules `ROT=` field (VERIFIED)
`search_instructions MOV operand=0x71c` → `TechnoTypeClass::ReadINI` writes it at 0x00714b2f.
`disassemble_bytes 0x00714ac0..0x00714b40`: `00714b1b PUSH 0x81b164; 00714b22 CALL 0x00524ec0
(INIClass::ReadInteger, default=[+0x71c])`. `read_memory 0x0081b164` = bytes `52 4F 54 00` = **"ROT"**.

## Acceptance numbers (rules ROT = 5 for MTNK/HTNK/AMCV/SMCV/HTK, from ini/rulesmd.ini)
Body in-place turn duration = delta_8bit / 5 (trunc):
- **90° (0x40):** 16384 / 1280 = **12 binary frames**
- **180° (0x80):** 32768 / 1280 = **25 binary frames**
- (45° = 6, 22.5° = 3.)
These are the values a Rust acceptance test should assert against a body FacingClass with ROT=5.

## Verdict on FRAME_BASIS §5.2 / Handoff H-1 ("body ROT=3 @ +0x370")
**WRONG.** The doc mistook the turret/barrel-visual facing (+0x370, ROT=3) for the body.
- Body facing = **+0x388**, ROT = **rules ROT** (turned by `DriveLocomotionClass::Do_Turn`).
- The claim "vehicle turn rate is enforced by drive-track curves, not BodyFacing ROT" is also
  misleading: `Do_Turn` sets the body FacingClass to interpolate at **rulesROT**; that IS the
  hull turn rate. Drive tracks carry the facing *during translation*, but the rate is rulesROT.

## In-place vs arc gating (PARTIAL)
`DriveLocomotionClass::Process` (0x004b0500, `decompile_function`) dispatches per tick:
mid-track → `Process_Drive_Track` (0x004b0f20, carries facing along the curve); otherwise
→ `Process_Movement` (0x004b2630) which selects the drive-track index and calls `Do_Turn`
(body +0x388 Set_Desired). gamemd does NOT rotate a strict 100% in place then translate — it
selects a track that turns the hull *while* moving for moderate turns. The exact threshold lives
in `Process_Movement` track-selection (not fully decoded here). For the body-facing model this
is orthogonal: the hull orientation is always the +0x388 FacingClass at rulesROT.

## Rust handoff — `handle_vehicle_rotation` (src/sim/movement/movement_step.rs:222)
- **DRIFT:** it advances a raw `u8` facing by `rot_to_facing_delta(rot, tick_ms)` — a
  millisecond-integrated per-tick delta. Replace with the frame-based **body FacingClass**.
- The existing `src/sim/movement/facing_class.rs` already models the primitive EXACTLY
  (Set: `duration = abs(diff)/(rot<<8)`, Start = frame; Current interpolation). Use it for the body.
- Feed it the unit's **rules ROT** (the same `ROT=` at TypeClass+0x71c → its own u8), NOT 3.
- Convert: body FacingClass is 16-bit; render/logic u8 facing = `current(frame) >> 8`; Set target
  = `move_dir_facing_u8 << 8`. Set_Desired once when the target changes; read `current(frame)` each tick.
- Drop `rot_to_facing_delta` / `tick_ms` from the body path entirely (ms-integration is the drift).
- Gating: keeping "turn then move" is an acceptable approximation for now; for exact arc parity,
  decode `Process_Movement` (0x004b2630) track-selection + `Process_Drive_Track` (0x004b0f20).
