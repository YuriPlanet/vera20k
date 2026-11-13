# Optional Ares stealth, power sources, radar jammers and sensor arrays

Build with `--features ares`. Default builds ignore every key below and keep
the stock cloak, Robot Tank, radar and sensor-array behaviour.

```ini
[SUBX]                     ; TechnoType
Cloakable.Stages=3         ; progress stages, else [General]CloakingStages
Cloakable.Powered=yes      ; Building: self-cloaks only while online
Cloakable.Deployed=yes     ; Infantry: self-cloaks only while deployed
CloakSound=SubDive         ; else [AudioVisual]CloakSound; none silences
DecloakSound=SubSurface    ; else [AudioVisual]DecloakSound, else CloakSound
PoweredBy=GAPOWR,NAPOWR    ; works only while one of these is online
RadarJamRadius=8           ; cells; jams enemy Radar/SpySat buildings
SensorArray.Warn=no        ; no radar event when this type is sensed

[General]
CloakHeight=120            ; generator cloak ceiling, else HoverHeight
[AudioVisual]
DeactivateDimPowered=0.5   ; Unit voxel dim while deactivated
DeactivateDimEMP=0.8       ; ... while under EMP
DeactivateDimOperator=0.65 ; ... while missing its Operator
```

## Where VERA20K owns it

- `rules/ares_cloak_power.rs`: the TypeExt and RulesExt readers
  (`1003DFB0`, `1002AB60`, `1002A800`), retained per pass by the registry;
  the sounds bind with the other fixed-catalog sounds. ValidateThings
  `100677B0` clears `PoweredUnit=` on a PoweredBy type and a `PowersUnit=`
  naming one.
- `sim/world/techno_ai_cloak.rs`: CloakDisallowed `10043A40` and
  CloakAllowed `10043990` inside the one cloak producer (hooks `6FB757`,
  `6FBDC0`, `6FBC90`); the stages and sounds of hooks `7036EB`, `703A79`,
  `70380A` and `70375B` go through `ObjectType::cloaking_stages` and
  `emit_configured_cloak_sound`. A cloaking parasite host keeps its parasite
  (`10048280`, `100482D0`).
- `sim/powered_by.rs`: PoweredUnitClass `1006A6F0` with EnableEMPEffect2
  `10062980` and DisableEMPEffect2 `10062650`; IsPowered `100454F0` is the
  shared veto of Activate, the operator update and the Unit spark block.
- `sim/powered_unit.rs`: Activate `1004CCB0` and Deactivate `1004CDC0` skip
  the type's sound under IKnowWhatImDoing and refresh a Building's factory
  queues on the transition (`10013250`).
- `sim/radar_jammer.rs`: JammerClass `10068500` and its Unjam/UnjamAll; the
  radar and SpySat scans skip a jammed or EMPed provider (`10024550`,
  `10024620`).
- `sim/sensor_lifecycle.rs`: the sensor-array activation counter
  (`10016D60`, `10016DB0`, `10016E30`) and its capture/destruction moves.
- `app/presentation/lighting.rs`: the Unit voxel dim (`1004CEA0`);
  `render/radar/tracker.rs`: `SensorArray.Warn` (`10016E60`);
  `app/presentation/ui_overlays.rs`: the health strip of a sensed object
  below the surface (`10048350`).

## Not covered

- The `EVA_CloakedUnitDetected`/`EVA_SubterraneanUnitDetected` lines and
  `[AudioVisual]StealthSpeakDelay`/`SubterraneanSpeakDelay`: the radar
  tracker has no EVA channel. The keys are read; no line plays.
- The sonar pulse's TechnoExt SonarTimer term of CloakDisallowed.
- The deactivated cursor and click actions (`10044AD0`, the What_Action
  hooks and the Deactivated arm of Building ClickedAction `10016F60`) are
  in [ares-techno-hooks.md](ares-techno-hooks.md).
- Prism-forwarded jammer retention on capture. (PoweredBy's team liberation,
  silent reselection and the spawn/slave manager updates of EnableEMPEffect2
  `10062980`, DisableEMPEffect2 `10062650` and updateSpawnManager `10063070`
  are in `sim/powered_by.rs`.)
