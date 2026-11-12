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

- [Ground, airborne and building EMP](ares-emp.md) (every Foot mover but Tunnel, human or AI,
  miners, falling and mind-controlled victims, every EMP-prone building with its factory, gap,
  superweapon, PowersUnit, clone, Prism and undeploy effects, the Tank Bunker exemption,
  and the player's miner and base warnings; team members and spawners in
  [warhead residuals](ares-warhead-residuals.md))
- [Water and land type conversion](../src/sim/game_entity/type_conversion.rs):
  `Convert.Water` applies over water or beach; `Convert.Land` applies over land
  and bridges. It uses the same in-place conversion owner as script action 68,
  preserves the object identity and ignores a target equal to the current type.
- [Custom armor](ares-custom-armor.md)
- [Supplemental string tables](../tools/rules_oracle/ares_csf.md), shared by
  game loading and default `asset csf-get` / `csf-grep` queries.
- [Per-type SHP building palettes](ares-building-palettes.md)
- [PCX sidebar icons](ares-pcx-cameos.md) (`CameoPCX`, `AltCameoPCX`, `SidebarPCX`)
- [EVA voice tags and Side selection](ares-eva.md)
- [Per-type promotion sounds](ares-promotion-sounds.md)
  (`Promote.VeteranSound`, `Promote.EliteSound`)
- [Subterranean dig effects](ares-subterranean.md) (`DigIn`, `DigOut`,
  `DigInSound`, `DigOutSound`) on the stock Tunnel locomotor, which every
  build ports for mods that select it
- [Finite infantry ammo](../src/sim/ammo.rs): Ares' Infantry reload dispatch
  runs through the shared Techno reload owner before the scatter/fire suffix,
  and per-weapon `Ammo` shares one count across admission, debit and reload
  (`Update_Reload 0x10047980`, `Fire_Ammo 0x10047910`, `CanFire_Ammo
  0x100479B0`, debit `0x100441F0`).
- Warhead house filtering: `AffectsOwner` (nullable) and `AffectsEnemies`
  extend the shared receiver's ally gate to animation-only damage and
  ordinary hits alike
  ([reader](../src/rules/ares_warhead.rs), installed Ares
  `TechnoClass_ReceiveDamage_Abilities 0x1004A950`, `CanTargetHouse
  0x10051B70`).
- [Fourth and later Side defaults](ares-side-defaults.md): crew, engineer,
  technician, disguise, survivor divisor, AI base defences and counts,
  paradrop lists/plane, parachute animation, tooltip and message colors, and
  the registered-side AI trigger token. Score screens, dialog backgrounds and
  the Hunter Seeker launch remain separate.
- [Country presentation](ares-country-presentation.md): `File.Flag`,
  `MenuText.Status`, `File.Taunt`, `ListIndex`, random/AI selection weights,
  ParentCountry inheritance, and the shell/Co-Op flag and status consumers.
  Observer art is retained but has no renderer yet.
- [Paradrop overrides](ares-country-paradrop.md): the superweapon's own
  `ParaDrop.*` planes (vehicles too), then the country's `ParaDrop.Types`,
  `.Num`, `.Aircraft` and `Parachute.Anim` in front of the SideExt values.
- [Country AI power plants](ares-country-powerplants.md): `AI.PowerPlants=`
  and the draw among the plants the AI can build when it splices one.
