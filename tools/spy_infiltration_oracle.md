# Spy infiltration: House gains and tech rechecks

Replay the existing original-execution profile:

```sh
python -B -m tools.spy_infiltration_oracle --check
```

`--write` explicitly regenerates the JSON and provenance sidecar. The sidecar
pins the original executable and replay command, with no repository-source
hashes. Native `BuildingClass::Infiltrate` is `0x004571E0..0x004575A2`;
`src/sim/spy_infiltration_tests.rs` consumes its House, money, power and Super
results, and the app's existing sound-dispatch consumer replays its presentation events.

The shared `ai_base_building_oracle.Emu` executes each supplied building/type,
House, Rules and optional Super input. Native selector order is Radar,
positive Power, identity in `[AI] BuildTech`, SuperWeapon, positive Storage,
then Unit/Infantry Factory. Existing money/blackout/Super kernels execute;
there is no second selector, wallet, INI reader or RNG implementation here.

All three supplied Houses start with `+0x1FC=0`, representing a settled House
after its constructor request was consumed. Expected `owner.recheck_tech` and
`infiltrator.recheck_tech` observe the bytes after the whole native call.
Gain writes precede the request on the supplied infiltrator House:

| Native arm | Gain | Tech request |
| --- | --- | --- |
| BuildTech side0 | `+0x2BE=1` at `0x004573D0` | `+0x1FC=1` at `0x004573EA` |
| side1 | `+0x2BD=1` at `0x004573DC` | same |
| other signed side | `+0x2BC=1` at `0x004573E4` | same |
| UnitType Factory | `+0x2C0=1` at `0x004574BD` | `0x004574C3` |
| InfantryType Factory | `+0x2BF=1` at `0x0045751E` | `0x00457524` |

Optional `initial_gains` is five supplied boolean bytes in native
Third/Soviet/Allied/veteranInfantry/veteranUnit order. Repeated-gain controls
start with the selected gain already true and the request clear: native still
requests a new pass. Own-house cases return before either write. Other arms,
including nonpositive Power with no matching effect and Aircraft/Building
factories, do not request the tech pass. The Rust replay installs prior gains
through their existing owner, then clears every constructor/setup request
before calling real `infiltrate_building`; an earlier true request cannot
make its comparison pass.

These are original leaf results on supplied inputs, not a full spy mission
or a claim that every surrounding path consumes no RNG. Native
`Mission_Capture` separately draws Scenario `RandomRanged(0,2)` at
`0x004D4C97`, and its dispatcher owns the mission timer. The selected tech
arm itself has no direct RNG, timer or detach call. Other Infiltrate arms
reset the blackout or Super timer. Caller `0x00519FF8..0x0051A02E`, optional
Tag48 and spy UnInit/expiry/Limbo/deferred destruction are outside this VM.

Recorded substitutions remain Map Reset_Shroud, CreateRadarEvent and PlayEVA;
the building cell is supplied and Mark returns. Sidebar byte `0x00884B8E`,
rendered alt cameos, radar/voice device internals and whole native save files
are outside the state comparison. Later House `0x004F9265..0x004F9302`,
Building construction options `0x004456D0`, retained sidebar insertion and
paid production have their existing owners and separate integration evidence.
For stock rules, a Soviet NATECH grants the technology required by CIVAN;
PTROOP instead requires Third stolen tech. This synthetic leaf profile does
not replace the production retail-layer and real spy-entry checks.
