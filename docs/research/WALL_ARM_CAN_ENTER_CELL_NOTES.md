# Wall arm of `Can_Enter_Cell` — research notes

**Status: research notes, not a report.** Facts below were read from `gamemd.exe`
(SHA-256 `1cdd1180…4298c`) on 2026-09-15 by disassembly and decompile; addresses
cite the read site. Nothing here is a parity claim. Consumed by ledger rows I9a
(crusher route) and I9b (open: codes 4/5, A* wall cost, wall-attack
Override) in `docs/plans/2026-09-15-movement-retail-acceptance.md`.

## Native facts gathered 2026-09-15

## UnitClass::Can_Enter_Cell 0x0073F0A0, wall arm 0x0073F3D0..F4F9 (disassembly + decompile)
- cell.OverlayTypeIndex (+0x44) == -1 -> skip arm.
- ot = OverlayTypes[idx]; ot+0x2AA nonzero && !HouseClass::IsControlledByHuman(owner) && g_GameMode == 0 -> return 7 (0x0073F3EC..F41D).
- ot.Wall (+0x2A8) zero -> skip arm (0x0073F428).
- Crusher route (0x0073F42E..F46C): ot.Crushable (+0x22D) && (type.Crusher +0xD28 || TechnoClass::HasWeaponAbility(0x11) 0x0070D0D0),
  OR (ot.Wall && type.MovementZone (+0x5B4) == 0xC CrusherAll)
  -> HouseClass::Is_Ally_ByIndex(cell wall owner +0x50) 0x004F9A10: ally -> code = max(code,4) (0x0073F4EB); not ally -> code unchanged (free entry).
- Weapon route (0x0073F483..F4E9): vtable +0x2AC (0x00701120: primary weapon slot +0x3F4 non-null) false -> return 7;
  Weapon(0) (+0x3F8) -> Warhead (+0xAC): Wall (+0x144) or (Wood (+0x147) && ot.Armor (+0x9C) == 6) else return 7;
  Is_Ally_ByIndex(wall owner): ally -> code = max(code,4); enemy -> code = max(code,5) (0x0073F50E per decompile).

## InfantryClass::Can_Enter_Cell 0x0051BF90, wall arm (decompile)
- ot+0x2AA (`Crate=`, written by `OverlayTypeClass::ReadINI 0x005FE770`; see `native_mark_overlay_data` in `src/rules/overlay_types.rs`) && !human -> 7.
- ot.Wall && (cell+0x11E >> 4) != ot+0x2A0 (`DamageLevels=` from the art section, parsed as `damage_levels`): the wall arm is skipped on a fully destroyed wall stage:
  +0x2AC false -> 7; Weapon(0) -> WeaponTypeClass__WarheadDamagesWalls (0x00772AC0, labelled 2026-09-16;
  warhead +0x144 Wall only) false -> 7; code = 5 - Is_Ally_ByIndex(owner).
- No crusher route for infantry.

## A* consumption
- AStar_compute_edge_cost 0x00429830 indexes 0x0081870C [1,1000,1,1,60,20,8,10000]; codes 4/5 expand at 60x/20x.
- VERA: core.rs `apply_search_cost_class_multiplier` has the table; `search_cost_classifier` hook exists but no production site sets it;
  entity_block_map carries 2/5/6 per cell. Neighbour passability is a bool from `is_cell_passable_for_mover_with_speed`.

## Crossing / blocked response
- DriveLocomotionClass::Process_Movement 0x004B2630: code dispatch 0x004B36F4 (6 -> scatter arm), 0x004B3944 (1 -> re-enter), else shared entry
  0x004B3607 (null Head_To); 0x004B364D `CMP code,2` -> code-2 wait; else 0x004B3A97: code 5 or 4 -> 0x004B3AD3:
  [ESP+0x64] set -> drop path (+0x5E0=-1), start +0x640 timer, return; else 0x004B3B03 -> cell of the refused coord
  (0x005657A0 -> 0x0047C5A0) then 0x004F9A90(owner house, cell) ... (continues; read 0x004B3B5E on).
- Walk/Hover: Override arm wall case (movement_occupancy.rs notes 0x00515C9C for Hover); cell target needs a Restore path.

## VERA state today
- Crushable walls reduce to zone class CRUSHABLE (overlay_reduced_zone_type); path grid `overlay_blocks` only for WALL/IMPASSABLE;
  class arm tests `zone_type == WALL`; so sandbags/fences admit every mover with a passable speed row (documented in cell_entry.rs header, I8).
