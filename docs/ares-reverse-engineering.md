# Ares reverse engineering, unit sale, produced cash and the Secret Lab

This optional chain requires the compile-time `ares` feature. Default builds
read none of its keys and keep stock grinding, the building-only SELL event,
stock ProduceCash and the stock Secret Lab draw alone.

## Reverse engineering

A victim ground at a `ReverseEngineersVictims=yes` building
(BuildingExt::ExtData::ReverseEngineer `10012E00`) whose type has
`CanBeReversed=yes` (default) records `ReversedAs=` (or its own type) in the
grinder owner's HouseExt `ReverseEngineered` list (`+0x34`,
HouseExt::ExtData::ReverseEngineer `10022360`): `sim/reverse_engineering.rs`,
state in `HouseState::ares_reverse_engineered` (saved, hashed while
nonempty). When the type was not buildable before (PrereqValidate
`10021B00`, `production::can_build`), House+1FC is set. The two hooks run
first in the PerCellProcess Grinding arm (`building_entry::foot_per_cell_eaten`):

- `0x5198AD` / `100498D0` (infantry) and `0x73A1BC` / `1004C460` (vehicles):
  reverse engineer; when recorded, the victim owner's player hears
  `EVA_ReverseEngineeredInfantry` (or `...Vehicle`) and
  `EVA_NewTechnologyAcquired`, and the grinder's Tag raises event 80 (source
  the victim), then 79. The vehicle arm then refunds its hijacker (captor
  InfantryType, Techno `+0x338`) to the grinder's owner (#368).
- RequirementsMet (`10021FF0`) answers Overridden for a listed type: the
  type skips its prerequisites, tech level and house lists, but still needs a
  factory and the build limit (`production::requirements_met`, asked by
  `production::can_build`).
- Trigger event 78 asks the Trigger's House's list
  (`trigger_runtime_ares`); `SpyEffect.UndoReverseEngineer` (`10012000`)
  empties it and sets House+1FC (`spy_infiltration`).

## Unit sale on a depot

- FootClass::CanBeSold (`0x004D9E70`) is `production::foot_can_be_sold`: a
  Unit or Aircraft whose first radio contact is a building with
  `UnitSell.Get(UnitRepair)` and whose type lacks
  `Unsellable.Get([General] UnitsUnsellable)` (hook `0x4D9EBD`, `10048FD0`),
  within 0x80 leptons of the building's dock coordinate for it (hook
  `0x4D9EE1`, `1005CA30`).