- [Generic production prerequisites](ares-prerequisites.md), with `Prerequisite.Lists`,
  `Prerequisite.Negative`, `Prerequisite.RequiredTheaters` and `Prerequisite.StolenTechs`.
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
- [Type selection groups](https://ares-developers.github.io/Ares-docs/new/typeselect.html):
  retained `GroupAs` keys share type IDs' case-insensitive namespace in T tap,
  held-T click/band and additive removal. `General/TypeSelectUseDeploy` controls
  automatic deployed-form seeds. Without `ares`, both tags are ignored and
  retail selection remains active.
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
- [Weapon beam, bolt and wave visuals](ares-weapon-visuals.md): `Beam.*`,
  `Bolt.Color1..3`, `Bolt.ParticleSystem`, `Wave.Color`, `Wave.Intensity`,
  `Wave.IsHouseColor`, `Wave.ReverseAgainst*` and `IsDetachedRailgun`. A wave
  keeps its own weapon for range and cell damage. TS laser waves
  (`Wave.IsLaser`, `Wave.IsBigLaser`) remain outside this chain.
- Warhead damage modifiers and effects ([reader](../src/rules/ares_warhead.rs)):
  `RelativeDamage.*`, `Culling.*`, `AllowZeroDamage`, `Damage.Deployed`,
  `DamageAirThreshold`, `CellSpread.MaxAffect`, `PreventScatter`, `Malicious`,
  `BridgeAbsoluteDestroyer`, `DeathWeapon.Suppress*`, `UnitLost.Suppress`,
  `DieSound.Override`, `VoiceDie.Override`, `Flash.Duration`,
  `IronCurtain.Flash`, `Temporal.HealthFactor`, `Temporal.WarpAway`,
  `IronCurtain.Duration`/`.Cap` with the per-type `IronCurtain.Modifier` and
  `ForceShield.Modifier`, `MindControl.Permanent`, `DisableWeapons.Duration`,
  `Sonar.Duration`, `InfDeathAnim` and the `IonCannon.*`/`Ripple.Radius`
  blast. Airborne victims of these effects and the building `EVA.Lost`
  announcement remain outside this chain.
- [Building infiltration and sabotage](ares-infiltration.md): `SpyEffect.*`,
  `Saboteur`, `ImmuneToSaboteurs` and spy veterancy. Revealing the owner's
  radar sight and custom spy cursors remain outside this chain.
- [Cloak, PoweredBy, radar jammers and sensor arrays](ares-cloak-power.md)
- [Superweapon common keys, Battery, Protect and SonarPulse](ares-superweapon-common.md):
  charge drain, `SW.AlwaysGranted`, `SW.Unstoppable`, the EVA, message and
  sidebar text keys, and `SidebarPalette`. DropPod and the retail-type options
  (Chronosphere, Dominator, Lightning and others) remain separate.
- [Veterancy, academies, build time and cloning](ares-veterancy.md):
  `Promote.*`, `Experience.*`, `Academy.*`, `BuildTime.*`,
  `FactoryPlant.Multiplier`, `Cloneable`, `ClonedAt`, `ClonedAs` and
  `CloningFacility`.
- [Ivan bombs, abductors and weapon extras](ares-ivan-bomb-abductor.md):
  `IvanBomb.*` from the planting weapon, `Abductor.*` with `ImmuneToAbduction`,
  `ApplyDamage`, `ProjectileRange` and the player's Detonate
  (`IvanBomb.CanDetonate*`). `Cursor.Attack*` remains outside this chain.
- [Building types](ares-building-types.md): advanced rubble (`Rubble.*`),
  `EngineerRepairable`, urban combat and raidable garrisons (`UC.*`,
  `Bunker.Raidable`, `CanBeOccupiedBy`), `IsPassable`, `EngineerDamage`,
  `EngineerAlwaysCaptureTech`, and the second part's sell, return, degrade,
  power-toggle and dock keys. `SolidHeight`/`SolidLevel` line-of-fire
  blocking remains outside this chain.
- [Techno types](ares-techno-types.md): custom missiles (`Missile.*`),
  reload and empty-ammo keys, `Gattling.Cycle`, crush damage, berserk ROF,
  passive acquisition, `CivilianEnemy`, damage smoke and sparks,
  `CrashSpin`, `PassengerTurret`, self-healing rates and combat delay, fall
  rates, `AirRate`, `NoManualUnload`/`NoManualEnter`, the Desolator, cow and
  bomb roles, drain keys and building reload.
- [Superweapon retail types](ares-superweapon-types.md): Ares handlers for
  the Chronosphere and ChronoWarp (`Chronosphere.*`, `Chronoshift.*`), Nuke
  (`Nuke.*`, `NukeFlash.Duration`, `PreImpactAnim`), Psychic Dominator
  (`Dominator.*`) and Genetic Mutator (`Mutate.*`), with `Light.*`. Building
  chronoshift, the SpyPlane and the Lightning Storm (`Lightning.*`,
  `LightningRod.Modifier`) and EMPulse with the stock EMP cannon mission
  follow in parts 2 to 4, and part 5 is the DropPod launch with its AI target.
- [Prism forwarding](ares-prism-forwarding.md): `PrismForwarding.*` networks
  with support weapons, chain and network limits, damage bonus, charge delay,
  allied feeds and support breaking. Radiation-beam and electric-bolt support
  visuals remain outside this chain.
- [Trigger events and actions](ares-triggers.md): events 62..88 with
  TechTypeExists and the Player@X lookups, their damage, kill, spotlight,
  abduction and superweapon raise sites, and actions 98 and 146..149.
  The reverse-engineering events are in
  [reverse engineering](ares-reverse-engineering.md).
- [Tiberium, ore and harvesters](ares-tiberium.md): TiberiumExt heal,
  explosion and debris keys, tiberium damage and healing toggles,
  `TiberiumProof`/`Remains`/`Spill`, the explosive harvester, harvester and
  slave-miner scans, `RandomCrateMoney` and `RepairStopOnInsufficientFunds`.
  Refinery storage (`Refinery.UseStorage`), chain-reacting overlays and the
  visceroid spawn follow; the visceroid merge needs a stock port first.
- [Line of fire and bullet types](ares-line-of-fire.md): the Ares obstacle
  walk with `SolidHeight`, `SolidLevel` and `AlliedSolidTransparency`,
  BulletTypeExt `SubjectToBuildings`, `SolidLevel`, `SubjectToTrenches` and
  `BallisticScatter.*`, inviso impacts and shrapnel weapons. Airburst splits
  are in [combat and simulation hooks](ares-combat-sim-hooks.md); parachuted
  bullets remain outside this chain.
- [Firestorm walls](ares-firestorm.md): the Firewall superweapon (`Type=Firestorm`),
  `Firestorm.Wall` sections with their animations, burning, passability, bullet
  interception and sale gate, the `Firestorm*` globals and trigger actions 92/93.
  The zone rebuild after a toggle and the trench lines are in
  [building hooks](ares-building-hooks.md); no Ares function raises the
  FirewallToggle event.
- [Mouse cursors](ares-cursors.md): the `[MouseCursors]` list with the nine Ares
  cursors, every `Cursor.*` key of techno, weapon, building and superweapon types,
  and the action overrides on the tactical map and the radar.
- [Gameplay bugfixes](ares-bugfixes.md): the Misc/Bugfixes hooks, each listed as
  ported, presentation, equivalent or not applicable, including the Find-only
  type-list readers (a list name no registry holds is dropped).
- [Anim and particle extensions](ares-anim-particles.md): `Damage.Delay`, the anim
  `Weapon`, `SpawnsParticle.Range*`, gas `DamageRange` and ParticleSystemExt, Ares'
  own Smoke, Spark and Railgun simulation and its after-Air draw.
- [Remaining Techno hooks](ares-techno-hooks.md) and
  [Building, House, Infantry and SWType hooks](ares-building-house-hooks.md): per-hook
  dispositions, including healers, gap and spotlight hooks, rank immunities, the
  AI base plan and unit-chooser fixes, power-toggle and EMP power, and infantry
  death-anim owners.
- The last ExtData keys of `report/ini_coverage.tsv`. The
  [RulesExt general keys](ares-techno-hooks.md#rulesext-general-keys) and
  [presentation keys](ares-techno-types.md#presentation-keys) list the wired
  consumers (airstrike voices, `EnemyUIName`, gate and slam sounds, bullet
  `AnimLength`, `InfantryElectrocuted`, `EnemyWrench`, `DiskLaserAnimEnabled`,
  `AIFriendlyDistance`, `StartInMultiplayerUnitCost`, `CameoPalette`) and the
  keys whose mechanism is still missing (see [remaining work](ares-remaining.md)).
- [Combat and simulation hooks](ares-combat-sim-hooks.md): the Ares cell-spread
  walk, the EMP construction filter, `LaserThickness` and TS laser waves
  (`Wave.IsLaser`, `Wave.IsBigLaser`), chronoshift validation, a building's
  free units, the factory hand-off, the Droppod team plane, bullet `Splits` and
  airburst, `StartInMultiplayer.Types` and `.WithConst`, the aircraft `Dock=`
  click, HunterSeeker flight and terrain fire.
- [Building hooks](ares-building-hooks.md): the `Wall=` any-wall query, gates on
  walls, trench lines and trench traversal (event 0x60), custom foundation
  pips, Ares tunnels (`[TunnelTypes]`, `Tunnel=`), `Passengers.Allowed` and
  `Passengers.Disallowed`, and the firestorm zone rebuild.
- [Reverse engineering, unit selling and the Secret Lab](ares-reverse-engineering.md):
  grinder reverse engineering with its trigger events and
  `SpyEffect.UndoReverseEngineer`, `UnitSell`/`Unsellable` sales on a depot,
  the ProduceCash rework with `ProduceCashDisplay`, and `SecretLab.*`.
- [Transports and payloads](ares-transports.md): `InitialPayload.*`,
  `Convert.Deploy`, Hospital/Armory eviction on sale and capture, Carryall
  totes (`Carryall.Allowed`, `Carryall.SizeLimit` and the 3.0 drop hooks),
  `Passengers.Allowed`/`.Disallowed` on aircraft transports, and
  `Assaulter.Level` over the stock assault
  ([dormant-stock-rules.md](dormant-stock-rules.md#assaulters)).
- [Warhead, AI and techno residuals](ares-warhead-residuals.md): airborne
  victims of the curtain, EMP and AttachEffect, PoweredBy team, spawn and slave
  updates, `TeamRetaliate`, `AIInnerBase`, `EVA.Lost`, EMP on Fly and Jumpjet
  victims, and the `CanMakeStuffUp` draws.
- [Sidebar and interface hooks](ares-sidebar-ui-hooks.md): no cameo limit, the
  Ares cameo insertion and top row, and Shift-click batch production. The CSF
  loader hooks are deferred.
- [Radiation types](ares-radiation.md): Ares 3.0 never loads `[RadiationTypes]`;
  its snow-theater hook is the only live radiation hook.
- [Random-map generator](ares-rmg.md): Archipelago and Desert in the dialog, the
  Urban Areas box and Surprise Me, `GenerateUrbanAreas` and the `[Urban]` lists,
  urban areas with roads, buildings, foots and plazas, and the bridge, road-end
  and `Medians` fixes.
- [Hooks skipped on purpose](ares-skipped-hooks.md) and
  [remaining work](ares-remaining.md).
- [INI Include and inheritance](ini-directives.md). The read-only `asset ini-expand` probe works
  in every build.
- [INI syntax](ares-ini-syntax.md): `+=` keys, `[Child]:[Base]` sections and later-wins
  repeated keys in every INI load, with included files loaded into the INI that lists them.
- [Extended authored House ally lists](../tools/input_oracle/ares_allies_reader.md).

## VERA20K porting credits

The Rust ports are individual VERA20K contributions; GitHub usernames name
the merged pull requests that carried each chain:

- **zzzzzzh123** — the opt-in build capability itself, EMP (with its
  powered-defence arm), water/land type conversion and script 68, custom
  armor, supplemental string tables, per-type building palettes, EVA voice
  tags, Country loading art and text, generic production prerequisites,
  `FactoryOwners`, vehicle drivers and KillDriver, extended IFV modes, type
  selection groups, NoManualFire, the Ares AttachEffect chain (ground
  warhead effects, attached armor/firepower/speed/reload, self-attached
  cloak effects, forced decloaking, animation warheads and assets, warhead
  house filtering), HunterSeeker, House survival, zero-fee and
  initial/virtual superweapon charge, extended House ally lists, infantry
  reload and per-weapon ammo costs, per-type promotion sounds, and the kill
  bounties.
- **baixiong443** — GenericWarhead, the superweapon building gates,
  single-stage and two-stage superweapon fees and rewards, UnitDelivery,
  and INI include and inheritance (with ra2-mono's `IniFile` adaptation).
- **1372704087** — fourth-Side defaults through SideExt, country
  presentation, country paradrop overrides and country AI power plants, and
  the inherited-INI expansion budget.
- **hrusten** — the Ares superweapon mask/AI-mode native reader pass, the
  bounty label's native slot, and the radiation decay-clock repair that
  survives loading a game.
- **EncoreAlpha** — the retained `EMPulseWarhead` rules state.
- **libi** — weapon beam, bolt and wave visuals, the warhead damage
  modifiers and effects (curtain duration, permanent control, weapon lock,
  ion cannon), building infiltration and sabotage, cloak, PoweredBy, radar
  jammers and sensor arrays, the common superweapon keys with Battery,
  Protect and SonarPulse, veterancy, academies, build time and cloning,
  Ivan bombs and abductors, the building and techno type keys, and the
  Chronosphere, Nuke, Dominator, Genetic Mutator, SpyPlane and Lightning
  Storm superweapon types with building chronoshift, prism forwarding,
  trigger events and actions, tiberium and harvesters, the line of fire, EMPulse,
  firestorm walls, mouse cursors, the Misc/Bugfixes hooks, anim and particle
  extensions, the remaining Techno, Building, House and Infantry hooks, the
  last ExtData keys, and the combat, building, tunnel, DropPod,
  reverse-engineering, transport, refinery-storage, Secret Lab,
  prerequisite-list and sidebar chains.

## Original project and authors

VERA20K is an independent Rust reimplementation. It does not load `Ares.dll` or
redistribute Ares binaries. The supported behavior follows the
[Ares 3.0 documentation](https://ares-developers.github.io/Ares-docs/), the
installed DLL and Ares' source code. Parts of the `ares` build are translated
from that source, including source restored from the Ares 3.0 DLL, and keep
Ares' copyright notice and
[license](https://github.com/YuriPlanet/vera20k/blob/main/LICENSES/Ares.txt).

This product includes software developed by the Ares Contributors.

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
