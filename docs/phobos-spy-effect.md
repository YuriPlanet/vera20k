# Spy-effect superweapon launches (`SpyEffect.*`)

Phobos lets a building launch a configured superweapon for the victim house
and/or the spy's house when a spy infiltrates it. Three building-type keys
(`Ext/BuildingType/Body.cpp:337-345`, fields `Body.h:59-61`, constructor
defaults `Body.h:192-194`).

```ini
[SOMEBUILDING]
SpyEffect.Custom=false                      ; Valueable<bool>, Ares tag
SpyEffect.VictimSuperWeapon=                ; ValueableIdx<SuperWeaponTypeClass>
SpyEffect.InfiltratorSuperWeapon=           ; ValueableIdx<SuperWeaponTypeClass>
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| — | `BuildingTypeExt::LoadFromINIFile` reads | `SpyEffect.Custom` always; both index keys only while `SuperWeaponTypeClass::Array` has a type. |
| `0x51A00B` | `BuildingClass_InfiltratedBy_Wrapper` | The `InfantryClass::Per_Cell_Process` call site of `BuildingClass::Infiltrate @ 0x004571E0` is replaced: save the owner's balance, call the original/Ares-rewritten function, then `BuildingExt::HandleInfiltrate` (`Ext/Building/Body.cpp:662-671`). |
| — | `BuildingExt::HandleInfiltrate` | `AccumulatedIncome += ownerMoney - moneyBefore`; `if (!SpyEffect_Custom) return false;`; when the houses differ, the victim index launches the **victim house's** copy and the infiltrator index the **infiltrator house's** copy (`Ext/Building/Body.cpp:316-356`). |
| `0x6CB820` | `SuperClass::SetReadiness(true)` | A byte write to `+0x6F` (the whole function is `mov [ecx+0x6F], al; ret 4`). |
| `0x6CC390` | `SuperClass::Launch(cell, isCurrentPlayer)` | The type's action at the cell. |
| `0x6CE0B0` | `SuperClass::Reset()` | Releases the placement anim and restarts the charge. |

The lambda (`Ext/Building/Body.cpp:333-346`) saves the Super's
`RechargeTimer.StartTime`/`TimeLeft`, forces readiness, launches at
`CellClass::Coord2Cell(building->GetCenterCoords())`, resets, then writes the
saved timer back — the launch does not consume the recharge.

`ValueableIdx<SuperWeaponTypeClass>::Read` (`Utilities/TemplateDef.h:1812-1826`)
resolves a name through `FindIndex`; `none`/`<none>` stores -1 and an absent or
empty key keeps the field. Constructed indices are -1, so the arms are dormant
until the keys are authored.

## Where VERA20K owns it

- `rules::phobos_spy_effect::PhobosSpyEffect` holds the three keys;
  `ObjectType::phobos_spy_effect` carries them. The two indices keep their
  names and are resolved through `RuleSet::super_weapon` at use, so an
  unknown name launches nothing (the answer an unset index gives); the native
  `SuperWeaponTypeClass::Array.Count > 0` read guard is deferred to that same
  lookup.
- The wrapper's call site is
  `Simulation::infantry_per_cell_infiltrator_entry` (`world_orders.rs`), the
  production Per_Cell entry: after `infiltrate_building` (vanilla or Ares)
  returns, `Simulation::phobos_handle_infiltrate` (`spy_infiltration.rs`)
  checks `SpyEffect.Custom` and the house difference and launches victim-first
  at the building's `GetCoords` reduced to a cell.
- The launch transaction is
  `superweapon::launch_super_preserving_charge` (`superweapon/fire.rs`): it
  saves `charge_start_tick`/`charge_duration`, writes readiness through the
  new `SuperWeaponInstance::set_readiness`, reuses the shared `launch` owner,
  reuses `reset_super` (`SuperClass::Reset`), and stores the saved timer back.
  `set_readiness` also replaces the direct `+0x6F` clear in the PostClick tail.

## Phobos quirks kept

- Both arms require `SpyEffect.Custom`; without it nothing launches even when
  the indices are set.
- The launch arms run **in addition to** the base infiltration (money, radar,
  power, tech, `SuperWeapon=`, storage, factory) and after Ares' own
  `InfiltratedBy` arms, matching the wrapper's ordering. Under Ares the entry
  is reached only when Ares' `InfiltrateAction` is `Spy` (an allied building
  is refused before the call); under the stock entry any different house is
  admitted. A same-house entry never launches.
- Victim index first, then infiltrator index; each uses its own house's Super
  instance.
- The recharge timer is restored exactly (`StartTime`/`TimeLeft`); readiness
  stays cleared by `Reset`.

## Residuals (trigger · impact · frequency · risk)

- **Income bookkeeping not owned.** `HandleInfiltrate` first adds the stolen
  amount to `BuildingExt::ExtData::AccumulatedIncome` and, for a computer
  victim with `DisplayIncome.AllowAI=no`, mirrors it into the Ares display
  state. VERA owns no AccumulatedIncome (the DisplayIncome chain keeps its
  settings only), so the launches run without that write. Trigger: any spy
  entry. Impact: no display-income accumulation from spies. Frequency:
  presentation only. Risk: low.
- **`IsCurrentPlayer` launch flag.** Native passes
  `pHouse->IsCurrentPlayer()` to `Launch`; the shared launch owner takes no
  such flag and no stock arm reads one, so the launched effects are the same
  for both audiences. Trigger: any launch. Impact: any player-only cue inside
  Launch is not differentiated. Frequency: presentation. Risk: visual-only.
- **Read-time `FindIndex`.** The native reader keeps the previous index when a
  non-blank name does not resolve; the merged-section reader VERA uses has no
  per-layer previous value, so a later layer's unknown name drops the earlier
  valid one. Trigger: an invalid name authored in a later INI layer. Impact:
  no launch instead of the earlier layer's superweapon. Frequency: broken mod
  data. Risk: low.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
