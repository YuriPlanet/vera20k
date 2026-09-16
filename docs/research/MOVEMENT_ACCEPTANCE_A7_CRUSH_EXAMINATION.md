# A7 "Crush" — acceptance examination against `gamemd.exe`

Scope: criterion A7 of [2026-09-15-movement-retail-acceptance.md](../plans/2026-09-15-movement-retail-acceptance.md)
— "Crusher over `Crushable` infantry, sandbags and fences; victim lifecycle inside
the crusher's cell slot".

Binary: `gamemd.exe`, Ghidra project `testProsjekt`, image base `0x00400000`,
10 073 functions, executable `C:/Users/enok/Documents/Command and Conquer Red Alert II/gamemd.exe`.
Retail data: `ini/rulesmd.ini` in this worktree.
Rust: this worktree at `e933da49` (`feature/vera20k-drive-residual-budget`).

Evidence words used below:

- **Native-established** — read from the disassembly/decompilation of the named
  address in this binary, with the INI key or enum value bound to its
  `ReadINI` store site where one exists.
- **Rust-read** — read from the named source file in this worktree.
- **Parity-demonstrated** — a gamemd-derived executable comparison. **There is
  none for A7.** No oracle under `tools/spatial_oracle/` touches the crush
  predicate, and `movement_bridge_retail_tests.rs` has no crush case. Every
  "matches" row below is native-established + Rust-read, never parity.

---

## 1. The native crush surface (native-established)

### 1.1 The predicate — `TechnoClass::Is_Crushable_By 0x005F6CD0`

`RET 4`; `this` (ECX) is the **victim**, the stack argument is the **crusher**.
Two blocks; the first falls through to the second on **every** failure.

Omni block `0x005F6CD2..0x005F6D3C`:

1. crusher type `+0xD29` = `OmniCrusher=` (key string `0x0084387C`, stored by
   `TechnoTypeClass::ReadINI` at `0x00714D04`)
2. victim non-null and `victim+0x14 & 1`
3. victim type `+0xD2A` = `OmniCrushResistant=` (key `0x00843868`, stored
   `0x00714D25`) clear
4. victim `What_Am_I() != 6` (Building)
5. `HouseClass::Is_Ally 0x004F9A90` (`this` = `crusher+0x21C`, the crusher's
   owner house; arg = victim) false
6. victim vtable `+0x160` = `IsIronCurtainActive 0x0041BF40` false
   → return 1.

Ordinary block `0x005F6D3F..0x005F6D89`:

1. victim type `+0x22D` = `Crushable=` set
2. victim non-null and `victim+0x14 & 1`
3. victim `+0x2A4` (deploy crush-immunity instance byte) clear
4. not ally (same call)
5. not Iron-Curtained → return 1; otherwise 0.

**There is no class test in the ordinary block.** `InfantryTypeClass`'s
constructor writes `+0x22D = 1` at `0x005238E2`, so keyless infantry are
crushable; everything else defaults to 0.

Three callers reach the same predicate: `Can_Enter_Cell` at `0x0073FB4A` and
`0x0073FD17`, and the kill loop at `0x007417C1`. One more,
`UnitClass::What_Action_OnObject 0x0073FE7F`, drives the crush cursor.

Stock census (`ini/rulesmd.ini`): `Crushable=yes` on exactly nine sections —
`[E1] [GGI] [JUMPJET] [GHOST] [LUNR]` and the four overlays `[GASAND] [CAFNCB]
[CAFNCW] [CAFNCP]`; `Crushable=no` on 22 (17 of them infantry: CLEG, YURIPR,
DNOA, JOSH, DNOB, ALL, COW, BRUTE, BORIS, PTROOP, SHK, POLARB, DESO, CIVAN,
TANY, CAML, CCOMAND); 43 infantry types carry no key and therefore default
crushable. `Crusher=yes` on 29; `OmniCrusher=yes` on `[BFRT]` alone;
`OmniCrushResistant=yes` on DNOA, BFRT, AMCV, SMCV, PCV, SMIN;
`DeployedCrushable=no` on `[GGI]` alone. No stock vehicle or building carries
`Crushable=yes`.

### 1.2 Admission — `UnitClass::Can_Enter_Cell 0x0073F0A0`

Crush latch, inside the non-allied occupant branch, `0x0073FB2A..0x0073FB6C`:
mover type `+0xD28` = `Crusher=` (key `0x0081BB58`, stored `0x00714CE3`) or
`TechnoClass::HasWeaponAbility(0x11) 0x0070D0D0`; then `Is_Crushable_By`
(`0x0073FB4A`); then a redundant ally re-test (`0x0073FB53`); then
`byte [ESP+0x17] = 1` at `0x0073FB67` and **continue the walk with the running
code unchanged**. A crushable occupant contributes no code.

Tail `0x0073FC24..0x0073FD43`, reached after the object list is exhausted:

