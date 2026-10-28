# Optional Ares compatibility

VERA20K can build in partial, Rust-native support for some Ares extensions. It does not load the
Ares DLL and is not general mod support.

Default builds use the retail readers and dispatch; an INI or client setting cannot enable omitted
extensions. Build the supported extensions explicitly:

```sh
cargo run --release --bin vera20k --features ares
```

Ares builds identify themselves in the window title and startup log. Saves retain the build
capability and cannot load in another capability combination, even with identical retail INIs.

Ares is one of three orthogonal capabilities: it combines with `phobos` and `npatch`
(`--features ares,phobos,npatch`), and every implemented feature must keep working when the others
are compiled in. The rule and the pattern for two extensions touching one native site are in
[NPatch placeholder](npatch.md#cross-extension-compatibility).

For ordinary selected-map offline loading, Ares Country loading fields are read
from the retained Country identity. `File.LoadScreenPAL` and the loading text
color bind before Full_Init; the background filename and `LoadScreenText` labels
bind after its second primary Country read and before Type reset. The loading
attempt retains that resolved palette until it retires, instead of reopening a
later Country palette. CnCNet's offline route admits Countries through the same
reader without replaying cached shell settings or consuming a PortPool RNG draw.
Campaign, observer and generated-map presentation retain their existing paths;
country row flags and status use the country presentation chain, and this
support does not certify whole-frame parity.

For offline CnCNet campaign launches, both builds pass the client's explicit `Scenario`
independently of campaign record 0, as the upstream spawner's
`StartScenario(ScenarioName, 1, 0)` call does. The retained first record supplies its real CD,
final movie and continuation identity even when its authored Scenario differs. `CampaignID`
and `Side` do not select a retail array entry; an empty catalog fails before reading the seed.
Ares builds additionally accept byte-wide campaign `Side` metadata, while default builds
retain their existing Side 0/1 admission. This does not change named campaigns in VERA's menu.
The [native prefix controls](../tools/input_oracle/campaign_start_prefix.md) demonstrate
filename/index/CD independence; they do not certify the installed legacy mod spawner,
complete mission loading or all downstream campaign extensions.

Supported extensions:

- [Ordinary ground and tower EMP](ares-emp.md) (bounded human Drive/Walk, unteamed AI Drive and independent powered defensive buildings)
- [Water and land type conversion](../src/sim/game_entity/type_conversion.rs):
  `Convert.Water` applies over water or beach; `Convert.Land` applies over land
  and bridges. It uses the same in-place conversion owner as script action 68,
  preserves the object identity and ignores a target equal to the current type.
- [Custom armor](ares-custom-armor.md)
- [Supplemental string tables](../tools/rules_oracle/ares_csf.md), shared by
  game loading and default `asset csf-get` / `csf-grep` queries.
- [Per-type SHP building palettes](ares-building-palettes.md)
- [EVA voice tags and Side selection](ares-eva.md)
- [Fourth and later Side defaults](ares-side-defaults.md): crew, engineer,
  technician, disguise, survivor divisor, AI base defences and counts,
  paradrop lists/plane, parachute animation, tooltip and message colors, and
  the registered-side AI trigger token. Score screens, dialog backgrounds and
  the Hunter Seeker launch remain separate.
- [Country presentation](ares-country-presentation.md): `File.Flag`,
  `MenuText.Status`, `File.Taunt`, `ListIndex`, random/AI selection weights,
  ParentCountry inheritance, and the shell/Co-Op flag and status consumers.
  Observer art is retained but has no renderer yet.
- [Country paradrop overrides](ares-country-paradrop.md): `ParaDrop.Types`,
  `.Num`, `.Aircraft` and `Parachute.Anim` in front of the SideExt values.
- [Country AI power plants](ares-country-powerplants.md): `AI.PowerPlants=`
  and the draw among the plants the AI can build when it splices one.
- [Generic production prerequisites](ares-prerequisites.md)
- [Factory-source restrictions](../src/sim/production/can_build.rs) (`BuiltAt`,
  `Factory.ExplicitOnly`) and [Country plan qualification](../tools/rules_oracle/ares_factory_owners.md)
  (`FactoryOwners`, forbidden Countries and permanent plans).
- [Vehicle drivers](../src/sim/vehicle_theft.rs): `CanDrive`, Country `CanBeDriven`,
  vehicle theft and driver replacement, with passenger and survivor handoff.
  [KillDriver](../src/sim/vehicle_theft/driver_kill.rs) respects type/rank protection,
  health and chance gates; a driverless hull stops acting until a new driver enters.
  Foot `Operator` requirements deactivate an empty hull and resume it through the
  shared power/EMP admission when a qualifying passenger enters. Building-specific
  Operator updates remain outside this path.
  Per-type `Survivor.*`, `Crew.TechnicianChance`, `Crew.EngineerChance` and nullable
  aircraft `Crashable` share the existing crew and placement owners. These paths
  require `ares`; loading INI settings cannot activate them in a default build.
- [Extended IFV modes](../src/rules/gunner_turrets.rs): Gunner vehicles read
  `WeaponCount` beyond the original eighteen slots, including normal/elite weapons,
  ART firing offsets and `WeaponTurretIndexN` mappings. `WeaponUINameN` supplies
  the current mode's full tooltip name. Default builds retain the original bound
  and FV-only mapping. Barrel geometry and `TurretLocked` retain their separate
  support limits documented in the [firing-offset owner](../src/rules/weapon_offsets.rs).
- [NoManualFire](../src/app/input/context_order.rs) blocks ordinary manual attacks;
  C4 and infiltration remain incomplete.
- [AttachEffect](ares-attach-effect.md): ground warhead effects and type-owned
  self effects, lifetime, animation, speed/firepower/armor/ROF multipliers, temporary
  cloakability and forced decloaking. Temporal and airborne victim
  collection remain outside this chain; manual game acceptance remains pending.
- [GenericWarhead](ares-generic-warhead.md)
- [Kill bounties](../src/sim/bounty.rs): `Bounty`, rank-specific `Bounty.Value`
  overrides, `GivesBounty`, on-map `BountyEnablers`, signed wallet changes and
  optional floating credit labels. All readers, state and display hooks are
  compiled only with `ares`; default builds keep the retail kill path.
- [HunterSeeker launch and initial target acquisition](ares-hunter-seeker.md)
- [House survival](ares-house-survival.md)
- [Superweapon building gates](ares-superweapon-building-gates.md)
- [Superweapon money](ares-superweapon-money.md)
- [UnitDelivery](ares-unit-delivery.md)
- [INI Include and inheritance](ini-directives.md). The read-only `asset ini-expand` probe works
  in every build.
- [Extended authored House ally lists](../tools/input_oracle/ares_allies_reader.md).

## Original project and authors

VERA20K is an independent Rust reimplementation. It does not load `Ares.dll`,
import Ares source code or redistribute Ares binaries; the supported behavior
follows the [Ares 3.0 documentation](https://ares-developers.github.io/Ares-docs/)
and observations of the installed DLL.

Ares was created by **pd** in 2007 and is developed by
[The Ares Contributors](https://ares-developers.github.io/Ares-docs/credits.html):

- Current developer: **AlexB**.
- Past developers: **pd** (creator), **DCoder**, **Renegade**, **Electro**,
  **Marshall** and **Graion Dilach**.
- Contributors: **Gluk-v48** (advanced rubble expansion), **Marshall** (prism
  forwarding, Launch Base and the YR Unofficial 1.002 Mini-Patch) and
  **A Mystery Guest** (new powered unit logic).
- Collaborators: **Nighthawk** (project manager), **Steel Mirage**
  (documentation), **Graion Dilach** (user support and ambassador), **Speeder**
  (promo material), **Starkku** (Final Alert support), **Project z Zomboid**
  (Chinese translation) and **Kik0u** (French translation).

Ares does not publish per-key authorship for the features ported here, so this
page credits the project and its developers instead of inventing individual
owners. The official [Ares credits](https://ares-developers.github.io/Ares-docs/credits.html)
list the remaining testers and community members.

简体中文：[扩展支持与原作者](zh-CN/credits.md)。
