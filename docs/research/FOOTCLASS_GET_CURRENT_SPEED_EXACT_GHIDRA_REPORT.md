# FootClass::GetCurrentSpeed Exact Mechanism — Ghidra Report

- **Date:** 2026-07-20
- **Binary:** retail Yuri's Revenge `gamemd.exe`, image base `0x00400000`, SHA-256 `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`
- **Primary target:** `FootClass::GetCurrentSpeed @ 0x004DB1A0`
- **Investigation mode:** exhaustive-slice
- **Status:** **VERIFIED static mechanism within the stated scope**; derived fixtures are not an executable native oracle
- **Active in YR:** Yes on the ordinary Unit/Drive path; the base helper is also reached by the Infantry movement-speed wrapper. The final flag-carrier branch is conditional YR behavior and is off in the stock/default multiplayer rules.

## Verdict

The exact `GetCurrentSpeed` calculation is now closed for stock AMCV/MTNK-class Units. The native helper does not use one fused “base speed times all modifiers” expression. It performs two mandatory signed-64 x87-to-integer conversions plus one conditional conversion, consumes the low 32 bits after every conversion that is reached, and therefore exposes these stage boundaries:

1. mandatory, after `type speed * owner SpeedUnitsMult * per-Foot speed-crate multiplier`;
2. conditional, after `FASTER * VeteranSpeed`;
3. mandatory, after `current speed fraction`.

For Unit objects only, a live CTF flag-carrier state then divides the third integer by two with signed truncation toward zero. Mission, health, docking, difficulty `Groundspeed`, and `GameSpeedBias` are not direct inputs. Terrain, slope, health, braking, and acceleration can affect the upstream current-speed fraction, but the helper itself consumes only the already-stored double at `Foot+0x578`.

This closes the **GetCurrentSpeed half** of Checkpoint B. It does **not** close Checkpoint B as a whole: RawTrack metadata/initializer reconciliation is still absent, and production activation remains blocked on the complete atomic ground population/effect ownership and an executable native oracle.

This investigation changed no Rust source, production behavior, Ghidra labels/comments, staging, or commits; its sole write is this report.

## Prior-State / Duplication Check

No exact report named `FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md` existed at investigation start. This report extends and corrects these partial artifacts rather than treating their unresolved labels as authority:

- [DRIVE_RULES_FIELDS_SPEED_INPUTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_RULES_FIELDS_SPEED_INPUTS_GHIDRA_REPORT.md);
- [DRIVE_PROCESS_DRIVE_TRACK_SPEED_BUDGET_RESIDUAL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_DRIVE_TRACK_SPEED_BUDGET_RESIDUAL_GHIDRA_REPORT.md);
- `DRIVE_ACCELERATES_TRUE_FALSE_SPEED_RAMP_GHIDRA_REPORT.md`;
- `CRATE_SYSTEM_GHIDRA_REPORT.md`;
- [VETERANCY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VETERANCY_SYSTEM_GHIDRA_REPORT.md);
- [UNITCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNITCLASS_GHIDRA_REPORT.md) and [UNIT_DRAW_EXTRAS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UNIT_DRAW_EXTRAS_REPORT.md);
- [COUNTRY_MULTIPLIERS_APPLICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_MULTIPLIERS_APPLICATION.md);
- [timing/movement-speed-turn-rate.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/movement-speed-turn-rate.md).

The repo-local research index was rebuilt before the deep pass. The prior reports were used for navigation, then every load-bearing claim below was re-read from the active binary.

## Scope

In scope:

- the complete body of `0x004DB1A0` and every direct callee;
- active Unit vtable binding for slots `+0x2C`, `+0x84`, `+0x38C`, `+0x538`, and `+0x544`;
- raw `Speed=` parser width, clamp, default, scaling, and AMCV/MTNK values;
- owner-house speed multiplier selection and INI provenance;
- `Foot+0x580` default and live speed-crate writer;
- `FASTER`, veteran/elite thresholds, and `VeteranSpeed` precision;
- `Foot+0x578` type, default, setter, clamp, and caller order;
- Unit `+0x6CC`, its CTF lifecycle, and signed half-speed branch;
- x87 control-word and `Math__ftol` semantics;
- normal versus same-Process retry behavior through the fresh-speed mask;
- derived stock AMCV and MTNK fixtures;
- current Rust disparity and a bounded implementation handoff.

Out of scope:

- RawTrack metadata and initializer roles;
- the full Drive point body, collision, pathfinding, and track chaining contract;
- full producer audits for terrain/slope/health target fraction, already covered by the Drive speed-input/ramp reports;
- non-Drive locomotor algorithms;
- the complete CTF house/cell/UI state machine;
- full save/load restoration;
- a runtime capture from the retail executable;
- Rust implementation or production activation.

## Evidence Conventions

- **VERIFIED** means read from the active binary body, assembly, xrefs, COL/vtable bytes, repo INI, or current Rust source.
- **DERIVED** means arithmetic walked from verified inputs. It is not a captured native output.
- **DEFERRED** means explicitly outside this slice and assigned a next owner.
- Ghidra labels are navigation hints only. Class identities used below were proved through COL/type-descriptor walks or concrete bodies.

## 1. Exact Formula

For normal active YR startup, define `ftol_low32(x)` as:

1. evaluate `x` on x87 under the active/cached YR control word;
2. `FISTP qword` to a signed 64-bit integer with round-toward-zero;
3. consume the low dword returned in `EAX` as the next signed 32-bit stage value.

Then `FootClass::GetCurrentSpeed(this)` is:

```text
type        = this.vtable[+0x84](this)
house_mult  = this.house(+0x21C).GetSpeedBonus(type)   // UnitType -> HouseType+0x12C f32
raw_speed   = this.vtable[+0x38C](this)                // Unit -> TechnoType+0x678 i32
crate_mult  = this.double(+0x580)

stage1 = ftol_low32(x87(raw_speed) * house_mult * crate_mult)

if this.HasWeaponAbility(0 /* FASTER */):
    stage2 = ftol_low32(x87(stage1) * Rules.double(+0x678 /* VeteranSpeed */))
else:
    stage2 = stage1

stage3 = ftol_low32(x87(stage2) * this.double(+0x578 /* current speed fraction */))

if this.vtable[+0x2C](this) == 1 /* UnitClass */
   and this.i32(+0x6CC /* flag carrier owner index */) != -1:
    return signed_divide_by_2_toward_zero(stage3)
else:
    return stage3
```

