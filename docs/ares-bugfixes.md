# Optional Ares gameplay bugfixes

Build with `--features ares`. Ares 3.0 corrects a set of gamemd behaviours
in `Misc/Bugfixes.cpp`, `Misc/Bugfixes.Safeguards.cpp`,
`Misc/Bugfixes.Buffer.cpp` and `Misc/Bugfixes.Perf.cpp`; these take no INI
keys. Default builds keep the original behaviour, bugs included. Each Rust
port sits in the engine owner of the hooked gamemd function, behind
`cfg(feature = "ares")` or the per-game switch `RuleSet::ares` (on when the
game's data uses Ares, see `rules::ares_switch`), with the Ares.dll function
and the gamemd hook cited next to it. Part 2 (the Buffer and Perf rows at
the end of the table) also has `sim::ares_bugfixes_buffer_tests` and
`sim::combat::ares_cliff_tests`.

The Buffer hooks read the Rules type lists through ParseList (`1005B340`
and its instantiations): the 0x800 buffer, comma tokens and **Find, not
FindOrAllocate**, so a list name no registry holds is dropped and no type is
allocated for it. Retail registers its list names, except WCLBOLT2 (so the
Weather Controller shows two bolts) and the systems only DamageParticleSystems
names (SmallGreySmokeSys). Fixtures that relied on the allocation now
register their names, and the oracle tests of the original allocation order
are `cfg(not(feature = "ares"))`.

Focused tests: `sim::ares_bugfixes_tests`, plus Ares cases in the owners'
tests: `techno_ai` (airborne target), `capture_manager_tests` (building
fate), `unit_repair_seek_tests` (repair bay), `failed_yard_repack_tests`
(forced MCV Put), `transport_unload` (gunner), `temporal_tests` (Top layer),
`tesla_charging_tests` (Unit charger), `lifecycle_tests` (wave wall),
`burst` (kept index). Stock oracle tests whose rows encode a fixed bug take
the Ares expectation or skip those rows under `ares`: the burst-index
oracles, `deploy_tests` (Unload return), `recruit_oracle_tests` (rows whose
squared distances overflow), `genetic_converter_tests` (refused MakeInfantry
rows) and the client's `pending_production_tests`.

Status: **ported**; **presentation** (draws, sounds, UI text: not ported
unless noted); **n/a** (crash guard for a state VERA cannot reach, or
DLL/CRT/memory/logging/save plumbing); **equivalent** (VERA already behaves
as Ares in both builds); **residual** (gameplay, but its host is unported);
**superweapon** (handed to the superweapon chain).

| gamemd hook | Ares.dll | What Ares changes | Status |
| --- | --- | --- | --- |
| `0x413FA3` | `1005C2D0` | An aircraft's type `Cloakable=` seeds its stealth ability | ported: `cloak_disguise::type_seeds_stealth`; stock no longer seeds aircraft/buildings |
| `0x414D36` | `1005C190` | An aircraft keeps an airborne Target (AircraftClass::AI dropped it each frame) | ported: `techno_ai` aircraft visit |
| `0x418478`, `0x4186D7`, `0x418826`, `0x418935`, `0x418A44`, `0x418B40` | `1005C2F0`..`1005C390` | Mission_Attack skips the target-cell Scatter (and the destination call) when the shot killed the Target | equivalent: `aircraft_release` scatter and destination skip a missing Target (native dereferences it) |
| `0x41D887` | `1005C3B0` | Airstrike with an empty `SovParaDropInf=` list | n/a: crash guard (reads item 0 of an empty list) |
| `0x4239F0` | `1005C410` | A bouncing anim with `DamageRadius<0`, no Warhead or Damage 0 damages nothing | ported: `anim_class::anim_bounce_contact` |
| `0x424B23` | `1005C3E0` | A MakeInfantry anim whose infantryman cannot be placed ends and UnInits him | ported: `anim_class::anim_make_infantry` |
| `0x441C21`, `0x7387D1` | `1005C4D0`, `1005D530` | `ShakeScreen=0` no longer divides by zero | n/a: screen shake is presentation; VERA divides nothing |
| `0x441D25` | `1005C4C0` | DestroyAnims remap | ported earlier (`destruction_effects`) |
| `0x442CE0` | `1005C4F0` | A building's type `Cloakable=` seeds its stealth ability | ported with `0x413FA3` |
| `0x4430E8` | `1005C470` | Demolish log line with null names | n/a: logging |
| `0x449FF8` | `1005C5D0` | A repacked MCV is Put with `IKnowWhatImDoing` raised; no refund on failure | ported: `world_spawn::finish_undeploy`; residual: the conversion tail is not run on a still-unplaced unit |
| `0x44A5F0` | `1005C550` | Sale survivors: at most nine re-picks after an Engineer, then none (original loops) | ported: `crew_survival::spawn_sale_crew` |
| `0x44A8A2` | `1005C530` | A sold building remembers its survivors are out (NoSurvivor) | ported: `production_sell::sell_stage_one` |
| `0x455E4C` | `1005CB50` | FindRepairBay asks only bays of the unit's Naval kind that are no aircraft pad, and takes only ROGER | ported: `unit_repair_seek` |
| `0x4586D6` | `1005C510` | KillOccupants registers the killer as the occupants' destroyer (vanilla swapped it) | ported: `passenger::garrison_eject::kill_building_occupants` books each occupant as killed by the assaulter ([dormant-stock-rules.md](dormant-stock-rules.md#assaulters)) |
| `0x4692A2` | `1005C630` | The second attacked-by-house event rereads the Tag | equivalent: the tag dispatcher rereads the Tag (`capture_manager`) |
| `0x4693B0` | `1005C650` | A Unit's ElectricAssault shot charges an Overpowerable building too | ported: `tesla_charging::electric_assault_detonation` |
| `0x469467` | `1005C600` | Temporal warps a Foot in the Air or Top layer too | ported: `temporal::temporal_detonation` |
| `0x472198` | `1005C6C0` | No mind-control link to a parasite in limbo | presentation |
| `0x47243F` | `1005C680` | A captured computer building queues Guard, no fate roll | ported: `capture_manager::decide_unit_fate` |
| `0x4748A0` | `1005CC40` | ReadPip without the default-as-index distortion; integer values | presentation: `Pip=`/`OccupyPip=` colours only; not ported |
| `0x4759D4` | `1005CD00` | WriteEdge with an out-of-range edge | n/a: INI writer |
| `0x47F9A4` | `1005C6F0` | Walls drawn in their owner's colour | presentation |
| `0x48A4F9` | `1005D070` | A negative damage packet selects its explosion by magnitude | ported: `detonation_anim::select_with_context` |
| `0x4B5EB0`, `0x4B5F9E`, `0x4B619F` | `1005C8B0`, `1005C890`, `1005C870` | Drop pod trailer smoke without a weapon, report divide by zero, no AtmosphereEntry anim when unset | superweapon: the DropPod locomotor (`0x004B5B70`, `0x004B6040`) has no port yet |
| `0x4C6DDB` | `1005CE40` | An ARCHIVE event does not give a selling object without an archive one | ported: `world_commands::execute_archive_event` |
| `0x4CA0E3` | `1005C960` | Abandoned building production clears the sidebar's pointers to it | n/a: dangling-pointer guard; VERA's placement holds no object pointer |
| `0x4CA437` | `1005C9A0` | FactoryClass::ComputeCRC skips a crashing log | n/a: logging |
| `0x4D5782` | `1005C9F0` | Approach_Target does not chase a passively acquired Target | ported: `foot_approach::approach_foot_target` |
| `0x4D9EE1` | `1005CA30` | A unit is sold at the depot's dock coordinate | ported: `production::foot_can_be_sold` ([ares-reverse-engineering.md](ares-reverse-engineering.md)) |
| `0x4DB87E` | `1005CA50` | A Foot's parasite takes its new Location | ported: `ground_pose::foot_set_location` |
| `0x4FA2E0` | `1005CC10` | SetThreat rejects a negative index | ported: `house_threat::HouseSpatialThreat::adjust` skips a kernel outside its grid in an Ares build, the rebuild's inline kernel too, which natively writes past the grid |
| `0x5005CC`, `0x50067C`, `0x5007BE` | `1005CBD0`, `1005CB30`, `1005CBF0` | The infantry production-request flag no longer falls through to the vehicle flag | ported (presentation): `app::sidebar_projection` cameo prediction |
| `0x50928C` | `1005CC30` | The empty Factory is not deleted (broken destructor) | n/a: Ares leaks the Factory; VERA keeps deleting it (no visible difference) |
| `0x51DF27` | `1005CD50` | A Teleporter's Limbo clears its Teleport locomotor's Marked coordinate | ported: `lifecycle::techno_limbo_with_context` |
| `0x51F716` | `1005CD20` | A deployed man's Unload returns 1 after Guard instead of 450 | ported: `deploy::infantry_mission_unload` |
| `0x52070F` | `1005CDD0` | A cloaked infantryman surfaces to fire only in range | ported: `world_fire` infantry case 9 (no test: fire-path fixture) |
| `0x52BA78` | `1005D7F0` | Animated ParaDrop, Chronosphere, IronCurtain, Detonate, BlueMove and IvanBomb cursors | presentation (cursor timing) |
| `0x52BB64`, `0x53029E`, `0x6BD7E3` | `1005C940`, `1005CE30`, `1005C950` | Base MIX files mount before LANGMD.MIX, and `ares.mix` after the expansion archives, ahead of the masters | ported: `AssetManager::mount_base_mix_files` |
| `0x6BE9BD` | `1005D840` | Ares resources | n/a: DLL/resource plumbing |
| `0x531726`, `0x53173F` | `1005D130`, `1005D140` | Loading screen pips | presentation |
| `0x534F89`, `0x5C9A6E`, `0x5FDDA4`, `0x615BD3`, `0x6BB9DD`, `0x720A61`, `0x720C42`, `0x720DE8`, `0x720F37` | various | Log lines | n/a: logging |
| `0x547043` | `1005E270` | Fatal error for a missing tileset file | n/a: loader diagnostics |
| `0x56017A`, `0x5601E3` | `1005CF10`, `1005CF00` | Options dialog resolution limits | presentation (launcher UI) |
| `0x56BC54` | `1005D4A0` | Threat index `(130*Y)/4 + X/4 + 131` with a 512 bounds check | ported: `house_threat::HouseSpatialThreat::caller_index`, for the `0x0056BC50` callers only (Cell `0x00481884`, Foot `0x004D8657`/`0x004D8664`, House rebuild `0x005094BE`/`0x0050957C`); the inline readers (`0x0056BCD0`, `0x00585F40`, the hierarchy seeds) keep the stock index, so off every fourth row a threat lands away from the cell they read. Affects computer threat maps only |
| `0x5CB0B1` | `1005D020` | Performance timer of zero | n/a: CRT |
| `0x5F6515`, `0x5F6560` | `1005C1A0`, `1005C240` | Squared XY distances in 64 bits, clamped to `INT_MAX` | ported: `util::native_coord::squared_xy_distance` (team recruit/centre, dock search) |
| `0x5F698F` | `1005CE70` | GetCell skips a duplicate lookup | n/a: no behaviour change |
| `0x5F77F0` | `1005CE80` | Pip SHPs freed with `delete` | n/a: memory |
| `0x621B80` | `1005C720` | FillRect clipped to the surface | presentation |
| `0x62A2F8` | `1005CF20` | A parasite whose victim dies two levels up drops from the victim as a bomb | ported: `parasite::parasite_pointer_expired` (no test: parasite fixture) |
| `0x62C2ED`, `0x62CDE8` | `1005CFE0`, `1005CFA0` | Gas and fire particles name their Techno owner and its house as attacker | ported: `particles::gas`, `particles::fire` (no test: attribution only) |
| `0x65D8FB`, `0x65EC4A` | `1005D170` | Reinforcements (actions 7, 80, 107) need an existing house, not a defeated `Player@X` one in multiplayer | ported: `team_script_vm::reinforcements` (no test) |
| `0x671152` | `1005D040` | PrismSupportModifier | ported earlier (`ruleset`) |
| `0x69281E` | `1005C790` | Power-toggle cursor action | presentation: power-toggle mode is unreachable in VERA |
| `0x699C1C` | `1005CA80` | Clears the PKT INI between files | n/a: loader plumbing |
| `0x6B7D50` | `1005D0A0` | CountDockedSpawns counts dead nodes with a finished stopped timer | presentation: only pips, NoSpawnAlt and the digital display read it |
| `0x6E20D8` | `1005D150` | DestroyAttached passes over a receiver that survives instead of looping | ported: `trigger_runtime` action 32 (covered by `destroy_attached_*` tests) |
| `0x6F661D` | `1005D280` | Last health pip red for a dead building | presentation |
| `0x6F7561` | `1005C450` | InRange's planar arm for an Aircraft firer (vanilla tested AircraftType) | ported: `in_range::direct_reaches` |
| `0x6FA2C7` | `1005D330` | Riders of an open-topped transport not drawn behind anims | presentation |
| `0x6FC339` | `1005D200` | A Temporal shot from a Gunner OpenTopped transport needs its own TemporalImUsing | ported: `fire_error` T22 (no test: needs a fire-query fixture) |
| `0x6FCA30` | `1005D2B0` | DecloakToFire: a passenger of a cloaked transport answers CLOAKED | ported: `fire_error` T48 (no test) |
| `0x6FCF53` | `1005D320` | Assign_Target(NULL) keeps CurrentBurstIndex | ported: `concrete_effects::represented_assign_target_admitted` |
| `0x700E47` | `1005D1C0` | A falling or warping-in unit cannot deploy | ported: `deploy::can_deploy_slash_unload` |
| `0x707A47` | `1005D2E0` | LastTarget cleared on expiry | n/a: VERA keeps no LastTarget |
| `0x707B09` | `1005D300` | MindControlledBy cleared on the controller's expiry | n/a: dangling-pointer guard; the capture manager's release clears VERA's id link |
| `0x70CBDA` | `1005D260` | Railgun line damage names the firer's house | ported: `railgun` (no test: the receiver already derives it from the live firer) |
| `0x7119D5`, `0x716D98`, `0x717823`, `0x717855` | `1005D350`, `1005D390`, `1005D3C0` | No-init constructor and palette pointer resets | n/a: save/load plumbing |
| `0x715857` | `1005D380` | Palette limit | ported earlier (`ruleset`) |
| `0x71810D` | `1005D3E0` | A deactivated or powered-off teleporter refuses Move_To | ported: `teleport_movement::teleport_move_to` |
| `0x718871` | `1005D420` | An amphibious or WaterBeach MovementZone floats after a chrono warp | ported: `teleport_chrono::chrono_post_warp_validation` (no test: chrono fixture) |
| `0x71AA52` | `1005D470` | IsAlive cleared before a warped-out object's UnInit | n/a: crash guard for a garrison pointer |
| `0x71AF2B` | `1005D460` | Unwarpable temporal targets | ported earlier (`temporal`) |
| `0x72920C` | `1005D500` | A tunneller without a destination stops | ported: `tunnel_process::tunnel_pre_dig_in` |
| `0x730DB0` | `1005CAF0` | The Guard key orders only the player's own controllable objects | ported: `world_commands::area_guard_key_command` (no test) |
| `0x730EE5` | `1005D110` | The Stop key skips berserk objects | ported: `app::input::dispatch::stop_qualifies` (no test) |
| `0x737CE4` | `1005D5E0` | A ship in its naval yard takes damage | ported: `world_receiver` via `production_spawn::unit_war_factory_contact` (no test) |
| `0x73D81E` | `1005D5B0` | A `Gunner=` transport keeps its last passenger (not one with turrets) | ported: `transport_unload` |
| `0x73F7DD` | `1005D550` | Only an allied building's bib is passable | ported: `object_entry` |
| `0x7440BD` | `1005D610` | A removed Unit leaves its Tank Bunker | ported: `lifecycle::techno_limbo_with_context` (no test) |
| `0x749088` | `1005C9C0` | UnitTracker counter clamped to 512 | n/a: overflow guard of a score counter VERA does not use |
| `0x74A884` | `1005D640` | Voxel anim bounce: own Damage copy per object, next read first, skips | ported: `voxel_anim_effects` |
| `0x75F46E` | `1005D7A0` | Waves destroy walls only with a `Wall=yes` warhead | ported: `Simulation::commit_logic_wave_damage_request` |
| `0x7BB445` | `1005D7D0` | Ellipse drawing on a failed lock | n/a: surface plumbing |
| `0x7272B5` | `1005E2B0` | A trigger naming an unknown house gets none | equivalent: `map::triggers` resolves a negative index to no house |
| `0x41088D` | `1005E230` | Message for a type ID over 24 characters | n/a: diagnostics |
| `0x42461D`, `0x42463A`, `0x48A507`, `0x6CF3CF`, `0x7077EE`, `0x71A922` | — | Ares 0.A hooks absent from the 3.0 DLL | n/a: not in Ares 3.0 |
| `0x44A03C`, `0x739956`, `0x519FF8` | — | 0.A hooks moved out of Bugfixes.cpp in 3.0 | n/a here: now in `Ext/Techno/Hooks.cpp` and `Ext/Infantry/Hooks.Saboteur.cpp` |
| `0x66D55E` | `1005BB00` | Buf_General: the `[General]` paradrop, AnimToInfantry, Secret*, HarvesterUnit, BaseUnit, PadAircraft, Shipyard, RepairBay, WeatherCon* and BridgeExplosions lists read first through ParseList: 0x800 buffer, Find-only (an unknown name is dropped, not allocated); `*ParaDropNum=` through ParseList<int> (`1005B030`), which drops a token that does not convert | ported: `native_processing::allocate_general_references`, `ParadropList::read_counts`; DefaultMirageDisguises' original read still follows and replaces it |
| `0x67062F`, `0x66FA13`, `0x66F7C0`, `0x66F589`, `0x66F34B`, `0x66DD13`, `0x66DB93` | `1005BA30`, `1005BDF0`, `1005BDB0`, `1005BE00`, `1005BDD0`, `1005BE10`, `1005BA40` | Skip the original 0x80 FindOrAllocate reads of those lists (Buf_PPA restores PrerequisiteProcAlternate, Buf_RepairBay NoParachuteMaxFallRate) | ported with `0x66D55E` |
| `0x66BC71` | `1005BA50` | Buf_CombatDamage: Scorches..Scorches4 and SplashList through ParseList | ported: `native_processing` CombatDamage (a Find-only Scorches read allocates nothing; VERA has no Scorches consumer) |
| `0x672B0E` | `1005B830` | Buf_AI: the twenty `[AI]` building lists through ParseList, AIForcePredictionFudge through ParseList<int> | ported: `allocate_ai_references`, `GeneralRules::ai_force_prediction_fudge` |
| `0x7125DF`, `0x713171`, `0x713C10` | `1005BF90`, `1005C020`, `1005C040` | TechnoType DamageParticleSystems, DestroyParticleSystems, Dock, DebrisMaximums, DebrisTypes, DebrisAnims through ParseList at `0x7125DF`; the later Dock and particle-system reads skipped | ported: `process_techno_base`, `ObjectType::debris_maximums` |
| `0x75D660`, `0x75DAE6` | `1005C080`, `1005C0E0` | WarheadType AnimList, DebrisMaximums, DebrisTypes through ParseList at `0x75D660`; the later debris reads skipped | ported: `process_warhead_family`, `WarheadType::debris_maximums` |
| `0x772462` | `1005C0F0` | WeaponType `Anim=` through ParseList | ported: `process_weapon_family` |
| `0x511D16` | `1005BE70` | Country veteran lists through ParseList | ported earlier (`process_house_family`) |
| `0x4750EC`, `0x475107` | `1005BED0`, `1005BEF0` | ReadHouseTypesList (Owner, RequiredHouses, ForbiddenHouses, SecretHouses) reads into the 0x800 buffer | ported: `object_type` house lists; residual: Phobos `SuperWeaponSidebar.*Houses` (same reader) keep 0x80 in an ares+phobos build |
| `0x47527C`, `0x475297` | `1005BED0`, `1005BEF0` | GetAlliesBitfield buffer | ported earlier (`read_houses_list`) |
| `0x7274AF`, `0x7274C8`, `0x727529`, `0x727544` | `1005BED0`, `1005BEF0`, `1005C070` | Trigger `[Events]`/`[Actions]` lines read into the 0x800 buffer | n/a: the INI loader (`0x00525A60`) keeps 511 bytes of a line, so a value never fills the original 512-byte buffer |
| `0x6A9348` | `1005BF00` | Cameo tooltip formatted with `_TRUNCATE` | n/a: buffer guard; VERA builds the tooltip as a String |
| `0x70CAD8` | `1005BF80` | A railgun line that meets the ground no longer rolls the destroyable-cliff collapse there | ported: `railgun::line_damage` |
| `0x489562` | `1005BE20` | DamageArea rolls the impact cell's destroyable cliff (RandomRanged(0,99) < CollapseChance), after the airborne receivers and before the spread walk | ported: `world_receiver::collect_area` |
| `0x48A634`, `0x5FF86E`, `0x422FCC`, `0x550BCA`, `0x62CEC9`, `0x6D7847`, `0x420F40` | `1005E090`, `1005E190`, `1005E020`, `1005E0B0`, `1005E170`, `1005E1B0`, `1005E000` | Detail effects (warhead flash, spotlights, anims, lasers, particles, pixel sparkles, alpha shapes) stay on while the frame rate meets the game speed's target (`RulesExt::DetailsCurrentlyEnabled` `1002A6C0`: `fps >= 60/clamp(GameSpeed,1,6)` before the Rules hysteresis is asked) | presentation: not ported; `app::presentation::detail::DetailState` has no game-speed input |
| `0x551A30` | `1005E0D0` | The Ground layer's single adjacent Y-sort pass becomes a full `std::sort` of one fifteenth of the layer per frame (`CurrentFrame % 15`, a quarter chunk overlap, the last window to the end) | presentation: not ported; only draw order and the hashed Ground order change, and no gameplay reader depends on that order (crate recipients) |
| `0x5F6612` | `1005E140` | UnInit announces the object's expiry only when Limbo fails; a successful Limbo's own Destroy(1) broadcast is the only one | residual: VERA keeps both broadcasts and their order around Limbo's steps; a difference shows only where a listener treats the repeat or the earlier broadcast differently (the dead-building anim slot); separate lifecycle mechanism |
| `0x725A1F` | `1005E040` | A `[General] Behind=` anim's expiry is not announced to the anim listeners | n/a: only its owner holds a Behind anim, and the owner drops it before destroying it (`techno_ai` behind indicator) |
| `0x6F90F8`, `0x707EEA`, `0x70133E`, `0x458E1E` | `1005E210`, `1005E1D0`, `1005E1F0`, `1005E070` | `max`/`min` macros no longer call GetWeaponRange / GetFoundationWidth twice | n/a: the same results (`70133E`'s second passenger scan recomputes the same minimum) |