- `0x0073FC24` — only runs when the running code is still 0.
- `0x0073FC2C` — latch set → `0x0073FCF6`.
- `0x0073FCF6` — the cell's vehicle-occupation bit (`OccupationFlags` bit 5,
  cached at `[ESP+0x15]` from `cell+0x124`, or `cell+0x128` on the bridge
  branch) clear → fall to `0x0073FD37` and return the running code, i.e. **0**.
- otherwise `CellClass::FindFirstUnit 0x0047EBA0` at `0x0073FD02`; null →
  `0x0073FD20` → **2**; else `Is_Crushable_By` at `0x0073FD17` → true → **0**,
  false → **2**.

So **a successful crush answers 0, and 2 when a vehicle bit is set whose first
unit is not crushable**. Code 1 is a different mechanism entirely:
`0x0073FAFF..0x0073FB25` calls `FUN_0040DD20 0x0040DD20` (a Techno downcast —
it returns the object only for `What_Am_I()` in {1, 2, 6, 0xF}) and tests
`[obj+0x220] == 2`. `+0x220` is the cloak state machine:
`TechnoClass::StartUncloaking 0x007036C0` reads 1/2 there and writes 3, so
**code 1 = a non-allied *cloaked* Techno in the cell**. The A* cost table
`0x0081870C` reads `[1.0, 1000.0, 1.0, 1.0, 60.0, 20.0, 8.0, 10000.0]` — code 1
costs 1000×, code 0 costs 1×.

### 1.3 The kill — `UnitClass::Crush_Cell`, vtable `+0x534`, body `0x007416A0`

Bound by the UnitClass vtable `0x007F5C70`: the slot entry at `0x007F61A4`
(= base + 0x534) holds `0x007416A0`; that data reference is the function's only
xref. `RET 8`, two stack arguments: `(CellStruct* coord, bool scatter_only)`.
Ghidra currently labels it `UnitClass__PerCellProcess`, which is wrong — the
real `UnitClass::PerCellProcess` is `0x00739EC0`.

Three call sites, all found by scanning for `CALL dword ptr [reg + 0x534]`:

| Address | Caller | `scatter_only` |
|---|---|---|
| `0x004B3E65` | `DriveLocomotionClass::Process_Movement` | 1 (`PUSH 0x1` at `0x004B3E35`) |
| `0x006A34B4` | `ShipLocomotionClass::Process_Movement` | 1 |
| `0x0073B089` | `UnitClass::PerCellProcess` | 0 |

Body:

- `0x007416BB..0x00741731` — pick the bridge occupier list `cell+0xE8` when the
  cell carries the bridge flag `0x100` and either the crusher's `+0x8C`
  on-bridge byte is set or the crusher's own cell level equals this cell's
  level + 4; otherwise the ground list `cell+0xE4`.
- `0x00741733..0x0074174E` — gate: `Crusher=` (`+0xD28`) or ability 0x11.
- **scatter mode** `0x00741754..0x00741785` (bridge) / `0x00741788` (ground) —
  the only further test is the cell's infantry sub-cell bits
  (`cell+0x128 & 0x1F` / `cell+0x124 & 0x1F`), then
  `CellClass::Scatter_Objects 0x00481670` with `(NullCoord 0x00B1CFE8, 1,
  force = 0, alt = on-bridge)`. Crushability and the ally relation are **not**
  consulted. `Scatter_Objects`' own dispatch gate is
  `elitePrescan || force || Rules+0x17ED (PlayerScatter) ||
  HasWeaponAbility(3) || Rules+0x144C ([IQ] Scatter) <= house+0x24C (IQ)`,
  with the documented ten-entry collection cap (`FUN_0040CE50(10, 0)`).
- **crush mode** `0x00741799..0x00741964`, per occupant:
  - `Is_Crushable_By` at `0x007417C1`; false → next.
  - ally and mover type `+0xC94` clear → next (`0x007417CE..0x007417EC`).
    `+0xC94` is `IsTrain=` (key string `0x008444BC` = "IsTrain", stored
    `0x00712284`).
  - `TechnoClass::DistanceSquaredTo 0x005F6560` from the crusher's live
    `+0x9C/+0xA0` to the victim's coordinate; `>= 0x4000` → next
    (`0x00741801..0x0074180B`). The gate is therefore **128 leptons**.
  - victim `+0x8D` set → next (`0x00741811`).
  - hijack arm `0x0074181F..0x0074188D`: victim `What_Am_I() == 0xF`
    (`InfantryClass::What_Am_I 0x00523340` returns 0xF), victim type `+0xEC6` =
    `VehicleThief=` (key `0x0082593C`, stored `0x005245F5`), victim `+0x5A4`
    (NavCom) == the crusher, crusher not `IsTrain` → copy victim `+0x41A` onto
    the crusher, crusher vtable `+0xDC`(0), crusher `+0x3D4`(victim owner, 1),
    victim `+0xF8` UnInit. No sound, no kill credit.
  - **kill arm** `0x00741892..0x00741918`, in this order:
    1. save the successor `victim+0x30` first (`0x00741894`);
    2. set the "crushed something" flag; set the **rock** flag only when
       `victim->What_Am_I() == 1` (`UnitClass::What_Am_I 0x00746E20` returns 1)
       — `0x0074189E..0x007418A6`;
    3. `VocClass::PlayAt 0x007509E0` with the **victim type's** `CrushSound=`
       (`ObjectTypeClass+0x1F0`; key string `0x00832BF4` = "CrushSound", stored
       by `ObjectTypeClass::ReadINI` at `0x005F93D5`) at the **crusher's own
       coordinate** `crusher+0x9C/A0/A4` (`0x007418AA..0x007418DC`);
    4. victim `+0x170` = `TechnoClass::FreeAllMindControlCaptures 0x00710460`;
    5. victim `+0xE0` = `TechnoClass::RecordKill 0x00702D40`(crusher);
    6. victim `+0x124` = `TechnoClass::MarkCellLists 0x004D3780`(0) — unmark
       from the cell lists;
    7. victim `+0xD4` = Limbo (`InfantryClass 0x0051DF10`: releases `+0x674`,
       `+0x6E8 = 2`, clears the prone byte `+0x6DB`, `+0x6C4 = 0`, then
       `FootClass::Limbo`);
    8. victim `+0xF8` = `FootClass::UnInit 0x004DE5D0`.
    **No `Take_Damage`, no death animation, no smudge, no RNG draw.**
  - tail `0x00741925..0x00741964`: if anything was crushed, crusher `+0x45C` =
    `TechnoClass::StartUncloaking 0x007036C0`(NULL); then, only if the rock flag
    is set *and* `crusher+0x334 == 0.0f`, write `crusher+0x334 = 0xBD4CCCCD`
    = **-0.05f** (`0x00741954`).