There is **no integer boundary between `house_mult` and `crate_mult`**. Both multiplications precede the first `Math__ftol`. There **is** an integer boundary before `VeteranSpeed` and another before current fraction. No clamp is performed inside `GetCurrentSpeed`.

Evidence: read-only Ghidra `decompile_function(address=0x004DB1A0, program=gamemd.exe)` and `disassemble_function(address=0x004DB1A0, program=gamemd.exe)`. The function body is `0x004DB1A0..0x004DB245`; the direct callees are `HouseClass__GetSpeedBonus @ 0x0050C050`, `Math__ftol @ 0x007C5F00`, and `TechnoClass__HasWeaponAbility @ 0x0070D0D0`.

### 1.1 Instruction-order ledger

| Address | Operation | Width / consequence |
|---|---|---|
| `0x004DB1A9` | owner virtual `+0x84` | returns type pointer in `EAX` |
| `0x004DB1AF` | read owner house at `+0x21C` | unguarded pointer in `ECX` |
| `0x004DB1B6` | `HouseClass::GetSpeedBonus(type)` | result in x87 `ST0`; saved as qword at `0x004DB1BF` |
| `0x004DB1C3` | owner virtual `+0x38C` | signed 32-bit type speed in `EAX` |
| `0x004DB1CD` | `FILD dword` | signed i32 -> x87 |
| `0x004DB1D1` | multiply saved house bonus qword | no conversion yet |
| `0x004DB1D5` | multiply `Foot+0x580` qword | no conversion yet |
| `0x004DB1DB` | `Math__ftol` | stage 1; caller retains `EAX` low dword |
| `0x004DB1E8` | `HasWeaponAbility(0)` | branch only; no speed write |
| `0x004DB1F1..0x004DB200` | `FILD stage1`, multiply `Rules+0x678`, `Math__ftol` | optional stage 2 |
| `0x004DB209..0x004DB213` | `FILD stage2`, multiply `Foot+0x578`, `Math__ftol` | stage 3 |
| `0x004DB21E` | owner virtual `+0x2C` | object category |
| `0x004DB221` | compare category to dword `1` | equality branch |
| `0x004DB226` | read signed dword `Unit+0x6CC` | `-1` is sentinel |
| `0x004DB233..0x004DB237` | `CDQ; SUB EAX,EDX; SAR EAX,1` | signed `/2`, truncating toward zero |

The stage-1 store is live despite misleading decompiler output. `PUSH 0` at `0x004DB1E0` temporarily moves `ESP`; `MOV [ESP+0x0C],EAX` at `0x004DB1E4` therefore writes the same stack address later read as `[ESP+0x08]` after `HasWeaponAbility` returns with `RET 4`. This stack alias preserves the first integer boundary on both the FASTER and non-FASTER branches.

## 2. Active Unit Binding and Type Identity

`Process_Drive_Track` does not call a label by name. It calls its owner object's vtable slot `+0x538`. The active Unit binding was proved from bytes:

- Unit vtable base: `0x007F5C70`.
- vtable `-4` at `0x007F5C6C` contains COL pointer `0x0080CC68`.
- COL `+0x0C` contains TypeDescriptor pointer `0x00842D80`.
- TypeDescriptor `+8` is `.?AVUnitClass@@`.
- Unit slot `+0x538` at `0x007F61A8` contains `0x004DB1A0`.
- Unit slot `+0x544` at `0x007F61B4` contains `0x004D3710`.
- Unit slot `+0x38C` at `0x007F5FFC` contains `0x0070EFE0`.
- Unit slot `+0x2C` at `0x007F5C9C` contains `0x00746E20`, whose body is `MOV EAX,1; RET`.
- Unit slot `+0x84` at `0x007F5CF4` contains trampoline `0x006F3270`; that jumps to slot `+0x88`, whose Unit target `0x00741490` returns `Unit+0x6C4`, the UnitType pointer.

Evidence: read-only Ghidra `read_memory` at `0x007F5C6C`, `0x0080CC68`, `0x00842D80`, `0x007F61A8`, `0x007F5FFC`, `0x007F5C9C`, and `0x007F5CF4`; `disassemble_function(0x00746E20)`, `disassemble_function(0x006F3270)`, and `disassemble_function(0x00741490)`.

The separate type-category value used by `HouseClass::GetSpeedBonus` was also proved:

- UnitType vtable base: `0x007F6218`.
- vtable `-4` -> COL `0x0080CD28` -> TypeDescriptor `0x00845980` -> `.?AVUnitTypeClass@@`.
- UnitType slot `+0x2C` at `0x007F6244` contains `0x00748170`.
- bytes at `0x00748170` are `MOV EAX,0x28; RET`.

Therefore `0x28` is positively identified as **UnitType**, not an inferred vehicle/aircraft label.

## 3. Raw `Speed=` Input

### 3.1 Concrete reader

Unit slot `+0x38C` targets `TechnoClass__GetTypeSpeed @ 0x0070EFE0`. Its complete body calls owner slot `+0x84`, returns dword `type+0x678` when non-null, and returns zero only if that second type lookup is null.

The apparent null fallback does not make a null type safe for `GetCurrentSpeed`: the earlier house-bonus call already invokes the type's vtable `+0x2C` without a null guard.

Evidence: read-only Ghidra `decompile_function(0x0070EFE0)` and `disassemble_function(0x0070EFE0)`.

### 3.2 Parser, default, signedness, and scale

`TechnoTypeClass__Constructor @ 0x00710AF0` initializes dword `+0x678` to zero at `0x007110DA`. `TechnoTypeClass__ReadINI @ 0x00712170`, slice `0x0071464A..0x00714699`, reads integer key `Speed` with default `-1`:

- exactly `-1`: preserve the existing dword;
- every other signed value `<= 0`: use zero;
- signed value `>= 100`: use `100`;
- otherwise keep the value;
- compute integer `(value << 8) / 100` after the clamp;
- cap result `>= 255` to `255`;
- write the full dword to `+0x678`.

The division uses the signed reciprocal sequence with `0x51EB851F`; because the input has already been clamped nonnegative, the result is floor division.

| INI integer | Stored `TechnoType+0x678` |
|---:|---:|
| `-1` | preserve existing value |
| `-2` | `0` |
| `0` | `0` |
| `1` | `2` |
| `4` | `10` |
| `7` | `17` |
| `99` | `253` |
| `100` or greater | `255` |

Evidence: read-only Ghidra `get_assembly_context(xref_sources=0x00710AF0,0x007110DA,0x0071464C,0x00714699)` plus decompile of `TechnoTypeClass__ReadINI`.

Stock effective YR data:

- `[AMCV] Speed=4` -> stored native integer `10` (`rulesmd.ini:6969..7007`, key at line 6980).
- `[MTNK] Speed=7` -> stored native integer `17` (`rulesmd.ini:6603..6643`, key at line 6618).

These integers are the native per-call speed input before modifiers. They are not the Rust “leptons per second” representation.

## 4. Owner-House Speed Bonus

`HouseClass::GetSpeedBonus @ 0x0050C050` calls the supplied type's vtable `+0x2C` and selects:

| Type-category result | Returned value |
|---:|---|
| `3` | `float HouseType+0x130` |
| `0x10` | `float HouseType+0x128` |
| `0x28` | `float HouseType+0x12C` |
| other | constant `1.0f` |

For AMCV and MTNK, the proved UnitType result is `0x28`, so the active value is `HouseClass+0x34 -> HouseType+0x12C`.

`HouseTypeClass__ReadINI @ 0x00511850` identifies the fields:

- `+0x128`: `SpeedInfantryMult`;
- `+0x12C`: `SpeedUnitsMult`;
- `+0x130`: `SpeedAircraftMult`.

The parser reads via `CCINIClass::ReadDouble` and stores each as **binary32 float**. `HouseTypeClass__Constructor @ 0x005113F0` initializes the float range containing `+0x12C` to exact `1.0f` (`0x3F800000`). Stock YR country/house-type sections contain no active `SpeedUnitsMult`; the lone `;SpeedUnitsMult=1.15` at `rulesmd.ini:3299` is commented. Therefore stock AMCV and MTNK use exact house multiplier `1.0f`.

`Groundspeed` is a different HouseType double at `+0xD0`. It is not read by `GetSpeedBonus` for UnitType. Difficulty-section `Groundspeed=1.0` and `[General] GameSpeedBias=1.6` likewise have no read or callee path in `GetCurrentSpeed`.

Evidence: read-only Ghidra `decompile_function(0x0050C050)`, `disassemble_function(0x0050C050)`, `decompile_function(0x005113F0)`, and `decompile_function(0x00511850)`; direct repo INI reads.

### 4.1 Precision

`GetSpeedBonus` loads `HouseType+0x12C` with `FLD float`. `GetCurrentSpeed` stores that x87 result to a qword stack temporary at `0x004DB1BF`, then reloads it as a qword multiplier. Every binary32 value is exactly representable as binary64, so this promotion loses no additional bits.

There is no house or type null guard in the active prefix. A null `Foot+0x21C` house pointer or null first type result faults; live game construction invariants must provide both.

## 5. `Foot+0x580`: Per-Foot Speed-Crate Multiplier

`FootClass__Constructor @ 0x004D31E0` zeroes `EBX`, writes the low dword of `+0x580` from `EBX` at `0x004D3292`, and writes high dword `0x3FF00000` at `0x004D329B`. The exact default double is `1.0`.

The live speed-crate dispatch path identifies this as a per-Foot multiplier, not a slope cache:

- `CrateClass::PickupDispatch @ 0x00481A00` owns the active switch even though Ghidra's nominal function boundary stops early; jump-table index `10` reaches the Speed case at `0x00482F36`, and key-table slot `10` points to `Speed`;
- both collector selection and the candidate write path require the existing qword to be exactly `1.0` by its two dwords and exclude object category `2`, positively identified through the AircraftClass COL/vtable and `WhatAmI -> 2` body;
- `0x0048305C..0x0048306C` loads the parsed factor as a qword, multiplies it by the existing `Foot+0x580` qword, and stores a qword, with no clamp or binary32 narrowing;
- the house pointer is loaded before the store, but the notification-gating byte at `House+0x1ED` is read only after the store and cannot gate it;
- `GetCurrentSpeed` reads the double on every call at `0x004DB1D5`, so the modifier is immediately part of the next reached speed calculation.

`RulesClass::ReadPowerups` parses token four of every powerup row into the binary64 multiplier table. Non-percent spelling is parsed directly as a double; percent spelling additionally multiplies by binary64 `0.01`. There is no binary32 intermediate or clamp. Stock `rulesmd.ini:30351` is `Speed=10,SPEED,yes,1.2`, so the statically expected nearest-binary64 factor is bits `0x3FF3333333333333` (approximately `1.1999999999999999556`). This bit pattern is parser/IEEE-derived, not a live-process memory capture.

The exact-`1.0` eligibility makes the stock boost effectively one-shot per Foot: after a factor of `1.2`, a later Speed pickup is converted to Money for that collector, while an already-boosted nearby candidate is skipped. A configured factor of exact `1.0` leaves the object eligible; zero, negative, or NaN is written once and then fails the exact-`1.0` test. The speed crate is conditional gameplay state; ordinary uncrated AMCV and MTNK remain at the constructor value `1.0`.

A full-program static direct-displacement scan found no other ordinary runtime writer/reset besides regular-constructor initialization and the Speed-case store. The load constructor does not initialize `+0x580`; indirect serialization or bulk restoration is not excluded, so exact per-Foot save/load restoration remains deferred.