- Sell mode over such an own object shows SellUnit (hook `0x6929FC`,
  `10057E70`, `app/input/cursor.rs`), and the click queues the SELL event
  (`app/input/commands.rs`, BandBox_LeftUp's action 0xD arm).
- The SELL event's unit arm (`0x004C6F5A..0x004C6F96`,
  `world_commands`) calls On_Sold (`0x004D9F70`, `production::foot_on_sold`,
  shared with the depot's computer sale) when a building stands in the
  unit's cell: `EVA_UnitSold` and `SellSound` for the owner's player, the
  refund, Stun, Limbo, UnInit.

## Produced cash

Both stock ProduceCash blocks are replaced (`credit_income`):

- `0x43FD2C` / `100146E0`: the type (`+0x6D0`) and each installed upgrade
  (BuildingExt timers `+0x54/+0x60/+0x6C`, saved, hashed once started) with a
  positive `ProduceCashDelay` restart a timer with one frame or none left for
  `delay + 1` frames, paying their `ProduceCashAmount` when one was left. A
  nonzero sum goes to a non-passive owner of an operational building.
- `0x4482BD` / `10013D40`: on capture from a `MultiplayPassive` house, the
  type and upgrades with a startup or an amount add their startup and
  restart their timers; a nonzero sum goes to a non-passive new owner.
- `ProduceCashDisplay` (`100117C0`) adds each payment to the building's
  floating money display (`sim::bounty`).

## Secret Lab

The Place hook (`0x445F80` / `100100B0`, at the entry of
`Simulation::grand_opening`, captures and repeated calls included) runs
UpdateSecretLab (`10013700`, `sim::secret_lab`) for a `SecretLab=` type
before it grants factory plans:

- Nothing for a House whose Country is `MultiplayPassive`, a type with a fixed
  `SecretInfantry=`/`SecretUnit=`/`SecretBuilding=`, or a lab that already
  rolled (BuildingExt `+0x50`, `GameEntity::ares_secret_lab_placed`, saved and
  hashed once set) unless its type has `SecretLab.GenerateOnCapture`.
- The options are `SecretLab.PossibleBoons` (NullableVector, `1000EA30`,
  `<default>` resets it) when set, else `[General]` SecretInfantry,
  SecretUnits and SecretBuildings in that order; AddToOptions (`10011340`)
  keeps a type whose `SecretLab.RequiredHouses` (default all) has and
  `SecretLab.ForbiddenHouses` (default none) lacks the owner's Country bit
  (ReadHouseTypesList `0x4750D0`), and whose RequirementsMet (`10021FF0`,
  `production::requirements_met`, the qualification `can_build` asks) is
  Forbidden or Incomplete: a type whose requirements the owner meets
  (Complete; prerequisites are not asked), or one reverse engineered,
  overridden or granted by a lab (this one too), drops out.
- One Scenario `RandomRanged(0, n - 1)` picks Building+6F4 and sets `+0x50`
  (no draw for one option; none and the lab stays unrolled for no option).

The stock startup draw (`68C050`, Post_Map_Init) is not hooked and still
overwrites every lab's boon; Ares's Buf_General sets the count it needs.
On retail maps the neutral (passive) labs therefore roll when first captured,
from the types the captor cannot build yet.

## Hooks and residuals

| Hook | Ares.dll | Name | Disposition |
|---|---|---|---|
| `0x5198AD` | `100498D0` | InfantryClass_UpdatePosition_EnteredGrinder | ported: `reverse_engineering` |
| `0x73A1BC` | `1004C460` | UnitClass_UpdatePosition_EnteredGrinder | ported: `reverse_engineering` |
| `0x4D9EBD` | `10048FD0` | FootClass_CanBeSold_SellUnit | ported: `production::foot_can_be_sold` |
| `0x4D9EE1` | `1005CA30` | FootClass_CanBeSold_Dock | ported: `production::foot_can_be_sold` |
| `0x6929FC` | `10057E70` | DisplayClass_ChooseAction_CanSell | ported: Unit/Aircraft arm; the building arm is the stock answer |
| `0x43FD2C` | `100146E0` | BuildingClass_Update_ProduceCash | ported: `credit_income::produce_cash_step` |
| `0x4482BD` | `10013D40` | BuildingClass_ChangeOwnership_ProduceCash | ported: `credit_income::produce_cash_on_owner_change` |
| `0x445F80` | `100100B0` | BuildingClass_Place (UpdateSecretLab `10013700`) | ported: `secret_lab::ares_update_secret_lab` |

Residuals:

- RequirementsMet's Unbuildable (`Prerequisite.RequiredTheaters`),
  `Prerequisite.Negative` (Forbidden) and `Prerequisite.StolenTechs`
  (Incomplete) answers are ported (`docs/ares-prerequisites.md`); the
  reverse-engineering recheck now asks RequirementsMet after the record and
  skips House+1FC below Incomplete, as `10022360` does.
- UpdateSecretLab's three debug log lines are not ported.
- `ReverseEngineered` holds type identities as category and Rules ID; Ares
  holds type pointers. Equivalent within one Rules registry.
- UpdateSecretLab is the chain's one RNG draw (above); no arm detaches. The
  ProduceCash timers are the chain's timer writes, and the grinder's Tag
  events may run trigger actions.