### 1.4 The overlay collapse — `UnitClass::PerCellProcess 0x0073AFD4..0x0073B074`

`Crusher=` (`0x0073AFEB`) or ability 0x11 (`0x0073AFF9`); `cell+0x44` overlay
present (`0x0073B002`); OverlayType `+0x22D` (`Crushable=`) at `0x0073B013`,
**or** `+0x2A8` (`Wall=`) at `0x0073B01D` together with the mover type's
`+0x5B4 == 0xC` at `0x0073B02D`. Then `VocClass::PlayAt` of the overlay type's
`CrushSound=` (`+0x1F0`) at the crusher's coordinate (`0x0073B045..B04D`),
`CellClass::DestroyOverlay(-1) 0x00480CB0` at `0x0073B056`, `crusher+0x6B5 = 0`
at `0x0073B067`, and `crusher+0x334 += 0.02` (`0x007F4E38` = `0x3CA3D70A`) at
`0x0073B05B..0x0073B06E`. `PerCellProcess` then calls `Crush_Cell(cell, 0)` at
`0x0073B089`.

**`TechnoTypeClass+0x5B4` is `MovementZone=`, settled.** `TechnoTypeClass::ReadINI
0x0071605E..0x00716081` reads the key string at `0x008431C8` ("MovementZone")
through `CCINIClass::ReadMovementZone 0x00474E40`, which indexes
`g_MovementZone_NameTable` (base `0x0081BA88`, 13 entries, upper bound
`0x0081BABC`). Index 12 = `0x0081BAD0` = "CrusherAll"; index 6 = `0x0081BB08` =
"Subterannean", which matches the `CMP EAX, 0x6 / SETZ DL` immediately after the
store at `0x0071607E`. Index 0 = "Normal", index 1 = "Crusher".

`CellClass::DestroyOverlay 0x00480CB0` rejects any overlay whose `+0x2A8`
(`Wall=`) is clear, so a modded `Crushable=yes` non-wall overlay plays its
`CrushSound` every cell entry and is never removed.

### 1.5 The `IsTrain` arms (stock-unreachable)

`TechnoTypeClass+0xC94` = `IsTrain=` gates three crush-adjacent behaviours:

- allied crushing inside `Crush_Cell` (`0x007417E4`);
- the code-5-instead-of-7 escape for a weaponless mover against an enemy
  occupant (`0x0073FB82..0x0073FB90`, `0x0073FC91..0x0073FC9F`);
- the whole co-occupant obliteration loop in
  `DriveLocomotionClass::Process_Drive_Track 0x004B1864..0x004B1978` (mirrored by
  `ShipLocomotionClass::Process_Drive_Track`), which for every occupant that is
  **not** crushable by the mover calls the victim's `+0x16C` `Take_Damage` with
  10000 and warhead `Rules+0xFA8` (`0x004B191A..0x004B1941`) and then 20 on the
  mover itself (`0x004B1947..0x004B196E`).

`IsTrain=` appears **0 times** in stock `rulesmd.ini`. All three are unreachable
in stock YR; VERA modelling none of them is stock-equivalent.

Note also that `MapClass::Check_Crushable_Obstacle 0x00578AD0`, despite its
label, has nothing to do with crushing: it walks the cell's ground list looking
for a `+0x16B7` (garrisonable) building. Treat the name as a stale lead.

---

## 2. What matches

Each row is native-established on the left and Rust-read on the right. None is
parity-demonstrated.