`FootClass__ComputeChecksum @ 0x004DBAD0` feeds both dwords of `+0x580/+0x584` into its checksum in the `0x004DBB9A..0x004DBBAA` slice, proving it is deterministic object state rather than a disposable local cache. `FootClass::Get_Slope_Speed_Factor @ 0x004DC760` reads `Foot+0x530`, not `+0x580`, independently rejecting the stale slope-cache label.

Evidence: read-only Ghidra `get_assembly_context(0x004D31E0,0x004D3292,0x004D329B)`, crate dispatch/writer inspection around `0x00481CE1..0x00481D05` and `0x00482F36..0x0048306C`, `RulesClass::ReadPowerups @ 0x00673E80`, full-program direct-displacement scans for `0x580/0x584`, and decompile/disassembly of `0x004DBAD0` and `0x004DC760`.

## 6. `FASTER`, Veterancy, and `VeteranSpeed`

### 6.1 Ability index and inheritance

`GetCurrentSpeed` pushes integer zero into `TechnoClass__HasWeaponAbility @ 0x0070D0D0`. The ability-name pointer table at `0x008463B8` begins with pointer `0x008464A4`, whose bytes spell `FASTER`. `AbilityClass__FindAbilityByName @ 0x0074FEF0` starts at this table and returns its zero-based index. Therefore the queried ability is positively identified as `FASTER`.

For ability index `i`, `HasWeaponAbility` behaves as follows:

- rookie: false;
- veteran: true only if byte `type+0x29C+i` is nonzero;
- elite: true if either veteran byte `type+0x29C+i` or elite byte `type+0x2AE+i` is nonzero.

An elite therefore inherits a veteran `FASTER` bit even if `EliteAbilities` omits `FASTER`.

Evidence: read-only Ghidra `decompile_function(0x0070D0D0)`, `disassemble_function(0x0070D0D0)`, `read_memory(0x008463B8)`, `read_memory(0x008464A4)`, and `decompile_function(0x0074FEF0)`.

### 6.2 Rank thresholds

The rank value is binary32 at `Techno+0x150`:

- `VeterancyClass::IsVeteran @ 0x0074FF90`: `1.0f <= value < 2.0f`;
- `VeterancyClass::IsElite @ 0x00750010`: `value >= 2.0f`.

Exactly `1.0` is veteran; exactly `2.0` is elite. NaN is false for both. Positive infinity is elite.

Evidence: read-only Ghidra decompile and disassembly of `0x0074FF90` and `0x00750010`.

### 6.3 `VeteranSpeed` storage precision

`RulesClass__Constructor @ 0x00665650` writes exact double `1.0` to `Rules+0x678/+0x67C` at `0x00665F62..0x00665F68`.

`RulesClass__ReadGeneral @ 0x0066D530`, slice `0x0066EEDC..0x0066EEFC`, reads key `VeteranSpeed` using the current double as default and stores a full double back to `+0x678`.

The routine named `CCINIClass::ReadDouble @ 0x005283D0` parses through a `%f` binary32 path and then promotes the binary32 result to binary64. For stock `VeteranSpeed=1.2` at `rulesmd.ini:17`, the statically derived stored value is:

```text
binary32 source bits:  0x3F99999A
promoted double bits: 0x3FF3333340000000
numeric value:        1.2000000476837158203125
```

This bit pattern is statically derived from the verified parser chain, not a live-process memory capture.

Evidence: read-only Ghidra inspection of `0x00665650`, `0x0066EEDC..0x0066EEFC`, `0x005283D0`, and internal `%f` assignment chain `0x007CA530 -> 0x007D170D -> 0x007CEBE8 -> 0x007D7D1C -> 0x007D7AAF`.

### 6.4 Stock applicability

- AMCV has `Trainable=no` and no `FASTER` ability list; stock AMCV does not take the veteran branch.
- MTNK has `VeteranAbilities=STRONGER,FIREPOWER,SIGHT,FASTER` and elite abilities that omit `FASTER`. Veteran MTNK takes the branch; elite MTNK also takes it through inherited veteran bytes.

Evidence: `rulesmd.ini:6641..6643` and `rulesmd.ini:7007`, plus the verified `HasWeaponAbility` OR behavior.

## 7. `Foot+0x578`: Current Speed Fraction

`FootClass__Constructor @ 0x004D31E0` initializes qword `+0x578/+0x57C` to exact zero at `0x004D327F` and `0x004D328B`.

`TechnoClass::SetSpeedFraction @ 0x004D3710` consumes a qword argument and writes a qword:

- input `>= 1.0`: store exact `1.0`;
- input `<= 0.0`: store exact `0.0`;
- ordered input strictly between: store the original 64 bits;
- NaN: take the unordered second-compare path and store zero;
- `+infinity`: store one;
- `-infinity`: store zero.

Unit vtable slot `+0x544` binds to this setter. `Process_Drive_Track` calls the setter before vtable slot `+0x538` (`GetCurrentSpeed`) and, for Unit linked-list members at `Unit+0x6C8`, propagates the resulting fraction through their slot `+0x544` before the speed query.

`FootClass__ComputeChecksum @ 0x004DBAD0` feeds both dwords of `+0x578/+0x57C` into its checksum at `0x004DBB85..0x004DBB95`; the next instruction at `0x004DBB9A` begins the separate `+0x580/+0x584` pair.

Evidence: read-only Ghidra decompile/disassembly of `0x004D3710`, constructor assembly around `0x004D327F`, Unit vtable bytes at `0x007F61B4`, `Process_Drive_Track` assembly `0x004B1211..0x004B1274`, and `decompile_function(0x004DBAD0)`.

### 7.1 Direct versus upstream modifiers

The helper does not read a cell, LandType, slope, health, mission, docking target, EMP, Iron Curtain, or difficulty structure. The existing verified Drive speed-input/ramp reports establish that terrain/slope/health and acceleration/braking affect the target/current fraction upstream. At the `GetCurrentSpeed` boundary, all such influence is represented only by the exact qword at `+0x578`.

This distinction matters for Rust: applying terrain or health again inside the final helper would double-count; omitting their upstream fraction producer would also drift.

