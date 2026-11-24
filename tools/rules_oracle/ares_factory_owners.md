# Ares FactoryOwners qualification suffix

`ares_factory_owners.py` reuses `ares_country_veterancy.CountryFixture` and its
existing Probe VM, image mapping, heap, call arena and original-code checks.
It executes installed Ares3.0 at `100221A0` through the original `ret4`, plus
separate skip-tech build-limit and retained GlobalControls component reads.
It also executes the human-predicate/TechLevel branch from `100220F8` to either
`10022118` or `10022325`, with earlier gates declared passed.
All input objects, names, vectors, counters and lexical values are synthetic.

```sh
export VERA20K_ARES_DLL=/path/to/Ares.dll
export VERA20K_GAMEMD_EXE=/path/to/gamemd.exe
python -B -m tools.rules_oracle.ares_factory_owners --check
```

Explicit `--ares-dll` and `--gamemd-exe` arguments are also accepted. The existing
Ares prerequisite owner checks the supported DLL identity; the shared native
runner selects and maps gamemd. Default execution checks without writing.
`--write` deliberately replaces the JSON and metadata after native execution;
`--output` selects another destination. Metadata records the replay command,
runtime, assumptions, substitutions and canonical payload digest.

## Coverage and interpretation

The first eight `rows` preserve the prior native qualification controls: empty
restrictions, matching live factory, Country mismatch, cross-category
HasAllPlans on/off, simultaneous allow/forbid for the same candidate, permanent
plans without a live factory, and no live factory or permanent plan.
Two appended rows retain a live factory with NULL OriginalCountry: forbidden-only
restrictions yield suffix status3, while a nonempty allowed list yields status2.
Their `input.factories: [null]` denotes that live Building and its NULL pointer;
an empty array denotes no live Building.

Each row records its exact supplied input, raw `qualification_status` and reached
native addresses. Values **3 and 2 are suffix statuses**. They are not complete
CanBuild Yes/No results: earlier Requirements gates are declared passed and the
target/House state is bounded. In particular, a permanent-plan candidate may pass
this gate while later actual production still lacks a corresponding factory.

The actual native candidate traversal and same-candidate allow/forbid checks
execute. The live path reads Factory RTTI and HasAllPlans; the permanent path
does not require a live factory. The target uses the real BuildingType vtable
`7E4570`, so its native WhatAmI body executes. The existing fixture rejects
monitored RNG `65C6D0/65C780/65C7E0` and detach `7258D0` calls, and checks its
selected original/Ares code spans unchanged.

The corpus was migrated from the earlier private eight-control experiment
without changing its case inputs or results. Migration alone is not a new native
execution. The reusable command above performs a fresh comparison when run;
execution results belong in the task checkpoint or PR, not in this document.

## Build-limit tail and GlobalControls

`build_limit_rows` preserves twelve corrected controls with `input`, `can_build`
and `reached` members. Input supplies signed `limit`, `owned`, `produced`, held
presence/type, `include_queued`, Infantry family/index, human control, session
mode1 and skip-tech=true. Answers are the bounded tail's actual 0/1/-1 results,
separate from qualification's 3/2 statuses. INT_MIN wrapping, zero/negative limits,
matching held Infantry at/over a positive limit, ignored queued items and a held
wrong type are covered.

Actual Counter construction `49F9B0`, CountProduced `10003220`, CountOwned
`10020AF0`, held scan `100212F0/4F83C0/4CA160` and real Infantry type/object
vtable leaves execute without count/get-type/get-held answer stubs. Counts are
**boundary input priors**, not production histories. Produced Counter base is
House+55C8; +55CC is its backing pointer. Incorrect earlier private results using
the latter as the Counter base are excluded. These controls bypass ordinary
qualification, rich FindFactory and AI bypass flags.

`global_controls.initial` records actual constructor `1002A360` phase0 and three
false flags. Its eight `rows` share retained state in order and record
`supplied_ascii`, `before`, `after`, `phase_after` and `read_events`. Actual
`1002B370` phase2 reads, tokenizer `1002A180` and bool leaf `10076CC0` execute;
only ReadString `528A10` is a lexical interface. Successful input prefixes write
difficulty slots2/1/0; first failure stops, missing/empty retains, and the fourth
value is ignored. These controls omit unrelated keys and do not execute phase0
registry initialization, installed Process-hook dispatch or AI CanBuild.

Installed caller/lifecycle instruction evidence additionally places the global
reader at Process entry `668BF0 ->1002C970 ->1002B490`, before Colors/type
allocation. Type reset `6686C0` retains RulesExt flags and phase, including an
earlier map Process's writes. Actual non-RMG Full_Init ClearClasses
`686B65 ->6851F0`, hooked at `685659 ->100057F0 ->10004D90`, reconstructs
RulesExt at `10004EF7 ->1002A670`; the RMG admission skips this clear. Rust's
retained Rules owner distinguishes that owning reset from Type deletion.
These lifecycle claims rest on instructions, not the eight read executions.

The combined command checks every component group once. Migration preserves the
corrected private tail/global inputs and results; migration itself is not a new
native execution.

## Priors and limits

Retained TypeExt allowed/forbidden Country pointers, HouseExt permanent plans,
current House owned-building membership, preallocated storage and the admitted
entry ABI are supplied inputs. Parent resolution, retained FactoryOwners list
reading, aliases, phase scheduling, Country initialization, capture/deploy,
GrandOpening/upgrade grants, lifecycle transitions, swizzle/save/load and failures
are not executed by the ten qualification controls. Earlier Requirements, full CanBuild,
build limits, prerequisites, FindFactory, BuiltAt, AI exceptions, UI and actual
production output are excluded. These controls establish this bounded suffix,
not whole-mechanism or whole-mod parity.

Rules implementation references live beside `AresFactoryOwners` in
`src/rules/native_processing.rs`. Its production-reader regressions are in
`src/rules/ares_factory_owners_tests.rs`; their list-reader expectations rest on
the separately established installed reader instructions, not these suffix
execution goldens. Qualification consumers may use this JSON directly without
introducing another eligibility owner.

## CountOwned dependencies

The two `owned_count_rows` execute Ares10020AF0 with real concrete WhatAmI, Counter construction and Building Find10007E50. One records signed wrapping addition of an owned Building and its UndeploysInto Unit. The other counts three matching installed slots across two hosts, with all Active counters zero and a deliberately different tracked upgrade count. Inputs are explicit counter/slot priors. These do not establish conversion, upgrade-installation, VehicleThief or whole production-history parity.

## Human-predicate/TechLevel branch

The six `human_tech_level_rows` record `input`, `admitted`, `stop` and `reached`.
Inputs supply `session_mode`, House `human` (+1EC), `player_control` (+1ED) and
Type `tech_level` (+634). Entry `100220F8` uses the original stack frame with
House at stack+18 and Type at stack+20; earlier prefix gates are declared passed.
Actual helper `10020AD0` always reads +1EC and ORs +1ED only in session mode0.
The native branch rejects human TechLevel=-1 at `10022325`; AI skips that test.
Human TechLevel0 and -2 both reach `10022118`. Controls distinguish mode0 from
mode1 for a House with only +1ED set.

`admitted` means only that this branch reached `10022118`, before that boundary's
instruction executes. It does not report full Requirements or CanBuild admission.
The shared runner enforces both permitted endpoints and requires execution of
`10020AD0`; the predicate and branch have no decision stubs. Like the other
groups, these use the existing CountryFixture and check its code spans unchanged.