| Behaviour | Native | Rust |
|---|---|---|
| Omni path: `OmniCrusher=` ignores `Crushable=`, blocked by `OmniCrushResistant=`, buildings, allies and Iron Curtain | `0x005F6CD2..0x005F6D3C` | `bump_crush::can_crush` (`bump_crush.rs:701`); tests `test_omni_crusher_crushes_non_crushable_infantry`, `test_omni_crusher_crushes_vehicles`, `test_omni_crush_resistant_blocks_all`, `test_structures_never_crushable`, `iron_curtained_victim_survives_an_omni_crusher` |
| Ordinary path: `Crushable=` + deploy byte + Iron Curtain, and `Crushable=` defaults yes for infantry only | `0x005F6D3F..0x005F6D89`, ctor `0x005238E2` | `can_crush`; `object_type.rs` test `crushable_defaults_yes_for_infantry_and_no_for_everything_else` |
| The deploy crush-immunity byte is `DeployedCrushable=`-driven, not prone | `+0x2A4`; the prone byte is `+0x6DB`, cleared by `InfantryClass` Limbo `0x0051DF10` | `deploy_crush_immune` (`bump_crush.rs:738`); tests `prone_infantry_are_crushed_like_standing_infantry`, `deployed_gi_is_crushable_but_deployed_guardian_gi_is_not` |
| One crush authority (`Crusher=` / `OmniCrusher=`), not `MovementZone` | `0x0073FB2A`, `0x00741733`, `0x0073AFEB` all read `+0xD28` | `CrushCapability::of` (`bump_crush.rs:557`), I9c |
| Crush radius is `DistanceSquared < 0x4000` | `0x00741801..0x0074180B` | `CRUSH_DISTANCE_SQ_LIMIT = 0x3fff`, `within_crush_distance_sq`; tests `crush_distance_gate_includes_0x3fff`, `crush_distance_gate_excludes_0x4000` |
| The kill loop saves the successor before teardown, so removing the victim cannot truncate the walk | `0x00741894` (`EBP = victim+0x30`) | `track_host.rs:1236-1241` reads `next_on_layer` before the kill |
| Victim lifecycle: sound → mind-control release → kill credit → unmark → limbo → UnInit, with no damage call, no anim, no RNG | `0x007418CE..0x00741916` | `finish_movement_pass` (`movement_tick.rs:4196-4245`), `track_host.rs:1266-1300`: `emit_crush_kill_sounds_at` → `capture_kill_credit` → `award_kill_experience` → `LifecycleRequest::Uninit { reason: Crush }` |
| The crusher earns the victim's experience | `+0xE0` `TechnoClass::RecordKill 0x00702D40`(crusher) | `award_kill_experience` at both kill sites |
| `DeathWeapon=` does not fire on a crush | `TechnoClass::Fire_Death_Weapon 0x0070D690` has only `ReceiveDamage 0x00701900` and `FlyLocomotionClass::Process 0x004CD600` as callers; `0x007416A0` enters neither | recorded NO-DIFF in `bump_crush.rs:679-690` |
| The crush sound is the **victim type's** `CrushSound=`, played at the crusher | `ObjectTypeClass+0x1F0`, `0x007418CE..0x007418DC` | `emit_crush_kill_sounds_at` resolves `rules.object(victim_type).crush_sound` and positions at the passed crusher coordinate |
| Crushable overlays fall to any `Crusher=`; the sound is the overlay's `CrushSound=`; removal is forced (`-1`), no RNG, with cardinal chain cleanup | `0x0073B013`, `0x0073B045`, `0x0073B056`, `DestroyOverlay 0x00480CB0` | `world/mod.rs:5202 apply_wall_crush_on_driveover` + `apply_wall_damage_events`; tests `crushable_fence_falls_to_any_crusher_and_plays_its_crush_sound`, `crusher_driveover_destroys_wall_but_noncrusher_does_not` (I4) |
| Crushable walls admit only crushers / `CrusherAll` at cell entry | `0x0073F3D0..F4F9` | `cell_entry.rs` class arm; tests `crushable_wall_admits_only_crushers_and_crusher_all`, `astar_routes_non_crushers_around_a_sandbag_line_and_crushers_through_it` (I9a) |
| The approach scatter uses `force = 0`, with the elite pre-scan / `PlayerScatter` / `[IQ] Scatter` dispatch gate and a ten-entry cap | `Crush_Cell 0x0074176D..0x0074177A` pushes `param_4 = 0`; `Scatter_Objects 0x00481670` | `ScatterEligibility`, `scatter_dispatch_allowed`, `cell_has_elite_occupant` (`bump_crush.rs:865-960`) |

---

## 3. Divergences

### D1 — the ally test is missing from the cell-entry crush classification

- **Native:** `0x005F6CD0` tests `HouseClass::Is_Ally(crusher->Owner, victim)`
  on **both** blocks, and `Can_Enter_Cell` re-tests it at `0x0073FB53`. An
  allied or own-house crushable infantryman therefore never latches; it falls
  through the ordinary occupant arm and yields **6** (non-moving allied
  non-building) or **2** (in-transit / head-on, per I3).