## 8. Unit `+0x6CC`: Conditional CTF Half Speed

### 8.1 Identity and lifecycle

`UnitClass__Constructor @ 0x007353C0` sets `EDI=-1` at `0x007353DA` and writes it to `Unit+0x6CC` at `0x007353F2`.

Verified Unit-receiver writers and clearers:

- `UnitClass__AttachFlag @ 0x00740DF0`: rejects argument `-1` or an already non-sentinel field, then writes the supplied index at `0x00740E02`;
- `UnitClass__DetachFlag @ 0x00740E20`: writes `-1` at `0x00740E2D` when attached;
- Unit destructor clears it at `0x00735884` after the flag-drop path;
- `UnitClass__Limbo @ 0x007440B0` restores/drops the flag to a valid cell and clears it at `0x007440EB`;
- the Unit override of the convoy/checksum-state routine at `0x00744640` calls the Foot checksum owner and then consumes this field at `0x00744694`.

`UnitClass__DrawExtras @ 0x0073CEC0` reads `+0x6CC`, uses non-`-1` values to select a house palette, and draws `FLAGFLY.SHP`. This positively identifies the state as the flag carrier/owner index. It is not docking state and not a dead padding field.

Evidence: read-only Ghidra global instruction search for operand `0x6CC`, then decompile/disassembly of `0x007353C0`, `0x00740DF0`, `0x00740E20`, `0x007440B0`, `0x0073CEC0`, and `0x00744640`.

### 8.2 Active-YR qualification

The branch is conditional but live YR behavior:

- `CaptureTheFlag` is parsed into `Rules+0x14B2` at `0x006720E9`;
- game/session setup propagates it into the multiplayer/scenario bit path;
- `ScenarioClass__Post_Map_Init @ 0x00686890` reaches `Generate_Random_Units @ 0x006886B0`;
- when the CTF bit and start-unit branch are active, `0x00688BF2..0x00688C02` reaches the flag-carrier assignment helper and then `AttachFlag`.

Stock effective `rulesmd.ini:3035` is `CaptureTheFlag=no`, so normal/default stock AMCV and MTNK have `+0x6CC=-1`. This is not a TS-only branch; it is an off-by-default YR multiplayer option.

### 8.3 Divide semantics

For Unit category `1`, any dword other than `-1` triggers:

```asm
CDQ
SUB EAX, EDX
SAR EAX, 1
```

That is signed division by two toward zero: `17 -> 8`, `-17 -> -8`. The divide occurs after the current-fraction integer conversion.

## 9. `Math__ftol` and x87 Semantics

`Math__ftol @ 0x007C5F00` is not an `i32` conversion helper:

- save current x87 control word at `0x007C5F03`;
- compare its low 16 bits with cached word `0x00822D80` at `0x007C5F13`;
- if different, load the cached word with `FLDCW` at `0x007C5F2F`;
- perform `FISTP qword` at `0x007C5F1B` or `0x007C5F32`;
- return high dword in `EDX` and low dword in `EAX`;
- do not restore the caller's former control word.

Normal YR startup establishes the cached/ambient policy:

- `WinMain @ 0x006BBFB7..0x006BBFC1` requests abstract rounding `0x300`;
- the runtime maps that to x87 `RC=0x0C00`, round toward zero;
- `WinMain @ 0x006BBFC9` calls `0x007C5EE4`, which caches the active word;
- file-image word `0x0E7F` has exceptions masked, 53-bit precision, and round-toward-zero; startup preserves that chop setting.

For finite normal gameplay values within signed-32 range, `ftol_low32(x)` is numerically truncation toward zero. Exact edge behavior is wider:

- valid signed-64 results outside signed-32 range are reduced to their low 32 bits by this caller, not saturated;
- with invalid-operation masked, NaN, infinity, or signed-64 overflow yields x87 integer-indefinite `0x8000000000000000`, so this caller observes `EAX=0`;
- every call pops `ST0`;
- a call that finds a different ambient control word changes it to the cached word and leaves it changed.

The first stage's multiplications occur before the first `Math__ftol` can repair a deliberately altered ambient control word. Under normal YR startup they execute under the verified 53-bit/chop word. A parity implementation must model that normal control policy or prove equivalence over its accepted input space; ordinary Rust `f64` round-to-nearest plus `as i32` is not an automatic proof.

The integer-indefinite edge result is derived from the verified `FISTP qword` opcode plus the verified masked-invalid control word and x87 ISA semantics; no corrupt-input runtime trace was captured.

Evidence: read-only Ghidra `decompile_function(0x007C5F00)`, `disassemble_function(0x007C5F00)`, and assembly contexts at `0x006BBFB7`, `0x006BBFC9`, `0x007C5EE4`, and the runtime rounding mapper.

## 10. DriveTrack Call Order and Same-Process Retry

The `Process_Drive_Track` retry flag has no pre-speed shortcut. Its only read in the 1,655-instruction function is at `0x004B127A`, after the current-fraction update/propagation and after owner slot `+0x538` at `0x004B1274`.

Three call-flow cases share two direct call instructions:

1. Existing active track: `0x004B0573` pushes zero, then `0x004B0576` calls `Process_Drive_Track`.
2. Same-Process chain after that track completes: after `Process_Movement @ 0x004B0647`, `0x004B0665` pushes one and jumps to `0x004B0AA8`, entering the shared call at `0x004B0AAA` after its normal zero-push instruction.
3. Fresh no-active-track path: after `Process_Movement @ 0x004B0A79`, `0x004B0AA7` pushes `EBX` (verified zero), then `0x004B0AAA` calls normally.

Every reached call that passes the function's initial guards repeats:

- type and `Accelerates` reads;
- current-fraction snap or ramp/brake work;
- `SetSpeedFraction` write;
- Unit linked-member fraction propagation when applicable;
- the complete `GetCurrentSpeed` virtual/read/conversion chain.

Only then does `0x004B127A..0x004B128D` read the low byte of the argument and mask the fresh integer:

```text
fresh_contribution = u8(retry_argument) != 0 ? 0 : GetCurrentSpeed_result
budget = fresh_contribution + DriveLocomotion.i32(+0x4C residual)
```

