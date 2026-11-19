# Optional Phobos compatibility

VERA20K can build in partial, Rust-native support for some Phobos extensions. It does not load the
Phobos DLL and is not general mod support.

## The Phobos switch

Four of these extensions are in every build, the release build too, and play in a game whose rules,
mode or map hold one of their keys. Such a key turns Phobos on for that game, as loading Phobos.dll
would, and the game's log names it, e.g.
`Phobos plays in this game: [GlobalControls] ForbidParallelAIQueues.Building is a Phobos key`.
`AllowParallelAIQueues`, which Ares reads too, turns neither extension on. Retail data holds no
such key, so a retail game plays as the original. A save, a replay and every player in an online
match reach the same answer from the same rules, mode and map
([Networking](Networking.md)). The four are:

- [Parallel AI queue limits](phobos-parallel-ai-queues.md)
- [Skirmish AI dehardcoding](phobos-ai-dehardcode.md)
- [AI spare power](phobos-power-surplus.md)
- [Target zone scan](phobos-target-zone.md)

The rest needs a build with the `phobos` feature, which plays Phobos in every game:

```sh
cargo run --release --bin vera20k --features phobos
```

The capability is independent of Ares and NPatch: a Phobos build needs neither. Every build holds
Ares, which a game's data switches on ([Ares](ares.md)), and Phobos combines with NPatch as
`--features phobos,npatch`. Every implemented Phobos chain must keep working
when Ares plays or the other capabilities are compiled in — the compatibility rule and the pattern for two
extensions touching one native site are in
[NPatch placeholder](npatch.md#cross-extension-compatibility).

Phobos builds identify themselves in the window title, the startup log, and the build description
drawn at the tactical view's top right during a match. That description is Phobos's own text: the
product name, the version and the build-type warning derived exactly as `Phobos.version.h` derives
them, so a Phobos build prints

```
Phobos v0.5.0.0-alpha1. Please test the build before shipping.
```

in red on black, ten pixels in from the view's right edge — the same string, site and colors as
Phobos's `GScreenClass::DrawText` hook. Saves retain the build capability and cannot load in the
other mode, even with identical retail INIs.

Supported extensions:

- [Custom radiation types](phobos-custom-radiation.md)
- [Shields](phobos-shields.md) (damage absorption, shield bar, shield tint, break, respawn and self-healing)
- [Custom ore spawners](phobos-ore-spawners.md)
- [Custom gather animations](phobos-harvester-gather-animations.md)
- [Ore-stage animation hiding](phobos-hide-if-no-ore-threshold.md)
- [Superweapon techno gates](phobos-superweapon-technos.md)
- [Superweapon chaining and link groups](phobos-sw-chaining.md) (`SW.Next*` chained launches with weights/rolls and the two ignore flags, and `SW.Link*` grant/ready/reset groups)
- [Limbo-delivered buildings](phobos-limbo-delivery.md) (`LimboDelivery.*` buildings created on launch with a `LimboID`, and `LimboKill.*` removing them by id and house mask)
- [Unlimbo detonation](phobos-unlimbo-detonate.md) (`UnlimboDetonate` brings back a `LimboLaunch=` firer at its bullet's detonation, with `.ForceLocation`, `.KeepTarget` and `.KeepSelected`; 4 keys)
- [Digital display](phobos-digital-display.md) (numeric health/ammo/cargo/timer values near the health bar)
- [Health-bar switches](health-bars.md#phobos-type-switches) (`HealthBar.Hide=`, `.HidePips=`, `.Permanent=` and `.Permanent.PipScale=`)
- [Shared transport ammo](phobos-shared-ammo.md) (`Ammo.Shared=` and `Ammo.Shared.Group=` feed a reloading open-topped transport's rounds to its passengers)
- [Credits indicator step limit](phobos-credits-indicator.md) (`CreditsIndicator.MaxStep=` and `.Smooth=` shape the sidebar balance roll)
- [Passenger deletion](phobos-passenger-deletion.md) (`PassengerDeletion.*` counts down and deletes a transport's passenger, optionally paying Soylent)
- [Warhead trigger conditions](phobos-warhead-triggers.md) (`AffectsAbovePercent`, `AffectsBelowPercent`, `AffectsVeterancy` and `AffectsInvokerOnly.*` on a detonation's targets; `Shrapnel.ObeyWarheadTriggerConditions` gates a bullet's whole detonation body, default yes)
- [Overload tables](phobos-overload.md) (`Overload.Count`, `.Damage`, `.Frames`, `.DeathSound`, `.ParticleSys` and `.ParticleSysCount` for mind-control managers)
- [Grinding gates](phobos-grinding.md) (`Grinding.AllowAllies`, `.AllowOwner`, `.AllowTypes`, `.DisallowTypes`, `.Sound`, `.PlayDieSound`, `.Weapon` and `.Weapon.RequiredCredits`)
- [Harvester area guard](phobos-harvester-guard.md) (`Harvester.CanGuardArea` and `.RequireTarget`: a human harvester holds Area Guard, optionally only with a threat in range)
- [Kill weapons](phobos-kill-weapon.md) (`KillWeapon`, `.OnFirer`, their `AffectsHouse`/`AffectsTarget` gates, and `SuppressKillWeapons`)
- [Ranking overrides](phobos-ranking.md) (campaign `Ranking.ParTime*` and title/message keys in the scenario's MISSIONMD section)
- [Gattling rate-down](phobos-rate-down.md) (`RateDown.Delay`, `.Reset`, `.Cover.Value` and `.Cover.AmmoBelow`)
- [Repair-service overrides](phobos-units-repair.md) (`Units.RepairRate`, `.RepairStep`, `.RepairPercent` and `.UseRepairCost` on hospitals, depots and `UnitReload` buildings)
- [Placement adjacency](phobos-adjacent.md) (`Adjacent.Allowed`, `.Disallowed`, `.Disallowed.Prohibit` and `.ProhibitDistance`)
- [Drain money display](phobos-drain-money-display.md) (`DrainMoneyDisplay.*` flying credit labels for a DrainWeapon link)
- [TransactMoney warhead keys](phobos-transact-money.md) (`TransactMoney`, its `Display.*` label and the `0x6FC339` insufficient-funds fire gate)
- [Jumpjet tilt](phobos-jumpjet-tilt.md) (`JumpjetTilt` and the four `JumpjetTilt.*` factors pitch and roll an airborne jumpjet body)
- [Forced weapon selection](phobos-force-weapon.md) (`ForceWeapon.*` / `ForceAAWeapon.*` force the SelectWeapon slot by target state, range and class)
- [Multi-weapon selection](phobos-multiweapon.md) (`MultiWeapon`, `.SelectCount` and `.IsSecondary` admit and order weapon slots past the stock two)
- [Alternate FLH gating](phobos-alternate-flh.md) (`AlternateFLH.OnTurret` and `.ApplyVehicle` choose hull/turret space and the open-topped carrier port)
- [Deploy ammo](phobos-ammo-deploy.md) (`Ammo.AddOnDeploy`, `.DeployUnlock*` and `.AutoDeploy*` shape a vehicle deployer's ammo and deploy admission)
- [Penetrates transport](phobos-penetrates-transport.md) (`PenetratesTransport.*` lets a warhead damage a transport's passengers)
- [Warp-in weapon](phobos-warp-in-weapon.md) (`WarpInWeapon`, `WarpInMinRangeWeapon` and `.UseDistanceAsDamage` detonate at a teleport's arrival)
- [Warp animation lists](phobos-chrono-warp-anims.md) (`WarpIn`, `WarpOut` and `Chronoshift.WarpIn/.WarpOut` pick a random endpoint animation before the `[General]` fallback)
- [Missile cruise and takeoff](phobos-missile.md) (`Missile.Cruise`, `.TakeOffSeparation` and `.TakeOffAnim` on a rocket-flying AircraftType)
- [Drop pods](phobos-drop-pod.md) (`DropPod.*` twelve per-type keys: descent angle/speed/height, trailer cadence and attachment, air/landing weapon, puff and ground anims)
- [Extended aircraft missions](phobos-extended-aircraft-missions.md) (`ExtendedAircraftMissions` master switch and the `.SmoothMoving`/`.EarlyDescend`/`.RearApproach`/`.FastScramble`/`.UnlandDamage` per-type keys; smooth Move arrival, dock-return approach and descent, helipad scramble on destruction, and unlandable damage)
- [Aircraft strafing](phobos-aircraft-strafing.md) (`Strafing.Shots`, `.SimulateBurst`, `.UseAmmoPerShot`, `.TargetCell` and `.EndDelay` shape a strafing round, its end delay and the spy plane's photo cap)
- [Vertical aircraft bullets](phobos-vertical-aircraft-fix.md) (`Vertical.AircraftFix` keeps a `Vertical=` bullet's FireAt velocity instead of the aircraft ROT course; bullet `Nullable` + `[General]` default yes)
- [Powers-up host list](phobos-powers-up.md) (`PowersUp.Buildings` lets one upgrade serve several hosts and `PowersUp.Owner` filters the installer)
- [Placement preview](phobos-placement-preview.md) (building artwork and grid translucency while placing)
- [Sidebar positions](phobos-sidebar-gdi.md) (per-side GDI/NOD sidebar coordinates)
- [Infantry auto deploy](phobos-infantry-auto-deploy.md) (`InfantryAutoDeploy` for AI GI-like infantry)
- [Fly wobble switch](phobos-fly-no-wobbles.md) (`FlyNoWobbles` for `Locomotor=Fly`, per type and globally)
- [Centred pause-menu background](phobos-center-pause-menu.md) (`uimd.ini [Sidebar] CenterPauseMenuBackground`)
- [Building production queue](phobos-building-queue.md) (queue buildings and start the next after placement)
- [Skirmish AI dehardcoding](phobos-ai-dehardcode.md) (last-legs sale/hunt switches and MCV deploy regroup)
- [Parallel AI queue limits](phobos-parallel-ai-queues.md) (per-category and per-type AI clone forbids)
- [AI spare power](phobos-power-surplus.md) (`[AI] EnablePowerSurplus` keeps `PowerSurplus` power spare in AI base planning)
- [Target zone scan](phobos-target-zone.md) (`TargetZoneScanType` lets hunting units pick targets outside their movement zone)
- [Unlimited skirmish colours](phobos-skirmish-unlimited-colors.md) (spawner colour indices address any `[Colors]` entry)
- [Superweapon sidebar](phobos-superweapon-sidebar.md) (left-edge superweapon cameos, toggle and positional hotkeys)
- [Prone movement speed](phobos-prone-speed.md) (`ProneSpeed` for crawling and non-crawling prone infantry)
- [Type select for buildings](phobos-building-type-selectable.md) (`BuildingTypeSelectable` for every building type)
- [Power-delta indicator](phobos-power-delta-indicator.md) (`PowerDelta.*` net-power text beside the credits counter)
- [Burst with no delay](phobos-burst-no-delay.md) (`Burst.NoDelay=` empties a burst in one frame)
- [Per-shot burst delays](phobos-burst-delays.md) (`Burst.Delays=` gives each burst shot its own rearm delay, on any class)
- [Burst within the firing sequence](phobos-burst-sequence.md) (`Burst.FireWithinSequence=` fires an infantry burst's later shots at cumulative-delay frames and arms a full rearm past the sequence; 1 key)
- [Harvester counter](phobos-harvester-counter.md) (`HarvesterCounter.*` working/total harvesters, `Harvester.Counted=`)
- [Weeds counter](phobos-weeds-counter.md) (`WeedsCounter.*` the House's stored weed total beside the credits counter)
- [AutoDeath](phobos-auto-death.md) (`AutoDeath.Behavior=kill|vanish|sell`, the condition arms and timers, `AutoDeath.VanishAnimation`)
- [Shrapnel admission](phobos-shrapnel.md) (`Shrapnel.AffectsGround=` / `Shrapnel.AffectsBuildings=` gate whether a bullet spawns its shrapnel)
- [Cluster scatter distance](phobos-cluster-scatter.md) (`ClusterScatter.Min=` / `ClusterScatter.Max=` set each cluster successor's distance from the impact)
- [Timer blink colour](phobos-timer-blink-color.md) (`[AudioVisual] TimerBlinkColorScheme=` names a `[Colors]` entry)
- [Missing cameo](phobos-missing-cameo.md) (`[AudioVisual] MissingCameo=` for types with no cameo art)
- [Power plant enhancers](phobos-power-plant-enhancer.md) (`PowerPlantEnhancer.*` output boosts, `PowerPlant.DamageFactor` and placement range rings)
- [Designator range rings](phobos-designator-range.md) (`ShowDesignatorRange` in `[Phobos]`, `[AudioVisual]` and per superweapon: ring the armed superweapon's `SW.Designators`/`SW.Inhibitors` technos at their `DesignatorRange`/`InhibitorRange`; 3 keys)
- [Overpower thresholds](phobos-overpower.md) (`Overpower.KeepOnline` / `.ChargeWeapon` per building type and `ElectricAssaultLevel` per warhead drive the Tesla-charger power bypass and the overpowered weapon latch; 3 keys)
- [Iron-curtained targeting](phobos-iron-curtain-targeting.md) (`CanTarget.IronCurtained` / `AutoTarget.IronCurtained` per weapon plus the `[CombatDamage]` human/AI fallbacks: firing, retaliation and auto-target gates; 5 keys)
- [Berzerk targeting](phobos-berzerk-targeting.md) (`[CombatDamage] BerzerkTargeting=` limits the houses a berzerk firer may target)
- [Recount burst](phobos-recount-burst.md) (`RecountBurst` recounts a retained burst index on a weapon change or idle timeout)
- [AI superweapon delay](phobos-ai-superweapon-delay.md) (`[General] AISuperWeaponDelay=` spaces a computer house's `AI_TryFireSW`)
- [Reveal houses](phobos-reveal-houses.md) (`RevealHouses` per techno, country and `[AudioVisual]` filters which players a techno's sight reveals to)
- [Freed-slave owner and sound](phobos-slave-freed.md) (`Slaved.OwnerWhenMasterKilled` picks killer/master/suicide/neutral per slave type, and a type `SlavesFreeSound` replaces the fixed release sound; 2 keys)
- [Warhead self-damage](phobos-allow-damage-on-self.md) (`AllowDamageOnSelf` per warhead and globally, and the airborne `DamageSelf` fix)
- [Zero-damage C4 minimum](phobos-c4-zero-damage.md) (`CanC4.AllowZeroDamage` keeps a `CanC4=no` building from taking 1 point when `Verses` scales a non-zero hit to 0; negative packets bypass the floor; 1 key)
- [Unit destroy-animation pick](phobos-destroy-anim-random.md) (`DestroyAnim.Random` per type and globally: a destroyed unit picks its `DestroyAnim` list entry at random or by facing; 2 keys)
- [Auto-target own position](phobos-auto-target-own-position.md) (`AutoTargetOwnPosition` / `.Self` and the deprecated `AutoFire` aliases make the auto-target scan hold the unit or its cell; 4 keys)
- [Sellable repack buildings](phobos-undeploys-into-sellable.md) (`UndeploysInto.Sellable` lets a non-ConYard repack building sell instead of packing up when it has no ArchiveTarget; 1 key)
- [Crate goodie reroll](phobos-crate-goodie-reroll.md) (`CrateGoodie.RerollChance` makes a crate unit candidate fail a `RandomDouble()` roll and draw again; 1 key)
- [Refinery normal active anim](phobos-refinery-use-normal-active-anim.md) (`Refinery.UseNormalActiveAnim` makes a refinery use its normal ActiveAnim slots instead of the storage-fill tier anims; 1 key)
- [Crater tiberium destroy](phobos-crater-destroy-tiberium.md) (`Crater.DestroyTiberium` / `[General] AnimCraterDestroyTiberium` keep a crater animation from reducing its cell's ore at the middle frame; 2 keys)
- [AnimType splash animations](phobos-splash-anims.md) (`SplashAnims` replaces the stock water-landing `SplashList`, `SplashAnims.PickRandom` draws the entry, and `Warhead.Detonate` routes the landing warhead through `WarheadTypeExt::DetonateAt`; 3 keys)
- [RevealToAll radius](phobos-reveal-to-all-radius.md) (`RevealToAll.Radius` sets the sight circle a computer RevealToAll building shows the current house; 1 key)
- [Negative-damage modifiers](phobos-negative-damage-modifiers.md) (`ApplyModifiersOnNegativeDamage` per warhead and globally runs a negative packet through falloff/Verses instead of the stock bypass; 2 keys)
- [Tiberium ramps](phobos-allow-ramps.md) (`AllowRamps` lets a tiberium type grow and spread onto cardinal ramp cells when its ramp art exists, and raises `NumSlopes` to 8; 1 key)
- [Terrain passability](phobos-terrain-passable.md) (`IsPassable` per terrain type plus `[General] Terrain.IsPassable` / `Tibtree.IsPassable` make a terrain object contribute no occupation; 3 keys)
- [Terrain CanBeBuiltOn](phobos-terrain-can-be-built-on.md) (`CanBeBuiltOn` per terrain type plus `[General] Terrain.CanBeBuiltOn` / `Tibtree.CanBeBuiltOn` let a building replace the object; admission, flush, PlaceDown removal and the AI site search; 3 keys)
- [Terrain crumbling frames](phobos-crumbling-frames.md) (`HasCrumblingFrames` keeps a lethally hit terrain object alive to play a crumbling stage, removed at `Frames/2` with `DestroySound`/`DestroyAnim`; `CrumblingSound`, `AnimationLength`, `HasDamagedFrames`)
- [Warhead debris block](phobos-warhead-debris.md) (`Debris.Conventional` water skip, `RandomRanged(MinDebris, MaxDebris)` budget, the warhead `DebrisMinimums`/`DebrisTypes.Limit` restored loop and `DebrisAnims` fallback; 4 keys)
- [TurretRecoil.Suppress](phobos-turret-recoil.md) (a weapon's fired shot skips the unit recoil record; 1 key)
- [FireOnce.ResetSequence](phobos-fire-once-reset-sequence.md) (`no` keeps an infantryman's firing sequence and action across the target clear his own `FireOnce=` shot causes; 1 key)
- [MakesWake](phobos-makes-wake.md) (`MakesWake` per type over the four `[AudioVisual] *LocomotorMakesWake=` globals skips a moving hull's water wake; 5 keys)
- [MindControl.ThreatDelay](phobos-mind-control-threat-delay.md) (a warhead's capture shields the new pawn from its former allies' retaliation and auto-targeting for the warhead or `[General]` delay; 2 keys)
- [Psychedelic.StackingMode](phobos-psychedelic-stacking-mode.md) (a Psychedelic hit combines its damage with the victim's remaining berserk time through a per-warhead or `[CombatDamage]` mode; a non-positive result skips the flag, removal, target clear and Hunt; 2 keys)
- [Tiled.Interval / Tiled.AlignToCenter](phobos-tiled-anim-options.md) (a Tiled anim's row height can be overridden and its column can start on the draw point; 2 keys)
- [DiskLaser.Radius](phobos-disk-laser-radius.md) (a disk-laser weapon's ring radius rebuilds the 16-point table; 1 key)
- [Layer.UseObjectLayer](phobos-layer-use-object-layer.md) (an attached anim can follow its owner object's display layer or keep its own type layer; 1 key)
- [Translucency.Cloaked](phobos-translucency-cloaked.md) (an anim attached to a cloaking or cloaked Techno draws at its alternate translucency level; 1 key)
- [Anim VisibleTo](phobos-anim-visible-to.md) (`VisibleTo` house-gates an anim's draw and `VisibleTo.ConsiderInvokerAsOwner` resolves the house from the invoker; 2 keys)
- [FireUp.ResetInRetarget](phobos-fire-up-reset-in-retarget.md) (a turretless Unit clears its firing sequence on a new target unless the key is off and the target is already in range; 1 key)
- [Gas.MaxDriftSpeed](phobos-gas-max-drift-speed.md) (a Gas particle type sets the ± bound its Velocity.X/Y clamp uses; 1 key)
- [EBoltZAdjust.ClampInitialDepthForBuilding](phobos-ebolt-zadjust-building-clamp.md) (a building bolt keeps or clamps its positive initial-depth delta per weapon or `[AudioVisual]`; 2 keys)
- [ProjectileRange.ApplyModifiers](phobos-projectile-range-apply-modifiers.md) (a `Ranged=` projectile's travel budget folds `GetRangeWithModifiers`'s veteran and AttachEffect arms per weapon or `[CombatDamage]`; 2 keys)
- [Bolt.Duration / Bolt.FollowFLH](phobos-bolt-duration-follow-flh.md) (a weapon sets an electric bolt's lifetime exponent and whether its bolt follows a non-Unit firer; 2 keys)
- [TeamMember.ConsideredAs](phobos-team-member-considered-as.md) (a TechnoType fills a TaskForce entry for the types it lists in the team add/liberate/recruit matches and the missing-type tally; 1 key)
- [Anim Damage.ApplyFirepowerMult](phobos-anim-damage-firepower-mult.md) (an anim's invoker hands it its current firepower multiplier for loop damage, per AnimType over the `[CombatDamage]` global; 2 keys)
- [AreaFire.Target](phobos-area-fire-target.md) (`AreaFire=` weapons pick the firer (`self`) or one drawn eligible adjacent cell (`random`) instead of the stock cell under the firer; 1 key)
- [Wall weapon selection](phobos-wall-weapon-select.md) (`AllowWeaponSelectAgainstWalls` per type and globally lets the wall checks at `0x51C1F1`/`0x73F495`/`0x70095A` fall back to a secondary wall-breaker; 2 keys)
- [Projectile gravity](phobos-projectile-gravity.md) (`[Projectile] Gravity=` per projectile type)
- [Projectile AU](phobos-projectile-au.md) (`[Projectile] AU=yes` admits underground targets in the auto-target scan, `CanFire` and the `0x6F37EB` weapon-selection arm)
- [Cylinder rangefinding](phobos-cylinder-rangefinding.md) (`CylinderRangefinding` per weapon and globally: direct range uses `dx²+dy²`)
- [Ambient damage warhead](phobos-ambient-damage.md) (`AmbientDamage.Warhead` / `.IgnoreTarget` per weapon plus the `[General]` fallback: railgun and blast-wave ambient damage use an override warhead and can skip the primary target; 3 keys)
- [Warhead decloaking](phobos-decloak-damaged-targets.md) (`DecloakDamagedTargets` per warhead and globally)
- [ShowBriefing keys](phobos-show-briefing.md) (`[Phobos] ShowBriefing` + scenario `[Basic] ShowBriefing` + two `uimd.ini [UISettings]` labels: a campaign-only scenario-start admission opens the 0x10C briefing dialog with the authored resume/status labels, while the F2 restate path keeps `GUI:Return`; 3 keys)
- [Warhead remove disguise](phobos-warhead-remove-disguise.md) (`RemoveDisguise=yes` strips a hit target's disguise)
- [Warhead ammo](phobos-warhead-ammo.md) (`Ammo=N` adjusts each hit target's ammo with the native clamp)
- [Warhead taunt](phobos-warhead-taunt.md) (`Taunt=yes` forces each hit target onto Attack against the firer)
- [Warhead CanKill](phobos-warhead-can-kill.md) (`CanKill=no` leaves a lethal victim at 1 HP)
- [Warhead Reveal](phobos-warhead-reveal.md) (`Reveal=N` reveals a radius, or the whole map when negative, to the firing house)
- [Warhead CreateGap](phobos-warhead-create-gap.md) (`CreateGap=N` shrouds a radius, or the whole map when negative, for enemies)
- [Warhead reverse engineer](phobos-warhead-reverse-engineer.md) (`ReverseEngineer=yes` records a hit target's type for the firing house through Ares)
- [Prevent escape](phobos-prevent-escape.md) (`PreventCrewEscape` / `PreventPassengerEscape` / `PreventOccupantEscape` deny survivors on a killing hit)
- [Turret response](phobos-turret-response.md) (`TurretResponse=` picks between the idle turret return and `CannotMove`)
- [Initial spawns number](phobos-initial-spawns-number.md) (`InitialSpawnsNumber=N` starts only N pool nodes live; the rest regenerate)
- [Airstrike flare](phobos-airstrike-flare.md) (`LaserTargetColor` and `AirstrikeLineColor` per TechnoType, and the line depth fix)
- [AttachEffect](phobos-attach-effect.md) (`[AttachEffectTypes]` catalog, warhead and TechnoType attach lists, stat multipliers, tint, discard conditions, expire/revenge/reflect weapons and the weapon-side gates)
- [Critical hits](phobos-critical-hit.md) (per-warhead `Crit.*` chance, extra damage, second warhead and animations, `ImmuneToCrit`, rank abilities)
- [Selection flash](phobos-selection-flash.md) (`[AudioVisual] SelectionFlashDuration` behind `[Phobos] ShowFlashOnSelecting`)
- [Semantic locomotor aliases](phobos-locomotor-aliases.md) (`Locomotor=Drive` and friends beside the braced CLSIDs)
- [Airstrike target eligibility](phobos-airstrike-targets.md) (`AllowAirstrike` per TechnoType, `AirstrikeTargets` per warhead, and object targets for non-buildings)
- [Deployable-building alternate cameo](phobos-deployable-cameo.md) (Ares' `UndeploysInto` cameo arm, fixed)
- [Animation bracket draw offsets](phobos-anim-offset.md) (`XDrawOffset.*` and `YDrawOffset.*` selection-bracket keys for owned animations)
- [Building turret animations](phobos-turret-anim.md) (`TurretAnim.IdleFrames`, `.LowPowerIdleFrames`, `.FiringFrames`, `.LowPowerFiringFrames`, `.IdleRate` and `.FiringRate` frame selection for SHP slot-9 turrets)
- [Roof production anims](phobos-roof-production-anim.md) (`RoofProductionAnim*` eleven ART keys: a roof-exit factory unload animation, its damaged/garrisoned picks, placement and powered flags)
- [Build limit groups](phobos-build-limit-group.md) (`BuildLimitGroup.*` nine keys, `[General]` fallbacks, ExtraLimit quota and the queue-match gate)
- [Delayed fire](phobos-delayed-fire.md) (`DelayedFire.*` wind-up delay, timer, pause, wind-up animation and `Burst.NoDelay` compatibility; 11 keys)
- [Aircraft spawner customizations](phobos-spawner.md) (`Spawner.*` launch spacing, immediate attack, turret facing, pursue range and the recycle point; 9 keys + 4 `[General]` fallbacks)
- [Detonate on all map objects](phobos-detonate-all-map.md) (`DetonateOnAllMapObjects.*` whole-map detonation, target/house/type filters and the Full/damage-area arms)
- [Keep target on move](phobos-keep-target-on-move.md) (`KeepTargetOnMove.*` move-order target retention, the Unit AI range re-check and the temporary ExtraDistance)
- [OpenTopped per-type properties](phobos-open-topped.md) (eleven `OpenTopped.*` TechnoType keys, eight `[General]` globals, two fallback chains and the firing/range/cloak/temporal hooks)
- [Promotion animations and promoted spawns](phobos-promote.md) (`Promote.*` rank-crossing animation lists with the `[AudioVisual]` fallbacks, and the spawner child rank lift; 3 keys + 3 globals)
- [Attack move](phobos-attack-move.md) (`AttackMove.IgnoreWeaponCheck`, `.Aggressive` and `.UpdateTarget`: the weapon check on the order, unarmed buildings as targets, and the target swap)
- [Attack-move followers](phobos-attack-move-follow.md) (`AttackMove.Follow*`: escort the nearest friendly attack-mover, air inclusion, the mind-control-full condition and the per-leader follower count)
- [Approach termination](phobos-approach-target.md) (`ApproachTarget.StopWhenInRange` stops a close, moving foot; `.PursuitTarget` sets the target as the destination; 2 type keys + 1 `[General]` fallback)
- [Trajectory base and Straight](phobos-trajectory-straight.md) (`Trajectory` selection, `Trajectory.Speed` and the 30 `Trajectory.Straight.*` keys: speed flight, snap/detonation distances, pass/proximity hits, obstacles and attenuation; 32 keys)
- [Bombard trajectory](phobos-trajectory-bombard.md) (`Trajectory.Bombard.*`: the turning-point lob, fall/scatter, early detonation, lead time and burst rotation; 21 keys)
- [Parabola trajectory](phobos-trajectory-parabola.md) (`Trajectory.Parabola.*`: six open-fire modes, lead solve, bounce ladder and detonation gates; 18 keys)
- [Spy-effect superweapon launches](phobos-spy-effect.md) (`SpyEffect.*` victim/infiltrator launches on a spy infiltration, with the recharge kept; 3 keys)
- [Rearm and reload holds](phobos-no-rearm.md) (`NoRearm.*` / `NoReload.*` freeze the rearm and ammo-reload timers while EMP'd or Temporal-held; 4 keys + 4 `[General]` globals)
- [Passenger owner sync](phobos-passengers-sync-owner.md) (`Passengers.SyncOwner` re-houses cargo on the transport's house change, `Passengers.SyncOwner.RevertOnExit` restores board-time houses on exit and on the carrier's death, and the threat-eval owner substitution skip; 2 keys + 2 `[General]` globals)
- [Driver-killed passengers](phobos-driver-killed.md) (`DriverKilled.KeptPassengers` and `.KillPassengers` choose keep-aboard, kill or the stock ejection for a driverless vehicle's passengers; 2 keys + 2 globals)
- [No manual move](phobos-no-manual-move.md) (`NoManualMove`: the shared player-control answer refuses the type while its AI keeps moving it)
- [Health-band conversion](phobos-convert-health.md) (`Convert.Health` and its `AbovePercent`/`BelowPercent` band: a Foot converts when its health ratio enters the range; 3 keys)
- [Deploy/Undeploy conversion](phobos-convert-undeploy.md) (`Convert.Deploy` / `Convert.Undeploy`: an infantryman converts once per completed Deploy or Undeploy sequence, with the DoingAI latch reset; 2 keys)
- [Owner-change conversion](phobos-convert-owner-change.md) (`Convert.HumanToComputer` / `Convert.ComputerToHuman`: a Foot converts on a human/computer owner change and the computer-to-human arm repairs its mission; 2 keys)
- [Mind-control reset on conversion](phobos-convert-reset-mind-control.md) (`Convert.ResetMindControl`: the capture-manager limits are rebuilt from the new type's weapons, trimming or deleting the manager; 1 key + 1 `[General]` global)
- [Convert pairs](phobos-type-convert-group.md) (`Convert{i}.From`/`.To`/`.AffectsHouse` on a warhead or superweapon: the per-target and whole-array Foot conversions)
- [Explodes keys](phobos-explodes.md) (`Explodes.KillPassengers` / `Explodes.DuringBuildup`: the death arm's passenger kill and a building's death weapon under Construction/Selling; 2 keys + 2 `[General]` globals)
- [Extra warheads](phobos-extra-warheads.md) (`ExtraWarheads` and its damage, chance, roll, weight and full-detonation lists: warheads detonated after the weapon's own at the same impact; 6 keys)
- [Projectile interception](phobos-interceptor.md) (`Interceptor.*` scans for enemy bullets, fires a dedicated slot and damages/replaces/deletes the intercepted projectile; 12 TechnoType keys + 4 BulletType keys + 3 globals)
- [SHP vehicle turret files](phobos-shp-turret.md) (`<name>TUR.SHP` for a non-voxel turreted unit, `[General] ArtImageSwap`, the frame-0 turret block and the body-slice fallback)
- [Base-center exclusion](phobos-ignore-for-base-center.md) (`IgnoreForBaseCenter` keeps a building out of the House's base-center math and a starting techno out of the opening view; 1 key)
- [Cloned infantry strength](phobos-initial-cloning.md) (`InitialStrength.Cloning` scales a cloning vat's exiting clone to a percentage or `min,max` range of its own Strength; 1 key)
- [Animation-created units](phobos-create-unit.md) (`CreateUnit.*` spawns a Techno at the anim's actual-effects frame, with the death-facing/turret inheritance, spawn height, parachute, pathfinding, mission and spawn-anim keys; 13 keys)
- [Warhead shield modifiers](phobos-shield-warhead.md) (`Shield.*` on a warhead: attach/remove/replace, break, respawn/self-healing modifiers, the absorb/pass split and penetration; 39 keys)

- [Return warhead](phobos-return-warhead.md) (`ReturnWarhead` and its damage, chance, full-detonation, target and house filters: the per-target strike back at the firer; 5 keys)

- [Tiberium eater](phobos-tiberium-eater.md) (`TiberiumEater.*`: FLH-cell tiberium eating with cash, delay, per-tiberium anims and EMP/limbo holds; 11 keys)
- [Ammo auto-convert](phobos-convert-ammo.md) (`Ammo.AutoConvertType`/`.MinimumAmount`/`.MaximumAmount`: a Foot converts when its ammo enters the band; 3 keys)
- [LaunchSW](phobos-launch-sw.md) (`LaunchSW` and its real-launch, inhibitor/designator and display-money options: a warhead launches superweapons at its impact; 7 keys)
- [NotHuman death sequences](phobos-not-human.md) (`NotHuman.DeathSequence` / `NotHuman.RandomDeathSequence`: a NotHuman infantryman's Die1..Die5 selection and the paradrop fix; 2 keys + 1 `[General]` global)
- [KeepRange](phobos-keep-range.md) (`KeepRange` and its AllowAI/AllowPlayer/EarlyStopFrame gates: an Infantry/Unit holds a distance while rearming; 4 keys + 3 `[General]` globals)
- [Parasite keys](phobos-parasite.md) (`Parasite.ParticleSystem`/`.DisableParticleSystem`/`.CullingTarget`/`.GrappleAnim`/`.AllowWaterExit`, `RemoveParasite`/`.Allow`/`.Disallow`, `Sinkable.SquidGrab`: the bite, grapple, removal and squid-cull customizations; 9 keys + 4 globals)
- [Superweapon Detonate](phobos-sw-detonate.md) (`Detonate.Warhead`/`.Weapon`/`.Damage`/`.Warhead.Full`/`.AtFirer`: a superweapon detonates a weapon or warhead at its target; 5 keys)
- [Cloak keys](phobos-cloak.md) (`Cloak.KickOutParasite` plus `CloakAnims`/`DecloakAnims`: a cloaking host ejects its parasite and plays its cloak/decloak anim; 3 keys + 3 globals)
- [AffectsUnderground](phobos-affects-underground.md) (`AffectsUnderground`: a warhead's impact also damages Underground-layer technos inside its CellSpread; 1 key)
- [OpenTransport keys](phobos-open-transport.md) (`OpenTransport.RangeBonus`/`.DamageMultiplier`/`.FireWhileMoving`: a passenger's own open-topped firepower, range and movement gate; 3 keys + 3 globals)
- [ChangeOwner warhead](phobos-change-owner.md) (`ChangeOwner`/`.SetAsMindControl`/`.MindControlAnim`: a warhead transfers its target to the firing house, optionally as a mind-control capture; 3 keys)
- [Low deploy priority](phobos-low-deploy-priority.md) (`LowDeployPriority` per TechnoType behind `[Phobos] PriorityDeployFiltering`: the deploy key drops a low-priority object from a multi-object selection that holds a normal one)
- [Destroyable pathfinding obstacles](phobos-destroyable-obstacle.md) (`IsDestroyableObstacle` per BuildingType: the foundation answers cell class Destroyable, so only movement rows that admit that class zone into it, and the class clears on limbo)
- [Amphibious enter and unload](phobos-amphibious-enter-unload.md) (`AmphibiousEnter` / `AmphibiousUnload` per transport TechnoType and `[General]`: boarding and unloading on water, using the passenger's own SpeedType/MovementZone in the unload searches)
- [SpawnsCrate warhead](phobos-spawns-crate.md) (`SpawnsCrate{i}.Type`/`.Weight` and their entry-0 aliases: a detonation places one weighted powerup crate; 2 numbered keys + 2 aliases)
- [Rocker warhead](phobos-rocker.md) (`Rocker.AmplitudeMultiplier`/`.AmplitudeOverride`: the area-damage rocker's base amplitude; 2 keys)
- [NoTurret.TrackTarget](phobos-no-turret.md) (`NoTurret.TrackTarget`: a keyed turretless vehicle keeps checking its facing while in REARM; 1 key + 1 `[General]` global)
- [Powered.KillSpawns](phobos-powered-kill.md) (`Powered.KillSpawns`: an offline spawner building resets its pool target and kills attacking/returning children; 1 key)
- [StartFacing](phobos-start-facing.md) (`StartFacing`/`StartFacing.Random`: a building's initial facing, optionally one random draw per building; 2 keys + 2 globals)
- [FactoryPlant restricted lists](phobos-factory-plant.md) (`FactoryPlant.AllowTypes`/`.DisallowTypes`/`.MaxCount` + `FactoryPlant.Multiplier`: per-type cost and refund discounts from a plant kept out of House+140; 3 keys + 1 type key)
- [Extra threat bonuses](phobos-extra-threat.md) (`ExtraThreat.*` / `ExtraThreatCoefficient.*` and `AlwaysConsideredThreat`: five type keys with `[General]` fallbacks added to the AI's candidate threat score; 6 keys + 5 globals)
- [Combat alerts](phobos-combat-alert.md) (`CombatAlert.*`: a throttled radar ping plus one feedback/attack/EVA line on a damaged human House's object; 6 type keys + 10 `[AudioVisual]` globals + 2 warhead keys)
- [Custom selection box](phobos-select-box.md) (`[SelectBoxTypes]` `Shape`/`Palette`/`Frames`/`Offset`/`Translucency`/`VisibleToHouses`/`DrawAboveTechno`/`Ground*`, plus TechnoType `SelectBox`/`HideSelectBox` and the `[AudioVisual]` defaults: the selected object's custom box and ground SHP)
- [EMPulse extensions](phobos-empulse.md) (`EMPulse.WeaponIndex`/`.SuspendOthers`/`.Cannons`/`.TargetSelf`: Ares EMPulse cannon weapon slot, suspended superweapons and the self-target arm; 4 keys)
- [Superweapon activation messages and sold EVA](phobos-sw-messages.md) (`Message.Activated.{Owner,Allies,Enemies}`, `EVA.Activated.*`, the `Message.LinkedSWAcquired`/`EVA.LinkedSWAcquired` pair and `EVA.Sold`: launch and linked-grant announcements picked by the launching House's relation to the current player, plus a type's replacement sale line; 9 keys)
- [Non-threatening building auto-target](phobos-attack-no-threat-buildings.md) (`AttackNoThreatBuildings`: a weapon's override, else the human/AI `[General]` pair, for auto-acquiring an unarmed or zero-threat enemy building; 1 key + 2 globals)
- [Projectile subject to ground](phobos-bullet-subject-to-ground.md) (`SubjectToGround` on a BulletType: the firing techno's obstacle scan stops where the projectile would pass below the cell floor; 1 key)
- [Feedback weapon](phobos-feedback-weapon.md) (`FeedbackWeapon`: a second weapon detonated on the firer after the shot's muzzle block, skipping an open-topped passenger when its `FireInTransport=no`; 1 key)
- [Visual scatter](phobos-visual-scatter.md) (`VisualScatter` plus the `[AudioVisual] VisualScatter.Min/Max` radius: a laser, bolt or beam drawn near the target while the shot keeps its real one; 1 key + 2 globals)
- [Damage multipliers](phobos-damage-multiplier.md) (`DamageOwnerMultiplier`/`DamageAlliesMultiplier`/`DamageEnemiesMultiplier` and their `.Berzerk` variants, plus the two health-fraction terms: the receiver rescales its own packet by both Houses' relation and both objects' health; 8 warhead keys + 8 `[CombatDamage]` globals)
- [Break mind control on impact](phobos-remove-mind-control.md) (`RemoveMindControl`/`.Silent`: a detonation frees its mind-controlled victims before every other per-target arm, so `ChangeOwner`/`Convert` see the real House; 2 warhead keys + 1 `[AudioVisual]` global)
- [Sell or undeploy a building on impact](phobos-building-sell.md) (`BuildingSell`/`.IgnoreUnsellable` and `BuildingUndeploy`/`.Leave`: a hit building is sold, else packed up into its `UndeploysInto=` unit; 4 warhead keys)
- [Merge area damage on a building](phobos-merge-building-damage.md) (`MergeBuildingDamage`: one detonation damages a building once with its per-cell distance multipliers summed, instead of once per foundation cell; 1 warhead key + 1 `[CombatDamage]` global)
- [Custom Ares radar-jam](phobos-radar-jam.md) (`RadarJamHouses`/`RadarJamDelay`/`RadarJamAffect`/`RadarJamIgnore`: the jammer scan's House filter, its interval and the two building-type lists; 4 type keys + 2 `[General]` globals)
- [Auto-firing at the object's own position](phobos-auto-target-own-position.md) (`AutoTargetOwnPosition`/`.Self` plus the deprecated `AutoFire`/`AutoFire.TargetSelf` aliases: a type replaces target acquisition with itself or its own cell; 2 type keys)
- [Reload speed on promotion](phobos-veteran-reload.md) (`VeteranReload`/`VeteranEmptyReload`: both `StartReloading` durations scaled by the type's key when the rank holds `RELOAD`/`EMPTY_RELOAD`; 2 type keys + 3 `[General]` globals)
- [Disguise movement speed](phobos-disguise-movement-speed.md) (`UseDisguiseMovementSpeed`: a disguised object takes its disguise's `Speed=`; 1 type key + 1 `[General]` global)
- [Sound on unit creation](phobos-voice-created.md) (`VoiceCreated` and `[AudioVisual] IsVoiceCreatedGlobal`: a factory product speaks its own line and the EVA "unit ready" line is dropped; 1 type key + 1 `[AudioVisual]` global)
- [Customizable spawns queue](phobos-spawns-queue.md) (`Spawns.Queue`: a spawn manager's per-slot and regeneration type order — read, not yet consumed; 1 type key)
- [Fast access to a vehicle or structure](phobos-no-queue-up-to-enter.md) (`NoQueueUpToEnter`/`.BoardDistance` plus `[General] NoQueueUpToEnter.Buildings`: boarding without the queue when close enough — read, not yet consumed; 2 type keys + 3 `[General]` globals)
- [Drop a crate on death](phobos-drop-crate.md) (`DropCrate`: the `[Powerups]` slot a dying object leaves behind — read, not yet consumed; 1 type key)
- [Damaged image changes](phobos-damaged-image.md) (`Image.ConditionRed`/`.ConditionYellow` and `WaterImage.*`: a Unit or Aircraft draws a replacement type in the yellow and red bands, with the water pair replacing the land pair on water; 4 type keys)
- [Phobos dot-key families](phobos-dotkey-families.md) (`CrashSpin.Multiplier`, `Cloning.Powered`, `BuildingRadioLink.SyncOwner`, `Damage.DealtByInvoker`/`.ApplyOncePerLoop`, `FallingDownDamage*`, `Wake*`, `WarpAway`/`WarpOutWeapon`, `ChargeTurret.Delays`, `IvanBomb.Visibility`, `CellSpread.Cylinder`, `Temporal.ApplyMultiplier`/`.ApplyVersus`, `Arcing.AllowElevationInaccuracy`: twelve small families in one batch page)
- [Phobos Ammo pip keys](phobos-ammo-pips.md) (`AmmoPipFrame`, `AmmoPipWrapStartFrame`, `AmmoPipSize`, `AmmoPipOffset`: four TechnoType keys next to the ported `EmptyAmmoPipFrame`)
- [Hover vehicle drown death](phobos-hover-drown.md) (`HoverDrownable`: a hover Unit that shut down over water takes the whole-health packet when it lands, and the type's key overrides `[General] HoverDrownable`; 1 type key + 1 `[General]` global)
- [RA1 multi-turret / multi-barrel](phobos-turret-barrel.md) (`BarrelOverTurret`/`BarrelOffset`/`ExtraBarrelCount`/`ExtraTurretCount`/`BurstPerTurret` and the indexed offsets: `BurstPerTurret` cycles the extra-turret fire-origin pivot; the per-barrel/turret draw transforms and recoil slots stay residual; 5 type keys + 2 indexed lists)
- [Parabombs](phobos-parabomb.md) (`Parachuted`/`.FallRate`/`.MaxFallRate` and `BombParachute`: a projectile born under a canopy that descends at a step/cap fall rate and detonates on the ground; 4 type keys)
- [Return weapon](phobos-return-weapon.md) (`ReturnWeapon` and `ReturnWeapon.ApplyFirepowerMult`: a detonating bullet fires the named weapon back at its firer from the impact, with the firepower multiplier scaling `Damage=`; 2 type keys + 1 `[CombatDamage]` global)
- [Damaged aircraft image changes](phobos-damaged-image.md) is shared by the Unit (`Image.Condition*`) and Aircraft (`WaterImage.Condition*`) readers — see that page.
- [Roof production exit](phobos-exit-through-roof.md) (`ExitThroughRoof`: a produced unit leaves through the roof anim instead of the gate — read, not yet consumed; 1 type key)
- [Slaves on master-killed](phobos-slaved.md) (`Slaved.OwnerWhenMasterKilled`: killer / master / suicide / neutral for a freed slave miner — read, not yet consumed; 1 type key)
- [Land sequences in water](phobos-land-sequences.md) (`OnlyUseLandSequences` on the art section plus `[General]`: infantry keep land sequences in water; 1 type key + 1 `[General]` global)
- [Super-weapon sidebar tab](phobos-sw-tab-index.md) (`TabIndex`: which of the four sidebar tabs a super weapon cameo occupies, out-of-range resetting to `1`; 1 type key)

## Unmapped-section audit

The forty `###` sections of Phobos's `New-or-Enhanced-Logics.md` whose keys the
coverage scanner could not resolve — the keys they really read, the VERA owner
and a status each — are tabulated in the
[unmapped-section audit](phobos-audit-unmapped-40.md). It records mapping state
only; the sections there are not implemented by it.
- [Deploy gates](phobos-deploy-gates.md) (`Deploy.SkipPassengerUnload`, `.NoPassenger`, `.NoTiberium` and `IsSimpleDeployer.ConsiderPathfinding`, `.DisallowedLandTypes`: the unload passenger skip, the harvester dump skip and the simple-deployer own-cell gate; 5 keys)

## Original project and authors

VERA20K implements the behavior in Rust. It does not load `Phobos.dll`, import
Phobos C++ source or redistribute Phobos binaries; the native identities cited
by these pages are the Phobos sources named in each page.

This product includes software developed by the Phobos Developers.

Phobos was founded by **Belonit** (Gluk-v48) and **Kerbiter** (Metadorius) in
2020 and is developed by the
[Phobos Developers](https://github.com/Phobos-developers/Phobos#credits):

- Maintainers: **Kerbiter** (lead), **Starkku** (co-lead), **CrimRecya**,
  **ZivDero**, **Ollerus**, **NetsuNegi**, **TaranDahl** and **Noble_Fish**
  (also documentation and triage).
- Triage: **FlyStar** and **Fryone**.
- Inactive maintainers: **Belonit** (past lead), **Uranusian** (also Chinese
  community ambassador), **secsome**, **Otamaa**, **FS-21**, **Morton** and
  **Trsdy**.

The features ported here originate with:

- Custom radiation types - **Otamaa**, who ported and fixed the **Ares
  developers**' unfinished RadTypes code.
- Shields - **Uranusian**, **secsome**, **Belonit**; shield warhead
  modifications **Starkku**; absorption/passthrough **Morton**; respawn
  animation and weapon **Ollerus**.
- Custom ore spawners - **Kerbiter**.
- Custom gather animations - **secsome**, **Uranusian**.
- Ore-stage animation hiding (`HideIfNoOre` threshold) - **Otamaa**.
- Superweapon techno gates (`SW.AuxTechnos`, `SW.NegTechnos`,
  `SW.TechLevel`) - **NetsuNegi**; their building-upgrade counts -
  **NetsuNegi**, **Ollerus**.
- Digital display - **ststl**, **FlyStar**, **NaotoYuuki**, **Saigyouji**,
  **JunJacobYoung**, **CrimRecya**.
- Placement preview - **Otamaa**, **Belonit**.
- Sidebar GDI/NOD positions - **Belonit**.
- Infantry auto deploy - **TaranDahl**.
- Fly wobble switch (`FlyNoWobbles`) - **Noble_Fish**.
- Centred pause-menu background - **Starkku**.
- Unhardcoded timer blinking colour scheme - **Starkku**.
- Semantic locomotor aliases (`Locomotor=Drive` and the like) - **Belonit**.
- Missing cameo - **Uranusian**.
- Power plant enhancers - **secsome**, **NetsuNegi** (range), **Ollerus**
  (`MaxCount`), **Trsdy** (EMP).
- Power plant damage factor - **Otamaa**, **Ollerus**.
- Building production queue - **CrimRecya**.
- INI `$Include` / `$Inherits` - **Morton**.
- Skirmish AI dehardcoding (last-legs sale/hunt, MCV deploy regroup) -
  **TaranDahl**.
- Parallel AI queue forbids - **Trsdy**, **NetsuNegi** (by category),
  **Starkku** (by TechnoType).
- Target evaluation map zone check (`TargetZoneScanType`) - **Starkku**.
- Unlimited skirmish colours - **Morton**.
- Superweapon sidebar - **NetsuNegi**, **CrimRecya**.
- Prone speed customization - **TaranDahl**.
- Type select for buildings - **TaranDahl** (code), **Ollerus** (doc).
- Flashing Technos on selecting - **Fryone**.
- Warhead self-damaging toggle (`AllowDamageOnSelf`) - **Starkku**; its and
  `DamageSelf`'s airborne fix - **NetsuNegi**.
- Custom gravity for projectiles - **secsome**.
- Warhead decloaking toggle (`DecloakDamagedTargets`) - **Starkku**.
- Airstrike flare visual customizations and fixes - **Starkku**.
- AttachEffect - **Starkku**, with target filtering **Flactine** and
  attach-event 606 **FS-21**.
- Critical hits - **Starkku** (logic), **AutoGavy** (warhead crit), and
  **Trsdy** (shield `ImmuneToCrit`).

## VERA20K porting credits

The Rust ports are individual VERA20K contributions; GitHub usernames name
the merged pull requests that carried each chain:

- **1372704087** — the Phobos chains named here: shields, custom
  radiation types, the three ore chains, superweapon techno gates, digital
  display, placement preview, sidebar GDI/NOD positions, infantry auto
  deploy, `FlyNoWobbles`, the centred pause-menu background, building
  production queue, skirmish AI dehardcoding, parallel AI queue forbids,
  unlimited skirmish colours, the superweapon sidebar, prone speed, type
  select for buildings, timer blink colour, missing cameo, power plant
  enhancers, semantic locomotor aliases, selection flash,
  `AllowDamageOnSelf`, `DecloakDamagedTargets`, projectile gravity, the
  airstrike flare colours, AttachEffect with the critical-hit chain, and
  the `$Include`/`$Inherits` native hooks.
- **Krisztiaan** — the shield damage/recovery hook-ordering fix.
- **CCDawn** — the Phobos build description drawn at the tactical view's
  top right.

The project's full contributor list is in
[Phobos CREDITS.md](https://github.com/Phobos-developers/Phobos/blob/develop/CREDITS.md).
VERA's include and inheritance traversal also adapts ra2-mono's `IniFile`;
see [INI directives](ini-directives.md#source-mapping).

简体中文：[扩展支持与原作者](zh-CN/credits.md)。