- **Rust:** `CrushTarget` (`bump_crush.rs:615`) carries no owner, and
  `can_crush` has no ally term — its doc says "the ally relation (tested by the
  caller)", but the caller `collect_crush_victims` (`bump_crush.rs:752`) does
  not test it either, nor does `cell_passable_after_crush`
  (`bump_crush.rs:822`). In
  `cell_entry::classify_occupied_cell_with_slave_query` the allied infantryman
  lands in `victims`, is `continue`d past `classify_blocker`
  (`cell_entry.rs:1381-1383`), contributes no code, and the function returns
  `CellEntryResult::Crushable { victims }` (`cell_entry.rs:1441`). The ally test
  exists **only** at the kill site, `classify_drive_crush_phase`
  (`bump_crush.rs:994-997`), which then refuses the kill.
- **Address:** `0x005F6D1A`/`0x005F6D67` (`Is_Ally` inside the predicate) and
  `0x0073FB53` (the latch's re-test).
- **Trigger:** any `Crusher=yes` vehicle ordered onto a cell holding its own or
  an allied crushable infantryman — the five `Crushable=yes` infantry types plus
  the 43 keyless (defaulted) ones, i.e. nearly all infantry.
- **Player-visible effect:** the tank is told the cell is enterable, drives onto
  its own GI's sub-cell and kills nobody; the GI is never treated as a friendly
  blocker, is never scattered, and the tank and the infantryman end up sharing
  the cell. Retail answers 6, scatters the man out of the way at 8× cost, and
  the tank waits.
- **Frequency:** routine — every mixed armour/infantry advance, and every
  regrouping over one's own infantry line.
- **Not covered by I4/I9a/I9c.** No test exercises it: `cell_entry.rs`'s crush
  tests are all wall-arm tests, and
  `classify_drive_crush_phase_full_cell_skips_allied_victim` covers the kill
  site only.

### D2 — the overlay crush's `Wall=` clause tests the wrong field

- **Native:** `0x0073B027..0x0073B034` requires the **mover type's
  `MovementZone=` to be `CrusherAll`** (`+0x5B4 == 0xC`), settled in §1.4 from
  the `ReadINI` key string and the name table.
- **Rust:** `world/mod.rs:5240-5244` computes
  `let drive = e.locomotor…effective_kind() == Some(LocomotorKind::Drive)` and
  gates on `flags.crushable || (flags.wall && drive)`. The doc comment above it
  and the I4 ledger row both call `+0x5B4` "LocomotorType", contradicting I9a's
  correct `MovementZone` reading of the same offset in the same plan.
- **Address:** `0x0073B02D`.
- **Trigger:** a `Crusher=yes` Drive vehicle occupying a `Wall=yes`,
  non-`Crushable=` overlay cell. Stock: 29 `Crusher=yes` types, all but `[BFRT]`
  non-`CrusherAll`, and nearly all Drive.
- **Player-visible effect:** a Grizzly (or any Drive crusher) flattens a brick
  wall, wooden fence or Kremlin wall that only the Battle Fortress should be
  able to flatten.
- **Frequency: UNCHECKED.** The I9a wall arm refuses cell entry into a
  non-crushable `Wall=yes` cell, so the clause needs a non-entry route onto such
  a cell (map pre-placement, deploy, teleport/chrono, or a wall raised under a
  parked unit). The sweep is a per-frame position scan, so a *parked* crusher on
  such a cell is enough. Instrument: enumerate the VERA writers that can place a
  `Crusher=yes` Drive entity on a `Wall=yes && !Crushable=` cell; if none exist,
  the clause is inert but still wrong and should be corrected for exactness.

### D3 — code 1 is the wrong admission code for a crush

- **Native:** the crush tail returns **0** when the cell's vehicle-occupation
  bit is clear (`0x0073FCF6` → `0x0073FD37`) and **2** when it is set and
  `CellClass::FindFirstUnit 0x0047EBA0` gives null or a non-crushable unit
  (`0x0073FD02..0x0073FD20`). Code 1 belongs to a cloaked non-allied Techno
  (`0x0073FAFF..0x0073FB25`, `FUN_0040DD20 0x0040DD20` + `+0x220 == 2`, cloak
  state written by `TechnoClass::StartUncloaking 0x007036C0`).
- **Rust:** `CellEntryResult::Crushable` maps to `yr_code() == 1`
  (`cell_entry.rs:265`), and there is no equivalent of the vehicle-bit / first-unit
  test on the crush path.
- **Address:** `0x0073FCF6..0x0073FD43` (the tail), `0x0073FAFF..0x0073FB25`
  (the real code-1 producer), `0x0081870C` (the cost table).
- **Player-visible effect today: none** — `movement_tick.rs:1284` groups
  `Clear | Crushable` at the same call site.
- **Frequency:** every crush, with no observable consequence while the cost
  table is unwired.
- **Downstream risk:** the moment `0x0081870C` is wired, code 1 costs 1000×
  where code 0 costs 1×, so every crushable cell becomes a near-wall to the
  search and crushers stop routing through infantry.
- This is already recorded in the `cell_entry.rs` header ("Code 1 is the wrong
  producer"), which called the code-1 receiver and field UNCHECKED. **Both are
  now native-established** (see §1.2), and the missing second half — native's
  code **2** from the vehicle-bit arm — was not previously recorded.

### D4 — the approach scatter is reachable only through the crushable classification

- **Native:** `Crush_Cell(cell, scatter_only = 1)` is called from
  `DriveLocomotionClass::Process_Movement 0x004B3E65` and
  `ShipLocomotionClass::Process_Movement 0x006A34B4` unconditionally; inside, the
  only tests are `Crusher=`/ability 0x11 and the cell's infantry sub-cell bits
  (`0x00741754..0x00741790`). Crushability and the ally relation are never
  consulted in scatter mode.
- **Rust:** the `EnteringCell` phase is invoked only from inside
  `CellEntryResult::Crushable` (`movement_occupancy.rs:739-790`), i.e. only when
  the classifier already found a crushable victim.
- **Address:** `0x004B3E65`, `0x00741754..0x00741790`.
- **Trigger:** a crusher approaching a cell whose infantry are all
  non-crushable (`Crushable=no`: TANY, BORIS, BRUTE, DESO, …) or all allied.
- **Player-visible effect:** those infantry never receive the approach scatter
  dispatch. With stock rules the dispatch gate suppresses it anyway for
  human-owned occupants (`PlayerScatter=no`, `[IQ] Scatter = 2` against a human
  house IQ of 0), so the reachable case is **a cell containing an elite
  occupant**, where retail scatters the whole cell and VERA does not.
- **Frequency:** low in stock (needs an elite in the approached cell); would
  become routine the moment an AI-owned house with a non-zero IQ exists — see
  `HUMAN_HOUSE_IQ` in `bump_crush.rs:842`.

### D5 — the crusher coordinate for the distance gate differs between VERA's three kill lanes

- **Native:** `TechnoClass::DistanceSquaredTo 0x005F6560` is called with the
  crusher's **live** `+0x9C/+0xA0` at the moment `Crush_Cell` runs
  (`0x00741801`).
- **Rust:** `movement_tick.rs:1365` and `movement_occupancy.rs:741` synthesise
  the **target cell centre** (`cell * 256 + 128`); `track_host.rs:1249` uses
  `position_world_coord(&crusher.position)`, the live position. Two different
  inputs to the same gate.
- **Address:** `0x00741801..0x0074180B`.
- **Effect:** all five native sub-cell offsets
  (`cell_kernel::INFANTRY_SUBCELL_OFFSETS` = `(128,128)` and
  `(64|192, 64|192)`) sit within 128 leptons of the cell centre — worst case
  `64² + 64² = 8192 ≤ 16383` — so the cell-centre lanes crush every slot. The
  live-position lane crushes fewer slots whenever the crusher is off-centre: a
  crusher on the cell edge is 192 leptons from the far sub-cells
  (`192² + 64² = 40960 > 16383`), and those infantry survive.
- **Frequency: UNCHECKED** — it depends on where within the step each lane
  fires, which this examination did not establish. Instrument: record the
  crusher coordinate at all three Rust call sites across a retail-map crush run
  and compare with the `Crush_Cell` entry coordinate captured at a
  `0x007416A0` breakpoint (`debugger_set_breakpoint` + `debugger_read_args`).

### D6 — the omni block does not fall through to the ordinary block

- **Native:** every failure inside the omni block jumps to `0x005F6D3F`, the
  ordinary block (`0x005F6CDA`, `CEE`, `CF2`, `CF8`, `D0C`, `D18`, `D28`,
  `D36`).
- **Rust:** `can_crush` returns early —
  `if capability.omni_crusher { return !omni_crush_resistant && !iron_curtained; }`
  (`bump_crush.rs:710-712`).
- **Trigger:** an `OmniCrusher=` crusher over a victim that is
  `OmniCrushResistant=yes` **and** `Crushable=yes`, or a building with
  `Crushable=yes`.
- **Frequency in stock: zero.** The six `OmniCrushResistant=yes` sections are
  DNOA, BFRT, AMCV, SMCV, PCV, SMIN; DNOA is `Crushable=no` and the rest carry
  no `Crushable=` key (default no). No stock building carries `Crushable=yes`.
  Mod-only.

### D7 — the ordinary path's `category == Infantry` gate has no native counterpart

- **Native:** the ordinary block `0x005F6D3F..0x005F6D89` reads `+0x22D`,
  `+0x14 & 1`, `+0x2A4`, `Is_Ally` and the Iron Curtain slot — **no
  `What_Am_I`**. The only class test in the whole predicate is `!= 6` (Building)
  inside the omni block.
- **Rust:** `can_crush` requires `target.category == EntityCategory::Infantry`
  (`bump_crush.rs:722`).
- **Frequency in stock: zero** — no stock vehicle or building carries
  `Crushable=yes`, and aircraft never appear in the ground occupant list. The
  Rust already records this as "VERA-internal, gamemd equivalent UNCHECKED"; it
  is now native-established as a real structural difference with no stock reach.

### D8 — `StartUncloaking` on a successful crush is unmodelled

- **Native:** `0x00741933` calls the crusher's `+0x45C` =
  `TechnoClass::StartUncloaking 0x007036C0` with NULL once the loop crushed
  anything.
- **Rust:** neither kill lane touches `sim/cloak_disguise.rs`.
- **Frequency: UNCHECKED.** No stock `rulesmd.ini` section is both
  `Cloakable=yes` and `Crusher=yes`, so no stock *type* reaches it; whether any
  runtime cloak source in stock YR can put a crusher into cloak state 1/2 was
  not established. Instrument: census the writers of `TechnoClass+0x220` and
  check whether any is reachable for a `Crusher=yes` type.

### D9 — neither rocking write nor `+0x6B5` is modelled

- **Native:** the A7 path writes `crusher+0x334` twice — `+= 0.02`
  (`0x0073B05B..0x0073B06E`, constant `0x007F4E38` = `0x3CA3D70A`) on an overlay
  crush, and `= -0.05f` (`0x00741954`, `0xBD4CCCCD`) on crushing a **vehicle**
  (`What_Am_I() == 1`) when the field is exactly 0. It also clears
  `crusher+0x6B5` at `0x0073B067`; that byte's only readers are
  `DriveLocomotionClass::Process_Drive_Track 0x004B1146`,
  `ShipLocomotionClass::Process_Drive_Track 0x006A0809` (both of which set it to
  1 at `0x004B1A2F` / `0x006A1071`) and `TechnoClass::RockingUpdate
  0x0070BA13`, where a non-zero value selects the alternate constant at
  `0x007F4E64`.
- **Rust:** `apply_wall_crush_on_driveover` records the `+= 0.02` as unwritten
  (no renderer); the `-0.05` vehicle-crush write and `+0x6B5` are unmodelled
  and, for the `-0.05`, **unrecorded** — I4's residual names only the overlay
  `+= 0.02` and the byte.
- **Player-visible effect:** no tank tilt when a crusher flattens a wall or a
  vehicle.
- **Frequency:** every crush.

### D10 — the `VehicleThief=` hijack arm is unmodelled

- **Native:** `0x0074181F..0x0074188D`.
- **Frequency in stock: zero** — `VehicleThief=` has 0 occurrences in
  `ini/rulesmd.ini`. Recorded for completeness so a future reader does not
  mistake the arm for part of the crush kill.

---

## 4. UNCHECKED items and the instrument that would settle each

| # | Item | Instrument |
|---|---|---|
| U1 | Where within the step each of VERA's three kill lanes fires relative to native's `Crush_Cell` entry, and hence the real crusher coordinate (D5) | Breakpoint `0x007416A0` in the Ghidra debugger, `debugger_read_args` + `debugger_registers` for `crusher+0x9C/+0xA0` over a retail crush; compare with logged coordinates at `movement_tick.rs:1365`, `movement_occupancy.rs:741`, `track_host.rs:1249` |
| U2 | Whether the crush predicate as a whole matches over the full input space | `emulate_function` on `0x005F6CD0` — it returns AL, so the result is register-observable; needs the four dispatched callees (`+0x84`, `+0x88`, `+0x160`, `Is_Ally 0x004F9A90`) stubbed or executed. This is the one A7 parity instrument that looks buildable |
| U3 | Victim unmark timing: native runs `MarkCellLists(0)` → Limbo → UnInit synchronously inside `Crush_Cell`; VERA removes occupancy in `handle_deferred_occupancy` and defers UnInit to `finish_movement_pass` | Already flagged in `movement_tick.rs:4191-4194`. Settle by a same-tick reader census: which native readers of the cell lists can run between `Crush_Cell` and the crusher's own mark |
| U4 | Reachability of D2 — can any VERA writer place a `Crusher=yes` Drive entity on a `Wall=yes && !Crushable=` cell? | Census the `position` writers against `OverlayGrid`; a `debug_assert` on the sweep would catch it at runtime |
| U5 | Whether any stock runtime cloak source can put a `Crusher=yes` unit into cloak state 1/2 (D8) | `get_xrefs_to` on the writers of `TechnoClass+0x220` |
| U6 | `victim+0x8D`, the skip byte at `0x00741811` | `search_instructions` census of `+0x8D` writers on FootClass |
| U7 | Whether `Crush_Cell`'s bridge-list selection (`0x007416BB..0x00741731`) matches VERA's `MovementLayer` choice in all three lanes | Bridge retail matrix run with a crusher over infantry on and under a span |
| U8 | Whether native's `Crush_Cell` really consumes no RNG (the claim rests on a direct-call census; the loop dispatches through `+0x170`, `+0xE0`, `+0x124`, `+0xD4`, `+0xF8`) | Breakpoint the RNG entry points and drive a crush |

---

## 5. Verdict on A7

A7 is **not met**. The kill mechanism, the predicate, the radius, the lifecycle
order and the overlay collapse are all modelled and match the binary as read
here, and I4/I9a/I9c closed the overlay and wall-admission halves. What I4/I9a/I9c
did **not** cover is the *admission* half of unit crushing:

- I4 covered overlays only (`Crushable=` overlays, `CrushSound=`), not occupants.
- I9a/I9c covered the wall arm of `Can_Enter_Cell` and unified
  `CrushCapability::of` across path searches. Neither touched the occupant crush
  latch `0x0073FB2A..0x0073FB6C` or its tail `0x0073FC24..0x0073FD43`.

The open, player-visible gap is **D1** — a crusher treats its own and allied
crushable infantry as crushable at cell entry, so it drives onto them instead of
scattering them. **D2** is a wrong field read inherited from I4's mis-naming of
`+0x5B4`, latent today but wrong. **D3** and **D5** are exactness debts with a
named downstream risk. The rest are stock-unreachable or cosmetic.

---

## 6. Proposed ledger row

Append to the increment ledger of
`docs/plans/2026-09-15-movement-retail-acceptance.md`:

| Increment | Criterion | Status | Evidence |
|---|---|---|---|
| I15 Occupant crush admission: the ally gate, and the overlay crush's `MovementZone` field | A7, A1 | OPEN | Native: `TechnoClass::Is_Crushable_By 0x005F6CD0` tests `HouseClass::Is_Ally 0x004F9A90` on **both** blocks (`0x005F6D1A`, `0x005F6D67`), and `UnitClass::Can_Enter_Cell`'s crush latch re-tests it at `0x0073FB53`, so an allied or own-house crushable infantryman never latches and yields code 6 (non-moving allied non-building) or 2 instead. VERA's `CrushTarget` carries no owner and `bump_crush::can_crush` has no ally term; `collect_crush_victims` and `cell_passable_after_crush` do not add one, so `cell_entry::classify_occupied_cell_with_slave_query` skips the ally past `classify_blocker` and returns `Crushable`. The ally test exists only at the kill site (`classify_drive_crush_phase`), which then refuses the kill — so a tank parks on its own GI and neither crushes nor scatters it. Trigger: any `Crusher=yes` vehicle ordered through a cell holding own/allied crushable infantry (5 explicit `Crushable=yes` types plus the 43 keyless infantry that default crushable via the `InfantryTypeClass` ctor `0x005238E2`). Frequency: routine. Second fix in the same owner: `world/mod.rs apply_wall_crush_on_driveover` gates the `Wall=`-only clause on `LocomotorKind::Drive`, but `0x0073B02D` compares `TechnoTypeClass+0x5B4` with `0xC`, and `+0x5B4` is **`MovementZone=`**, not a locomotor — settled from `TechnoTypeClass::ReadINI 0x0071605E..0x00716081` (key "MovementZone" at `0x008431C8`) through `CCINIClass::ReadMovementZone 0x00474E40` and `g_MovementZone_NameTable` (base `0x0081BA88`, index 12 = "CrusherAll" at `0x0081BAD0`, index 6 = "Subterannean" matching the adjacent `CMP EAX,6 / SETZ` at `0x0071607E`). I4's ledger row and the Rust doc comment both mis-name that offset; I9a's `MovementZone` reading of the same offset is the correct one. Stock: 29 `Crusher=yes` types, only `[BFRT]` is `CrusherAll`, so VERA lets every Drive crusher flatten non-crushable `Wall=yes` overlays (reachability UNCHECKED — cell entry is refused by I9a's arm, but the sweep is a per-frame position scan). Recorded alongside, not owned here: code 1 is the wrong admission code for a crush (native returns 0, or 2 when the cell's vehicle-occupation bit is set and `CellClass::FindFirstUnit 0x0047EBA0` gives null or a non-crushable unit, `0x0073FCF6..0x0073FD43`; code 1's real producer is a **cloaked** non-allied Techno, `FUN_0040DD20 0x0040DD20` + `TechnoClass+0x220 == 2`, cost 1000× at `0x0081870C`); the approach scatter is reachable only through the crushable classification where native calls `Crush_Cell(cell, 1)` unconditionally from `0x004B3E65`/`0x006A34B4` with no crushability or ally test; VERA's three kill lanes feed two different crusher coordinates into the 128-lepton gate (`0x00741801`); the crusher's `StartUncloaking 0x007036C0` at `0x00741933`, the `-0.05f` vehicle-crush rocking write at `0x00741954` and the `+0x6B5` clear at `0x0073B067` are unmodelled. Stock-unreachable and deliberately not modelled: every `IsTrain=` arm (`+0xC94`, 0 occurrences in `rulesmd.ini`) including the 10000-damage co-occupant loop `0x004B1864..0x004B1978`, the `VehicleThief=` hijack arm `0x0074181F..0x0074188D` (0 occurrences), the omni→ordinary fallthrough (`0x005F6D3F`), and the ordinary path's missing class test. Full examination: [MOVEMENT_ACCEPTANCE_A7_CRUSH_EXAMINATION.md](../research/MOVEMENT_ACCEPTANCE_A7_CRUSH_EXAMINATION.md). No parity instrument exists for A7; `emulate_function` on `0x005F6CD0` is the buildable candidate. |
