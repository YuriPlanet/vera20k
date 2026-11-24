# Deployed Infantry Guard and direct self-fire

The optional `--deployed-guard` mode extends the existing
`infantry_deploy_action.py` owner. It saves64 original Guard-shim controls and47
original deployed-predicate controls in `infantry_deployed_guard.json`. The355
historical deploy/action rows,44 callback rows and52 Unload rows retain their
exact JSON bytes and canonical hashes. The companion records those identities
and rejects drift before and after generation.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.infantry_deploy_action --deployed-guard --write
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.infantry_deploy_action --deployed-guard --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.infantry_deploy_action --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.infantry_action_callback --check
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=. python -m tools.spatial_oracle.infantry_deploy_action --mission-unload --check
```

These execute active-retail `gamemd.exe`.
The entire resident original `.text` at401000, length3E0000, retains SHA256
`4cd5557a7490debc493ff965afc4483d8d2f1065f434f6b665cbb8fc4835b0cc`
for every row. Original Infantry, Cell and constructor-installed Walk vtables are
unchanged. Native slices and generator/shared-owner identities are saved in the
metadata. The only inherited support is OS interlocked operations.

The mechanism owner saved and read back bounded labels/comments in the explicit
`gamemd.exe` Ghidra program on2026-09-30:521320
`InfantryClass__DeployedGuardShim` and522510 `InfantryClass__IsDeployedDoing`.
The first48 original bytes matched the loaded image; the predicate's false/true
returns are52252C/522532. Those annotations distinguish native returns, the
pre6FDD50 stop, callback tails and impact-time RadSite arming. Annotations are
navigation aids; the executable receipts establish the claims above.

The input declares Doing, sequence counts, UndeployDelay, DeployFire/immune bits,
mission/rate, seed, raw site fields, target/firing, ammo/rearm/speed, current Z,
OnBridge/current Cell flags and weapon Range/RadLevel. Outputs retain
`before`/`after`, detailed `guard_before`/`guard_after`, ordered `calls` with actual
arguments and returned EAX, writes, native map query order, `execution` boundary,
unsigned `return_eax`, signed `return_signed`, and every byte of all three
3F4-byte RNG objects before/after. `predicate_rows` execute whole522510; only
Doing27..30 return1, including signed/wide controls outside that range.

## Executed handler order

Guard51F620 and AreaGuard51F640 call the same original521320 shim; only return−1
falls through to their separate Foot bodies. This corpus enters the shim rather
than reproducing those Foot fallbacks or the mission dispatch timer epilogue.

Doing27..30 reaches the deployed arm. A nonnegative Type+6C4 UndeployDelay takes
precedence over DeployFire/Immune:52135F requests unforced Undeploy31, then521374
returns the signed sequence count at Type+E3C bank+460 (`31*36+4`). Physical
RULESMD YURI and YURIPR author `UndeployDelay=150` and `75`, respectively;
ARTMD YuriSequence and YuriXSequence author `Undeploy=301,6,0` and `274,6,0`,
so their Count31 is6 in both. The varied Count31 rows are explicit supplied
controls. Count0 refuses DoAction but still returns0;
negative nonzero counts admit and return their full signed values. Prior Doing27
can refuse unforced31 while the shim still returns count1. This value is a count,
not the configured UndeployDelay or animation timer duration.

With delay−1 and DeployFire, the immune arm resolves the actor's actual current
Cell through41BEA0→Map5657A0, then Cell487C80 reads Cell+F8. GetWeapon1 at5213C3
returns the normal secondary WeaponStruct;5213CB reads **WeaponType+158 RadLevel**.
Original signed division5213D1..5213DD computes its threshold. Original65B510
returns0 for nonpositive site duration; otherwise it performs wrapping DWORD
remaining*level and signed division by duration. No Python arithmetic supplies
these results.

A present site's level at or above the threshold jumps directly to cadence and
retains the old target and firing latch. An absent/below site resolves the current
Cell again, calls original InfantryAssignTarget51B1F0, checks original
InfantryGetFireError51C8B0 with `(Cell,1,1)`, and directly calls
InfantryFire51DF60 with `(Cell,1)` on error0. The actual class target setter requests
Deployed28: prior Doing29/30 changes to28 before the fire check; prior Doing27
refuses this unforced action. AssignTargetNULL requests28 again. This action order
is captured rather than inferred from final target storage.

Two accepted boundaries are explicit. `base_fire_boundary=stop` reaches the real
base6FDD50 entry after original51DF70 clears68D, with current Cell target still
retained. That prefix has no Guard return. Separate `supplied_return` rows replace
**only base6FDD50** with their declared EAX and original8-byte argument cleanup.
They then execute the original Infantry wrapper, Guard targetNULL setter,
unforced DeployedFire29 and signed bank+418 count return (`29*36+4`). This callback
does not synthesize projectile/rearm/radiation/damage/sound or consume omitted
base-body RNG. Its0/1 return controls show the Guard suffix's bounded behavior;
they are not native Bullet-pointer results. Count0 leaves Doing28 but returns0;
negative nonzero counts enter29 and return the full signed count. Callback rows
retain target-clear→action ordering and their original sequence timer writes.

Real admission refuses ammo0 with error1, active rearm with3, fraction just above
native0.1 with7, falling with5, and the short deck range with8. It clears the target
before cadence. No FireError, range or locomotor answer is supplied. The actual
Map world lookup, Cell486840, GetWeapon and Walk FireError bodies run. For a
supplied OnBridge1 pose at Z416 over level/slope0, native current-Cell ground Z
remains0: Range415 refuses, Range416 and1024 admit. The1024 control corresponds to
the physical RadEruption Range4 authoring; it does not establish deck placement or
upper occupancy.

Cadence521484 executes current MissionControl rate*900, original ftol, then
Scenario RandomRanged(10,20). Supplied rate0.1/seed31 returns103 with draw13;
seed1 returns105 with draw15. Rate0/negative/1 controls and complete RNG objects
retain native arithmetic/order. Main and MapGen do not advance. The nonimmune
arm executes51F330 and Scenario(0,2), returning91 at rate0.1/seed31. Its empty scan
runs real Foot4D9920 under explicitly supplied empty registered scan storage;
Infantry's wrapper changes the observed mask to189. A separate retained Cell target
passes actual InRange and avoids the scan. Populated-world scanner registration
belongs to the existing FootMissions owner.

## Physical radiation launch dependency: instruction evidence

The companion records exact physical lexical sections from RULESMD.INI and
ARTMD.INI. RadEruptionWeapon authors Projectile=InvisibleLow, AreaFire=yes,
IsRadEruption=no, Range4 and RadLevel500; RadEruptionWarhead authors CellSpread10.
The supplied scalar fixtures do not run their constructors/readers or establish
full retail closure. DESO/YURI ART count strings and their source locations are
recorded as lexical evidence; varied signed sequence records remain supplied
controls.

The original ordinary FireAt body creates a Bullet via46B050 at6FE55D, configures
it through4664C0 at6FF859 and calls the Bullet fire virtual at6FF86C. The ordinary
positive RadLevel arming path is in BulletDetonate4690B0:469130 reads retained
Bullet+130 WeaponType,46913E reads WeaponType+158, and46914A gates positive values.
469150..4691AA converts impact XYZ to a Cell and reads its RadSite. A missing site
allocates74 bytes and calls65B1E0,65B4C0(center),65B4D0(spread from Warhead+124),
65B4F0(level),65B580(activation), then Cell487C70 stores the site. Existing sites
reach65B530 at469206. Those exact original slices are saved. This establishes
impact-time arming by instruction reading under the physical ordinary-weapon
premise; the current controls do not execute full DESO launch or detonation and
make no damage, activation, radiation spreading or scheduler-parity claim.

## Bounds

The fixture supplies actor/type/House/weapon/Warhead/Projectile/RadSite storage,
42 sequence records and concrete Cell10,10/11,10 receivers in a fixed512-stride
map table. Map dimensions128×128 and empty scanner storage are supplied; full
map/topology/registration/House allocation/INI loading are excluded. The real
Walk constructor and all admitted gameplay callees execute unchanged. Health100,
Alive1, Limbo0, ordinary land zone, no transport/radio/temporal/particle links,
normal weapon rank0 and TypeFraidycat0 bound the class state. The shader/rendering,
bridge collapse/repair and persistence mechanisms are outside this receipt.

GameOptions A8EB60 speed0 and FPCW0E7F are explicit supplied input premises;
Options construction/settings/retail configured speed are excluded. Sequence
sounds are supplied absent and original audio gate8464AC is false. Sound request
order is observed, excluding device playback and audio Main RNG. Raw104 timer
auxiliary stack storage is recorded but has no semantic timer claim. Native
execution results and callback-tail results stay separately labeled. Rust and
production validation are reported by the mechanism owner.
