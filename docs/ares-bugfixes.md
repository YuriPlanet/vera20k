# Optional Ares gameplay bugfixes (part 1)

Build with `--features ares`. Ares 3.0 corrects a set of gamemd behaviours
in `Misc/Bugfixes.cpp` and `Misc/Bugfixes.Safeguards.cpp`; these take no INI
keys. Default builds keep the original behaviour, bugs included. Each Rust
port sits in the engine owner of the hooked gamemd function, behind
`cfg(feature = "ares")` or `rules::ARES_ENABLED`, with the Ares.dll function
and the gamemd hook cited next to it. `Bugfixes.Buffer.cpp` and
`Bugfixes.Perf.cpp` are part 2.

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
| `0x4586D6` | `1005C510` | KillOccupants registers the killer as the occupants' destroyer (vanilla swapped it) | residual: `KillOccupants(killer)` is reached only by the `Assaulter=` arm, which VERA does not represent; no retail assaulter |
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
| `0x4D9EE1` | `1005CA30` | A unit is sold at the depot's dock coordinate | residual: `FootClass::CanBeSold @ 0x004D9E70` (the player's sell cursor over a unit at a depot) is unported |
| `0x4DB87E` | `1005CA50` | A Foot's parasite takes its new Location | ported: `ground_pose::foot_set_location` |
| `0x4FA2E0` | `1005CC10` | SetThreat rejects a negative index | equivalent: `house_threat` refuses writes outside its grid |
| `0x5005CC`, `0x50067C`, `0x5007BE` | `1005CBD0`, `1005CB30`, `1005CBF0` | The infantry production-request flag no longer falls through to the vehicle flag | ported (presentation): `app::sidebar_projection` cameo prediction |
| `0x50928C` | `1005CC30` | The empty Factory is not deleted (broken destructor) | n/a: Ares leaks the Factory; VERA keeps deleting it (no visible difference) |
| `0x51DF27` | `1005CD50` | A Teleporter's Limbo clears its Teleport locomotor's Marked coordinate | ported: `lifecycle::techno_limbo_with_context` |
| `0x51F716` | `1005CD20` | A deployed man's Unload returns 1 after Guard instead of 450 | ported: `deploy::infantry_mission_unload` |
| `0x52070F` | `1005CDD0` | A cloaked infantryman surfaces to fire only in range | ported: `world_fire` infantry case 9 (no test: fire-path fixture) |
| `0x52BA78` | `1005D7F0` | Animated ParaDrop, Chronosphere, IronCurtain, Detonate, BlueMove and IvanBomb cursors | presentation (cursor timing) |
| `0x52BB64`, `0x53029E`, `0x6BD7E3`, `0x6BE9BD` | `1005C940`, `1005CE30`, `1005C950`, `1005D840` | MIX bootstrap and Ares resources | n/a: DLL/resource plumbing |
| `0x531726`, `0x53173F` | `1005D130`, `1005D140` | Loading screen pips | presentation |
| `0x534F89`, `0x5C9A6E`, `0x5FDDA4`, `0x615BD3`, `0x6BB9DD`, `0x720A61`, `0x720C42`, `0x720DE8`, `0x720F37` | various | Log lines | n/a: logging |
| `0x547043` | `1005E270` | Fatal error for a missing tileset file | n/a: loader diagnostics |
| `0x56017A`, `0x5601E3` | `1005CF10`, `1005CF00` | Options dialog resolution limits | presentation (launcher UI) |
| `0x56BC54` | `1005D4A0` | Threat index `(130*Y)/4 + X/4 + 131` with a 512 bounds check | residual: Ares changes the index of the `0x0056BC50` callers (`0x00481884`, `0x004D8657`, `0x005094BE`, `0x0050957C`) only, not the inline readers (`0x0056BCD0`, `0x00585F40`); VERA's one `house_threat` index would need a per-caller split. Affects computer threat maps only |
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
| `0x72920C` | `1005D500` | A tunneller without a destination stops | residual: no Tunnel locomotor port; no retail type uses it |
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