All verified callers supply canonical `0` or `1`, so the low-byte distinction does not alter these active paths. Thus a same-Process retry can advance current fraction again and repeats all speed reads/conversions, but it contributes only residual to the point budget. `Math__ftol`'s x87-control side effect also repeats. The helper itself writes no Foot speed field.

Evidence: read-only Ghidra xrefs to `0x004B0F20`, assembly contexts at `0x004B0576`, `0x004B0647..0x004B0667`, `0x004B0A79..0x004B0AAA`, and `0x004B1211..0x004B129A`; function-scoped instruction search showing the sole `0x104` stack-argument read at `0x004B127A`.

## 11. Derived Stock Fixtures

These are **DERIVED**, not runtime captures. They isolate the now-verified helper contract using a full/steady current fraction of `1.0`, no CTF flag unless named, and the stock/default owner multiplier `1.0`.

The stored stock `VeteranSpeed` is `V = 1.2000000476837158203125`. The parser/IEEE-derived expected stock Speed-crate factor is binary64 `C = 0x3FF3333333333333`, approximately `1.1999999999999999556`; unlike `V`, this expected bit pattern was not independently sampled from live process memory.

| Fixture | Raw parsed speed | House | Crate | FASTER | Fraction | Flag index | Stage 1 | Stage 2 | Stage 3 / result |
|---|---:|---:|---:|---|---:|---:|---:|---:|---:|
| AMCV, uncrated, steady full speed | `10` | `1.0f` | `1.0` | no | `1.0` | `-1` | `10` | `10` | `10` |
| MTNK rookie, uncrated | `17` | `1.0f` | `1.0` | no | `1.0` | `-1` | `17` | `17` | `17` |
| MTNK veteran | `17` | `1.0f` | `1.0` | yes | `1.0` | `-1` | `17` | `ftol(17*V)=20` | `20` |
| MTNK elite with stock lists | `17` | `1.0f` | `1.0` | yes, inherited | `1.0` | `-1` | `17` | `20` | `20` |
| MTNK rookie with stock Speed crate | `17` | `1.0f` | `C` | no | `1.0` | `-1` | `ftol(17*C)=20` | `20` | `20` |
| MTNK veteran with stock Speed crate | `17` | `1.0f` | `C` | yes | `1.0` | `-1` | `20` | `ftol(20*V)=24` | `24` |
| MTNK rookie carrying CTF flag | `17` | `1.0f` | `1.0` | no | `1.0` | non-`-1` | `17` | `17` | `17 / 2 = 8` |

AMCV has `Accelerates=true` by default, so `fraction=1.0` is a steady/full-speed helper fixture, not its first movement invocation. MTNK explicitly has `Accelerates=false`; on a full target it snaps the current fraction to `1.0` before the same call's speed query.

For arbitrary already-stored fraction `f` under ordinary no-crate/no-flag stock state:

- AMCV: `ftol_low32(10 * f)`;
- rookie MTNK: `ftol_low32(17 * f)`;
- veteran/elite-FASTER MTNK: `ftol_low32(20 * f)`.

## 12. Attractive Wrong Models Rejected

| Wrong model | Binary result |
|---|---|
| “Vehicle house bonus is always 1.0.” | False for the mechanism. UnitType selects `SpeedUnitsMult`; stock happens to default to `1.0`. |
| “Difficulty `Groundspeed` is the vehicle owner multiplier.” | False. UnitType selects HouseType `+0x12C`, not `Groundspeed +0xD0`. |
| “`GameSpeedBias` participates in GetCurrentSpeed.” | False. No read/callee path exists in the helper. |
| “`Foot+0x580` is a slope cache.” | False. Constructor default and the live Speed-crate writer identify a per-Foot crate multiplier. |
| “House and crate each truncate separately.” | False. Both multiply before stage-1 `Math__ftol`. |
| “Veterancy and fraction can be fused into one multiply.” | False. A mandatory integer boundary follows `VeteranSpeed`. |
| “Elite uses only `EliteAbilities`.” | False. Elite `HasWeaponAbility` ORs veteran and elite bytes. |
| “Unit `+0x6CC` is docking/dead TS state.” | False. It is a live conditional YR CTF flag-carrier index. |
| “Retry skips or reuses GetCurrentSpeed.” | False. It repeats the full pre-budget speed path and only masks the returned fresh integer afterward. |
| “`Math__ftol` is an ordinary saturating i32 cast.” | False. It is chop-mode `FISTP qword`; this caller consumes low 32 bits. |

## 13. Adversarial / Boundary Cases

1. **Parser sentinels:** `Speed=-1` preserves prior storage; `Speed=-2` clamps to zero. Treating every negative value as “missing” drifts.
2. **Maximum parser boundary:** `Speed=99 -> 253`, while `Speed=100 -> 255`; the mathematical `256` is capped.
3. **Sequential conversion:** a modded pre-conversion stage-1 value of `18.9` becomes integer `18`; stock `VeteranSpeed` then produces `ftol(18*V)=21`. Fusing the raw value instead would produce `ftol(18.9*V)=22`, so it is not equivalent.
4. **Elite inheritance:** stock MTNK remains `FASTER` at elite rank even though its `EliteAbilities` line omits the token.
5. **Fraction NaN:** the verified setter stores zero, not NaN. Corrupt direct memory that bypasses the setter instead reaches x87 invalid conversion and this caller observes low dword zero.
6. **Signed half:** a corrupt/modded negative stage `-17` with flag state becomes `-8`, not `-9`.
7. **Wide conversion:** a valid truncated signed-64 value outside signed-32 range wraps through its low dword; it does not saturate to `i32::MIN/MAX`.
8. **Null invariant:** the early house/type chain faults on null before `GetTypeSpeed`'s later null fallback can help.
9. **Retry:** the second call can mutate current fraction again even though its freshly computed integer is discarded from budget.
10. **CTF default:** off-by-default is not dead code. A production model that deletes the branch drifts when the YR CTF option is enabled.

## 14. Current Rust Disparity

