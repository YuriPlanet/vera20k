# Team script action 56: chronoshift to a building

Action `56,<argument>` uses the selected leader's House's Chronosphere to
move the source area to a selected building. The building is the destination,
not the object being teleported. This is a vanilla Yuri's Revenge action,
not an Ares-only opcode. It does not grant a superweapon or bypass ordinary
Chronosphere effects.

The argument has the same packed BuildingType index/selection mode as actions
46 and 47. The low 16 bits select the registered BuildingType; the high 16 bits
select least threat (0), highest threat (1), nearest (2), or farthest (3).
For example, `0=56,131072` selects the nearest admitted building of registry
index 0 (`131072 = 2 << 16`). Use the index in the actual scenario's registry,
not a hard-coded building name or a coordinate.

## Source and adaptation

This contribution adapts the author's earlier ra2-mono work and is submitted
under VERA's GPLv3, without a separate license notice. In particular:

- `AIScriptActionWiring.cs::QueryTeamSuperweaponProgress` supplies the
  action-56 building-specific branch: use the building-property selector, not
  a general visible/armed quarry scan, and retain the exact building target.
- `TryResolveTeamSuperweaponSource` supplies the selected member's House
  authority and source observation. `GameLogicManager.TriggerTargetCells.cs`
  supplies the readiness-before-target and source/destination pair boundary.
- `GameLogicManagerTriggerSuperweaponTests.cs::AiTeamChronosphereBuildingsAction_UsesOriginalBuildingSelectorAndPrimaryHouseLedger`
  supplies the regression where Team owner, leader owner and another enemy
  differ, and only the leader's current enemy should be selected.

The original helper is `TeamClass` action 56 at `0x006EFE60`, dispatched at
`0x006E9DAE`. It selects a leader, checks the last Supers of Type 3 and 4,
checks readiness/power, calls `Find_Best_Target_Building @ 0x006EEBD0`, fires
both stages through `HouseClass::Fire_SW @ 0x004FAE50`, assigns the exact
building as mission target and finishes. When the source is almost ready it
waits; no eligible target, missing types or a source that is not almost ready
finishes without a launch.

VERA reuses its existing leader query, House-owned Supers, building selector,
team centre (`TeamClass+0x34`), coordinate conversion and Fire_SW. Actions 56
and 57 share the paired-fire/readiness implementation; only their target
selection differs. Action 57 keeps its quarry scan. No C# world, alternate
superweapon state, custom timer or save field is introduced. The ordinary
ScriptType reader and its 50 numbered slots are unchanged.

## Compatibility boundary

- The shared building search retains its House/enemy/alliance/passive rules,
  distance/threat scoring and construction-order tie handling. It does not
  add a weapon or fog gate to action 56.
- Fire_SW's native Type-value/list-index coupling and the existing Super
  readiness/Chronosphere limitations still apply. In particular, the current
  money owner refuses nonzero `Money.Amount` on two-stage Chronosphere types;
  this action does not bypass that policy.
- Missing team centres finish without firing instead of reproducing the
  original null-object fault, as in VERA's existing action 57.
- Authored Rust regressions exercise production readers, VM dispatch,
  target selection, charge and Fire_SW. They are not native-execution
  goldens, a retail campaign playtest or full Mental Omega certification.