- Non-crushable walls: HardBlocked unless Destroyer/AmphibiousDestroyer/InfantryDestroyer/CrusherAll (cell_entry.rs class arm; cell_rect.rs).
- Inputs available: OverlayTypeFlags {wall, crushable, armor_is_wood}; OverlayCell {overlay_id, overlay_data, wall_owner}; ResolvedCell.overlay_id/overlay_zone_type;
  WarheadType {wall, wood}; GameEntity.regular_crusher; combat_weapon::primary_for_tier(obj, veterancy).
- Identified: ot+0x2AA = `Crate=`, ot+0x2A0 = `DamageLevels=`. Unmodelled: mover ability 0x11 (no stock grant).
- Stock wall-capable non-crushers (primary warhead `Wall=yes`): `[FV]` (`HoverMissile` -> `HE`), `[BRUTE]` (`Punch` -> `Battering`).

## Drive code-4/5 arm, continued (0x004B3B03..3BEF, disassembly)
- 0x004B3B03: cell = Get_CellClass(refused coord) (0x005657A0); CellClass::Find_Blocking_Object 0x0047C5A0(cell):
  - found: HouseClass::Is_Ally_ByObject 0x004F9A90(owner house, object): ally -> nothing (0x004B3BEF); not ally -> owner vtable +0x1F4 (1, object)
    = mission override with Attack (1) on the blocking object.
  - none (0x004B3B94): cell.OverlayTypeIndex != -1 && OverlayTypes[idx].Wall (+0x2A8) -> owner vtable +0x1F4 (1, cell) = override Attack on the wall cell.
- So a Drive mover refused with code 4/5 attacks the wall cell (or the enemy body) through the same Override slot Walk/Hover use; VERA's
  Override arm (movement_occupancy.rs) exists for Walk/Hover objects only and has no cell-target Restore path.

## The wall-attack cycle, end to end (read 2026-09-15, second pass)

- **Override slot identity.** UnitClass vtable `0x007F5C70 + 0x1F4 = 0x007F5E64` holds `0x004D8F40`,
  `FootClass::Override_Mission(mission, target, destination)`: `SuspendedNavCom (+0x5A8) <- NavCom (+0x5A4)`,
  then `TechnoClass::Override_Mission 0x007013A0` (`SuspendedTarCom +0x2B8 <- TarCom +0x2B4`,
  `MissionClass::Override_Mission`, `Assign_Target` via `+0x3C8`), then `Assign_Destination(+0x480)(destination, 1)`.
- **All three ground locomotors take the same arm.** Drive `0x004B3B03..3BEF` and Hover `0x00515C3F..5C9C`:
  `CellClass::Find_Blocking_Object 0x0047C5A0` on the refused cell; a found object that is not allied
  (`HouseClass::Is_Ally_ByObject 0x004F9A90`) gets `Override_Mission(1, object, 0)`; with no object, a cell whose
  `OverlayTypeIndex (+0x44) != -1` and `OverlayType.Wall (+0x2A8)` gets `Override_Mission(1, cell, 0)`: Attack, the
  wall **cell** as TarCom, null destination. Walk's pair is the one `movement_occupancy.rs` already cites.
- **Unowned walls are enemies.** `HouseClass::Is_Ally_ByIndex 0x004F9A10` returns true for its own index, false for
  `-1`, else tests the ally bitfield at `+0x5788`. A wall with no owner therefore takes code 5 on the weapon route and
  free entry on the crusher route. VERA reconstructs map-wall owners from nearby buildings
  (`MapWallOwnerCandidate` in `src/sim/overlay_grid.rs`); `wall_owner: None` maps to `-1`.
- **How the attack on the wall cell ends is already modelled in VERA.** Wall destruction runs the transaction host's
  `pointer_expired(WallPointerTarget::Real)` (`SimulationWallRuntimeHost` in `src/sim/world/mod.rs`,
  `AoEWallDamageHost` in `src/sim/combat/combat_aoe.rs`), which calls `expire_cell_target_references`: every listener
  whose `attack_target` is that cell has its target cleared and, if a mission was suspended, Restore runs
  (`restore_entity_after_target_expiry`). That is the native CellClass pointer-expiry order (clear, then Restore). So a
  wall-attack Override needs no new termination logic, only the producer and the Override with a cell target.

## Native facts added 2026-09-16 (disassembly + decompile; each re-derived by an independent reviewer)