Current source was read during the investigation; concurrent repository commits did not touch the speed-path conclusions below. The separate test-only host harness modification in `src/sim/world/techno_ai.rs` was left untouched.

Current Rust does preserve the common stock base numbers through a different representation:

- `ra2_speed_to_leptons_per_second` computes the scaled native integer then multiplies by `15`;
- AMCV raw `4` becomes `150` leptons/s and later `/15` gives `10`;
- MTNK raw `7` becomes `255` leptons/s and later `/15` gives `17`.

That sampled agreement is a regression property, not exact-mechanism parity. The active Drive path does not carry or stage:

- native `GetCurrentSpeed`'s no-clamp result contract; command resolution currently imposes a `max(25)` floor on the speed scalar;
- owner `SpeedUnitsMult` as a binary32 house/type multiplier;
- the per-Foot Speed-crate qword at the native stage;
- veteran/elite `FASTER` inheritance and the binary32-promoted `VeteranSpeed` value;
- the native integer boundary before current fraction;
- the conditional CTF signed `/2`;
- x87 53-bit/chop arithmetic and qword-conversion/low-dword behavior.

`LocomotorState::speed_multiplier` is applied during command resolution and is not evidence of equivalence to `Foot+0x580`'s persistent, checksum-visible, staged native field. `MovementTarget.current_speed` and Drive fraction logic likewise do not by themselves prove the native conversion boundaries.

Relevant Rust touchpoints for a future implementation plan:

- `src/util/fixed_math.rs::ra2_speed_to_leptons_per_second`;
- `src/sim/world/world_commands.rs::resolve_move_info`;
- `src/sim/movement/drive_locomotion.rs::update_drive_speed_fraction`;
- `src/sim/movement/movement_tick.rs` current-speed assignment;
- `src/sim/movement/movement_step.rs::drive_track_fresh_budget_from_current_speed`;
- entity/type/house/veterancy/crate/CTF state owners, which are not yet present as one verified production contract.

Verdict: **DRIFT / not implemented as the native mechanism**. Frequency and stock-default values affect priority, not the parity verdict.

## 15. Implementation Handoff

No production implementation is authorized by this report. When the remaining design gates permit a plan, the smallest truthful contract is:

1. Preserve native input storage precision:
   - type speed signed dword;
   - `SpeedUnitsMult` binary32;
   - crate multiplier/current fraction/VeteranSpeed qwords with their verified parser provenance;
   - rank binary32 and veteran/elite ability-byte inheritance;
   - CTF sentinel signed dword.
2. Preserve the three `FISTP qword` boundaries and low-dword consumption; do not fuse stages.
3. Preserve normal startup x87 53-bit/chop semantics or provide an exhaustive equivalence proof over all accepted rule/state bit patterns.
4. Apply CTF `/2` only after stage 3 and only for object category Unit with `+0x6CC != -1`.
5. On same-Process retry, repeat the fraction update/propagation and complete speed query, then mask the fresh integer before adding residual.
6. Keep terrain/slope/health in the upstream target/current-fraction owner; do not apply them twice.
7. Do not replace per-object state with a command-time scalar unless positive proof shows identical later crate, rank, owner, CTF, save/checksum, and same-Process visibility.

Future focused acceptance tests should include:

- stock AMCV `10` and MTNK `17` at full fraction;
- MTNK rookie/veteran/elite `17/20/20`;
- stock Speed-crate rookie/veteran `20/24`;
- fraction values that distinguish stage boundaries;
- modded values that distinguish fused from sequential conversion;
- `Speed=-1`, negative, `99`, and `100` parser cases;
- setter zero/one/NaN/infinity cases;
- CTF positive and signed-negative `/2` cases;
- valid signed-64 values outside i32 and invalid x87 conversion results;
- same-Process retry trace proving repeated speed state with zero fresh contribution;
- an executable retail oracle for AMCV/MTNK state/order, not only Rust-vs-Rust goldens.

## 16. Coverage Ledger

| Target | Depth | Result |
|---|---|---|
| `FootClass::GetCurrentSpeed @ 0x004DB1A0` | DEEP | complete body, widths, order, branches, callers/callees |
| Unit / UnitType COL and vtables | DEEP | concrete identities and slots proved from bytes |
| `HouseClass::GetSpeedBonus @ 0x0050C050` | DEEP | all category branches and Unit field proved |
| `HouseTypeClass` constructor / `ReadINI` | VERIFY | defaults, keys, float storage |
| `TechnoClass::GetTypeSpeed @ 0x0070EFE0` | VERIFY | exact Unit reader |
| `TechnoTypeClass` constructor / `ReadINI` speed slice | DEEP | default, sentinel, signed clamp, scale, cap |
| `TechnoClass::HasWeaponAbility @ 0x0070D0D0` | DEEP | rookie/veteran/elite and inherited bytes |
| `VeterancyClass::IsVeteran/IsElite` | VERIFY | exact binary32 thresholds |
| `RulesClass` constructor / `ReadGeneral` | DEEP | `VeteranSpeed` default, key, parser precision |
| `Math__ftol @ 0x007C5F00` + startup CW path | DEEP | qword conversion, low/high return, chop policy, edge behavior |
| `FootClass` constructor | VERIFY | `+0x578=0.0`, `+0x580=1.0` |
| `TechnoClass::SetSpeedFraction @ 0x004D3710` | DEEP | clamp and unordered behavior |
| Speed-crate dispatch/writer | DEEP | case, eligibility, write, stock key |
| Unit CTF attach/detach/limbo/draw/checksum | DEEP | identity, writers, clearers, active gate |
| `DriveLocomotionClass::Process @ 0x004B0500` | VERIFY | normal, fresh, and retry call arguments |
| `Process_Drive_Track @ 0x004B0F20` | DEEP for pre-budget slice | repeated writes/calls and post-query mask |
| `InfantryClass::GetMovementSpeed @ 0x00521D80` | VERIFY | direct base call plus out-of-scope Infantry post-adjustment |
| current Rust speed path | VERIFY | source-level disparity map; no build needed |

## 17. Final Open-Questions Log

