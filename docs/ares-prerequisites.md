# Optional Ares generic prerequisites

Build with `--features ares` to enable this Rust-native prerequisite subset.
Default builds keep retail prerequisites, including `PrerequisiteProcAlternate`,
and ignore `[GenericPrerequisites]` and Ares's other generic alternate lists.
An INI, map or client setting cannot enable the feature. This feature supplies
no Mental Omega adapter, rules, maps or resources.

```ini
[GenericPrerequisites]
custom=A,B

[General]
PrerequisiteCustomAlternate=TANK,INF

[UNIT]
Prerequisite=custom
```

The member buildings and alternate TechnoTypes must already be registered at
the read point; unknown names are skipped without allocating types. An owned
member building or alternate type satisfies `custom`. Normal human prerequisite
lists require every entry; the AI planned-building check ignores alternates.
Alternate counts use constructor-to-destructor ownership, so limbo objects,
factory-held products and transported units still count.

The retained Rules catalog starts with `POWER`, `FACTORY`, `BARRACKS`, `RADAR`,
`TECH` and `PROC`; custom groups follow in registration order with full indexes
starting at six. Ares resolves BuildingTypes before groups on a name collision;
retail resolves its six group names first. Group lookup is ASCII
case-insensitive, and member references are not recursively expanded.

Each pass reads General members, generic members, then General alternates.
For `custom` these keys are `[General] PrerequisiteCustom`,
`[GenericPrerequisites] custom` and `[General] PrerequisiteCustomAlternate`.
The General suffix lowercases the name and uppercases only an initial ASCII
letter. Section/key reads remain exact-case; registration keeps the first
group spelling. Lists use the shared 2048-byte reader and comma-only tokens.
Missing/empty reads retain prior values; nonempty reads replace them, including
comma-only and all-unknown lists. Stored group names are limited to 31 bytes.

Specific building requirements accept a positive active count. Otherwise, an
installed upgrade can qualify through its declared `PowersUpBuilding` host:
the host needs a positive active count and a matching installed type in any
of its three slots. The Ares fallback scans all matching hosts and does not
filter health, limbo, online, Selling or the separate upgrade-count field.
This applies to direct requirements and immediate group members.

Matching alternate lifecycle callbacks request the existing House tech-tree
and sidebar pass on Foot Put, reached non-building discovery tails, mobile
destruction and ownership change; ownership change requests both houses.
The catalog remains with the Rules registry across ordinary handoffs and Rust
save/load. A destructive scenario reset rebuilds the six defaults. Definitions
enter the simulation configuration hash; House counts and sidebar membership
remain with their existing owners.

## Lists, negatives, theaters and stolen technology

TechnoTypeExt `LoadFromINIFile` (`1003DFB0`) reads, after the `Survivor.*`
keys: `Prerequisite.Lists` (stock ReadInteger, default the current count
minus one, at least 0, plus one; the vector grows or truncates),
`Prerequisite.List0`..`Prerequisite.List<n>` through the same Parse
`100085F0` as `Prerequisite=`, `Prerequisite.Negative`, then
`Prerequisite.RequiredTheaters` (a nonempty value replaces the all-theaters
constructor mask with the FindTheater bits of its names) and, after
CameoPalette, `Prerequisite.StolenTechs` (a nonempty value replaces the set
with each token's `atoi` below 32). List 0 is `ObjectType::prerequisite`: Ares
parses `Prerequisite=` into it with the parser the stock read already uses,
and no stock reader of the stock list remains once CanBuild, the planner
check and the power plant choice are replaced. Lists 1.. and the other keys
live in `AresPrerequisiteRules` (`rules::ares_techno_type`), saved with the
Ares type data and checked against the registry on restore.

`production::requirements_met` (`RequirementsMet @ 10021FF0`) answers, in
DLL order: Unbuildable for a scenario theater outside the mask; Forbidden
when the house owns any `Prerequisite.Negative` entry (HouseOwnsAny
`100081E0`); Overridden for a reverse-engineered type; Incomplete when the
house's HouseExt StolenTech lacks a required bit; then the existing override,
lab, TechLevel, stolen-side-tech, house, FactoryOwners, shell and tech level
answers. CanBuild (PrereqValidate `10021B00`) refuses every answer below
Complete; the lab boon pick still takes only Forbidden and Incomplete.
`production::owns_prerequisites` (`PrerequisitesMet @ 10021CA0`, the human
arm and the AI power plant filter) and the planner's
`prerequisites_satisfied` (hook `0x505360` -> `10024520` ->
`PrerequisitesListed @ 10021C50`) accept any one fully met list.
`SpyEffect.StolenTechIndex` supplies the bits (`docs/ares-infiltration.md`).

Residuals: the Ares constructor leaves the list vector empty until a Rules
section is first read (Initialize `1003DEC0` sizes it to one), so natively a
listed type without any section matches no list; VERA always has list 0
(empty for such a type). Such a type keeps `TechLevel=-1`, so a human cannot
build it either way; only the planner can differ. The parser and theater
logs are not written.

## Evidence and bounds

The [native control documentation](../tools/rules_oracle/ares_prerequisites.md)
links reproduction commands, the [57-control corpus](../tools/rules_oracle/ares_prerequisites.json)
and [binary identities and execution boundaries](../tools/rules_oracle/ares_prerequisites.meta.json).
Original Ares 3.0 instructions establish parser/load behavior, owned/planned
checks, installed-upgrade fallback, clear/default lifecycle and 11 alternate
refresh-hook controls. Those controls supply registries, counters, installed
slots, ReadString results and selected runtime services. Whole installation,
Put, destruction, discovery and ownership transitions are not executed by
these component controls. Caller ordering rests on instruction reads. No RNG
draw, timer write or detach call occurs in the inspected prerequisite bodies;
supporting constructors and destruction retain their own lifecycle effects.

The selected scope is a fresh match with rules/type loading finished before
membership queries, and fixed alternate membership during play. Residual:
Ares saves an initialized flag and cached per-type alternate membership;
restoring that native cache or rereading INIs after it was filled is outside
the established coverage. Such loads/rereads can affect later refresh requests
and leave sidebar or production eligibility stale. Rust catalog persistence
does not demonstrate native cached-save or live-reread parity.

Physical INI parsing, custom-key registration, complete CanBuild, whole DLL
startup and whole-mod parity are not certified by this corpus. Non-ASCII
locale case folding remains a residual for uncommon custom IDs and can change
membership binding. Rust parser/load, ownership, upgrade and lifecycle
regressions are separate checks; PR validation records their actual runs.

Malformed nested generic members are treated as unmet. The native owned check
can index the BuildingType array with their unchecked negative group codes;
this safe rejection does not reproduce that memory-dependent path.