- **The wall arm does not return.** It accumulates `max(code,4)` at `0x0073F4EB` / `max(code,5)` at
  `0x0073F50E` into EBP (stored to `[ESP+0x18]`) and then falls through: `0x0073F520 TEST ESI,ESI /
  JZ 0x0073FA92` sends an empty object list straight to the land-row block, and the occupant walk
  exits there too. `0x0073FAB5 FLD [ECX*4 + 0x89EA40]` / `FCOMP [0x007E1748]` (eight zero bytes):
  row == 0 falls through to `MOV EAX,0x7` at `0x0073FAD0`, **discarding the accumulated 4/5**;
  row != 0 takes `JZ 0x0073FC24`, where `TEST EBP,EBP / JNZ` returns the accumulated code. Infantry
  mirrors it at `0x0051C750`/`0x0051C7D0`. The deck branch never reaches the read: `0x0073FA92`
  tests the deck flag and jumps it at `JNZ 0x0073FC24`.
- **Two distinct 7-exits, previously conflated here.** An unarmed mover leaves at `0x0073F48F`
  (`JZ 0x0073FCD0`, the shared epilogue) without reading a warhead; a warhead miss returns 7 at its
  own exit, `0x0073F4C9`. Same value, different exits.
- **`Wood=` is Unit-only.** `0x00772AC0` (now `WeaponTypeClass__WarheadDamagesWalls`) is one test:
  `warhead = *(weapon + 0xAC); return warhead != 0 && *(warhead + 0x144) != 0`. No `+0x147`, no
  `Armor == 6` compare. So the infantry arm has no Wood route at all.
- **Slot 0, not the current weapon.** `0x0073F497 PUSH 0x0` into vtable `+0x3F8`
  (`TechnoClass::GetWeapon 0x0070E140`, elite-only tier): the warhead comes from weapon slot 0
  unconditionally, while `Is_Armed` (`+0x2AC`) resolves the turret-aware *current* weapon. Different
  slots — conflating them diverges on a turreted type whose slot 0 is empty.
- **Overlay land defaults, and why the 4/5 codes are reachable at all.**
  `OverlayTypeClass::ReadINI 0x005FE770` reads every field in the echo form
  `ReadX(section, key, *(this + off))`, so an absent key keeps the constructor default; its field
  map records `+0x298 Land` default `0 = Clear` and `+0x2AC NoUseTileLandType` default true, and the
  body's `if (Tiberium && Land == 0) Land = 5` corroborates the `Land` default independently of the
  annotation. No stock `Wall=yes` overlay declares `Land=`, so stock walls carry the passable
  `[Clear]` row rather than the all-zero `[Wall]` row — had `Land` defaulted to 4, native would
  refuse every wall at the land row and its own 4/5 codes would be unreachable. The constructor
  itself was not read.
- **`CellClass::RecalcAttributes 0x0047D2B0`** opens with `this->LandType = ot->Land` (`+0x298`) and
  early-returns on `Land == 4`/`9` or `NoUseTileLandType` (`+0x2AC`) — VERA's
  `uses_early_recalc_land_branch`. So the cell's land row is *post-overlay-land* in gamemd too;
  reading it (rather than a wall-blind row) is the faithful analogue.

## What I9b has to add

1. **Producer:** the class arm's wall test answers 4 (allied wall) / 5 (enemy wall) / 7 for the weapon route and
   `max(code, 4)` for a crusher on an allied crushable wall, from mover facts: primary weapon present, primary warhead
   `Wall=` (Unit also `Wood=` against an `Armor=wood` overlay), `Crusher=`, CrusherAll, infantry, owner alliance.
   `CanEnterCellResult` has only `Clear`/`HardBlocked`, so it needs a cost-class carrying variant.
2. **A\*:** consume that class through `apply_search_cost_class_multiplier` (60x / 20x, table already present).
   Production entry `zone_search::find_layered_path_zoned_marker_detailed` takes 19 positional arguments and fans out
   through six wrappers; the mover facts belong in one request struct rather than three more positional flags.
3. **Crossing:** a refused wall step runs the Override with `TargetKind::Cell` for Drive, Hover and Walk
   (`authority.rs::override_entity_to_attack` is entity-only today), stops the mover, and relies on the existing
   expiry path to Restore.
4. **Consumers to re-check:** `.is_clear()` callers (scheduling, spawn, cursor) must keep treating 4/5 as not clear.