| ID | Status | Resolution / next owner |
|---|---|---|
| OQ-01 | RESOLVED | `0x004DB1A0..0x004DB245`, ECX receiver, signed dword result used through Unit vtable and Infantry wrapper. |
| OQ-02 | RESOLVED | Corrected wording: Drive calls the **owner Unit vtable** `+0x538`; COL and slot bytes bind it to `0x004DB1A0`. |
| OQ-03 | RESOLVED | `Foot+0x21C` is the unguarded House receiver for `0x0050C050`. |
| OQ-04 | RESOLVED | UnitType `0x28` selects `HouseType+0x12C SpeedUnitsMult`; stock default `1.0f`. |
| OQ-05 | RESOLVED | Unit `+0x38C -> 0x0070EFE0 -> TechnoType+0x678` signed dword. |
| OQ-06 | RESOLVED | Exact parser/default/clamp/scale/cap; AMCV `10`, MTNK `17`. |
| OQ-07 | RESOLVED | `FILD raw; FMUL house qword; FMUL crate qword; ftol`. |
| OQ-08 | RESOLVED | `Foot+0x580` is a checksum-visible double, default `1.0`, Speed-crate multiplier; it is not the slope factor. |
| OQ-09 | RESOLVED | Speed case qword-multiplies it by the binary64 `[Powerups]` factor; exact-`1.0` eligibility, one-shot behavior, and next-query visibility are proved. |
| OQ-10 | RESOLVED | Ability index zero is `FASTER`; veteran/elite byte behavior exact. |
| OQ-11 | RESOLVED | `Rules+0x678` is double `VeteranSpeed`; stock parser-derived bits recorded. |
| OQ-12 | RESOLVED | MTNK veteran and elite inherit FASTER; stock AMCV does not. |
| OQ-13 | RESOLVED | `Foot+0x578` is checksum-visible current-fraction double, default zero, clamped setter exact. |
| OQ-14 | RESOLVED | Unit object category is dword `1`; equality comparison exact. |
| OQ-15 | RESOLVED | `Unit+0x6CC` flag state and signed `/2` sequence exact. |
| OQ-16 | RESOLVED | Constructor, live attach/detach/drop/clear paths, CTF gate, and stock-off default proved. |
| OQ-17 | RESOLVED for helper | No internal clamp; qword/low-dword conversion and invalid/overflow behavior documented. Extreme malformed textual `atoi` before the parsed signed dword remains outside scope. |
| OQ-18 | RESOLVED | House/type nulls are not guarded by the active prefix; live construction invariant required. |
| OQ-19 | RESOLVED | No `GameSpeedBias` or difficulty `Groundspeed` input. |
| OQ-20 | RESOLVED | UnitType uses `SpeedUnitsMult`; stock relevant owner types default to `1.0f`. |
| OQ-21 | RESOLVED | Retry flag read only after repeated fraction work and `GetCurrentSpeed`; fresh integer then masked. |
| OQ-22 | RESOLVED | Direct callees and `Math__ftol` startup/control semantics exhausted. |
| OQ-23 | RESOLVED | Derived AMCV/MTNK fixtures recorded and explicitly not called an oracle. |
| OQ-24 | DEFERRED — persistence/oracle | Fraction and crate/flag state are checksum-visible. The regular constructor initializes the crate field and no other direct reset writer was found, but the load constructor omits it; exhaustive indirect save/load restoration and paused/first-tick runtime capture belong to the executable-oracle/serializer phase. |
| OQ-25 | RESOLVED | Ordinary Unit/Drive path is active YR; CTF is conditional YR and off by default, not TS legacy. |
| OQ-26 | RESOLVED — zero-add | Infantry directly calls the base helper then may post-adjust; no effect on AMCV/MTNK Unit fixtures. |
| OQ-27 | RESOLVED — zero-add | Terrain/slope/health are upstream fraction producers, not additional direct helper multipliers. |
| OQ-28 | DEFERRED — CTF UI integration | Full shipped-shell control reachability and the complete House/cell flag state machine are not needed for the proved speed branch; next owner is a dedicated CTF system investigation if requested. |

There is no silent open width, signedness, rounding, modifier-order, stock AMCV/MTNK, or retry-mask question inside this report's scope.

## 18. Exhaustive-Slice Closeout

### Zero-add pass

The post-analysis zero-add pass added two in-scope checks that were not explicit in the initial 25-question seed:

- the direct Infantry wrapper and its class-specific post-adjustment;
- separation of direct helper inputs from upstream terrain/slope/health fraction producers.

Both are resolved above. The pass also identified two explicitly deferred integration questions—full persistence/runtime capture and full CTF UI/state-machine coverage. Neither changes the exact helper contract.

### Stale-document corrections required downstream

This report supersedes these load-bearing older claims:

- [COUNTRY_MULTIPLIERS_APPLICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_MULTIPLIERS_APPLICATION.md): type code `0x28` is UnitType; Unit speed uses `SpeedUnitsMult`.
- [DRIVE_RULES_FIELDS_SPEED_INPUTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_RULES_FIELDS_SPEED_INPUTS_GHIDRA_REPORT.md) and related Drive prose: `+0x580` is not a slope cache, and it multiplies before the first integer conversion.
- older Foot/timing layouts: `+0x6CC` is not docking/dead padding.
- [timing/movement-speed-turn-rate.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/timing/movement-speed-turn-rate.md): the CTF branch has live YR writers and is not TS-only; `GameSpeedBias` is not part of this helper.
- any fused “base * house * crate * veteran * fraction” formula: intermediate integer boundaries are mandatory.

Those source documents were not patched because this `/re-investigate` slice owns one new report only. A later explicit audit/fix pass should correct them against this evidence.

### Remaining production blockers

- [DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md) is still required to finish Checkpoint B.
- Exact Phase-1 miner, Infantry/Walk, Hover, Ship, tube, forced-track, lifecycle/effect, and formation owners remain required for the approved atomic production flip.
- An executable retail native oracle remains required for parity certification.
- This report authorizes research/plan reconciliation only, not a vehicle-only production activation.
