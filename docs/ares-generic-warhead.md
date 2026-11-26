# Ares GenericWarhead

`Base-for-modders` recognizes `Type=GenericWarhead` with Ares Off.

VERA supports an initial, INI-selected `Type=GenericWarhead` implementation.
It detonates an existing warhead at the selected cell without creating a
projectile. Stock superweapon types keep their existing behavior.

```ini
[SuperWeaponTypes]
; Use the next available list entry in your mod.
12=ExampleStrike

[ExampleStrike]
Type=GenericWarhead
SW.Warhead=ExamplePayload
SW.Damage=75
RechargeTime=3
IsPowered=yes
Action=Custom
SidebarImage=YOURICON
SW.AITargeting=Offensive

[ExamplePayload]
CellSpread=2
PercentAtMax=0.5
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%

[YOURBUILDING]
SuperWeapon=ExampleStrike
```

The building must also be a normal, registered BuildingType. Use its
`SuperWeapon2` slot if its first slot already supplies another superweapon.
Provide the normal art/cameo assets for your mod.

## Supported behavior

- `SW.Warhead` resolves through the ordered rules type registry. It can be
  the only reference to that warhead. Later layers retain an omitted reference
  and can clear it with `none`.
- `SW.Damage` is a signed integer, defaulting to zero. There is no implicit
  weapon or default warhead. A missing/null warhead refuses launch without
  consuming the charge.
- Damage uses the existing area-damage owner, including verses, CellSpread,
  damage receivers, ownership attribution and destruction consequences.
  The impact is at the target cell centre, on the bridge deck when present.
  The shared damage-animation and combat-light owners run after the damage.
- The first living, operational building in the owner's building order
  supplying this superweapon and satisfying its configured range is the damage source. Its `DamageSelf` controls
  whether it can damage itself. If no eligible building exists (for example,
  a trigger-granted superweapon), damage still carries the firing house.
- Normal grants, command dispatch, charging, one-time consumption and saves
  use the existing superweapon instance. No parallel superweapon state is
  introduced. Like Ares's new-type action, this type uses the default attack
  cursor (`Action=Custom` is accepted); user `PreClick`, `PostClick` and
  `PreDependent` tags cannot turn it into a two-click superweapon.
- `SW.AITargeting=Offensive` is the handler default, set once on the type's
  first rules pass, so a type that becomes GenericWarhead only in a later layer
  keeps the mode its first `Type`'s handler stored. Targeting, constraints,
  manual/automatic masks and ranges belong to the common SWType metadata,
  shared with `Type=UnitDelivery` and `Type=HunterSeeker`; they are not
  duplicated in the payload.
- `SW.UseAITargeting=yes` makes a left click queue the automatic selector rather
  than entering a target cursor. The selector's result is not checked again
  against different manual masks. Script launches retain their explicit cell.
- `SW.AutoFire=yes` also enables the human-house strategy hook. `SW.ManualFire`
  restricts the cameo only when AutoFire is enabled. Background AI failures
  do not produce a local refusal message.
- `SW.Designators`/`SW.Inhibitors` resolve existing category-distinct TechnoTypes
  in construction order. `SW.AnyDesignator`/`SW.AnyInhibitor` use the same
  collector. `DesignatorRange`/`InhibitorRange` are nullable integer overrides;
  omission uses Sight, and invalid later values retain an earlier override.
- `Message.InsufficientFunds` and `Message.CannotFire` use the existing CSF,
  message-list, colour, expiry and insert-sound owners. `Message.FirerColor`
  and `Message.Color` select the style. Readiness precedes funds, then UseAI
  constraints; a failed queued UseAI selector also warns its owning player.

The shared parser recognizes the installed Ares table's 21 mode names. This
is not a claim that every selector is implemented: Stealth, Base,
MultiMissile, EnemyBase and LightningRandom still decline selection.
The secondary defensive preferred-cell writer is also pending. Stock redirected
superweapons retain their existing dispatch until their Ares dependencies are
ported. The production path has focused coverage for PsychicDominator selection,
including first-maximum ties, designation/inhibition and a queued refusal.

## Port provenance

Original implementation and the reported ra2-mono port are by **baixiong443**.
The contributor identifies the earlier source as
`src/Ra2.Logic/Domains/SupportSuperweapons/SuperweaponTargetPreflight.cs`,
with SHA-256 `CEF5E67CE6EE0B8D09CEB193D474A5C23F36CDBA619249D791DD262C99ECF4D7`
at ra2-mono commit `299548d1`, and contributes this change under VERA's GPLv3.
That donor repository was unavailable during review; its contents, hash and
history have not been independently verified. The mapping below records the
contributor's attribution. Review used the submitted Rust, its production
consumers and the available official Ares sources.

| ra2-mono source | Rust adaptation |
| --- | --- |
| `TryParseTargetMask`, `TryParseHouseMask` | Replaced by Ares's own readers (`0x10034430`, `0x100341D0`): `TargetMask::read_ares`, `HouseMask::read_ares` |
| `Evaluate` and its input/result types | `evaluate_target`; same ordered terrain/content/house/shroud/range gates, typed rejection reasons |
| `ResolveSuperweaponLaunchSites` | VERA's retained house building list and shared operational-state query; no OOP/ECS bridge copied |
| `EvaluateSuperweaponTargetPreflight` | Read-only occupancy, alliance and fog observations; the same gate serves cursor, command and AI |
| `TryQueueSupportWarheadSuperweapon` | Existing VERA synchronous area-damage owner, payload validation and source-house attribution |
| GenericWarhead house-filter regression | Ordinary HP damage substitutes for ra2-mono's AttachEffect observation; target requirements never replace warhead filters |

Intentional boundaries/adaptations:

- The common policy is executed for GenericWarhead, UnitDelivery and HunterSeeker. Automatic
  requirements remain independent of manual ones and take their defaults from
  the selected native targeting mode; stock dispatch stays separate.
- `SW.RequiresTarget`, `SW.RequiresHouse`, `SW.AIRequiresTarget`,
  `SW.AIRequiresHouse`, `SW.FireIntoShroud`, `SW.RangeMinimum` and
  `SW.RangeMaximum` are supported. The four masks take Ares's comma-separated
  names only (`land`, `water`, `empty`, `infantry`, `units`, `buildings`,
  `all`, `none`; `owner`, `allies`, `enemies`, `team`, `trump`, `others`,
  `all`, `none`), with no spaces around commas. An unknown name keeps the
  value from earlier rules layers, as in Ares. Range limits are inclusive; negative limits
  are disabled. Missing/inactive launchers cannot satisfy a configured range.
- SWType metadata uses the shared Ares value readers, retaining binary64
  ranges and earlier values after unsuccessful reads.
  Non-finite or reversed ranges are rejected. A query uses isolated native-cell
  fallback state so hovering cannot change the authoritative shared Dummy.
- The pure kernel retains per-launcher range overrides from the source, but
  GenericWarhead has no per-launcher override producer. EMPulse weapon-range
  fallback, test-range synthetic launchers and additional Ares availability
  rules are not transplanted into this PR. Launch sites use VERA's existing
  provider owner, including native `AuxBuilding` prerequisites.
- GenericWarhead launches synchronously and shares the existing combat
  effect implementations.

- Target masks accept any occupant whose type and house both qualify. This
  differs from Ares's singular `Cell.GetContent()` selection in mixed-occupant
  cells. The Offensive selector uses VERA's stable entity order; exact Ares
  building tie order and RNG draw equivalence have not been demonstrated.

## Compatibility boundary

With Ares Off, a GenericWarhead payload can use the warhead's ripple, Iron
Curtain, EMP, AttachEffect and permanent mind control. The 3.0p1 activation
(`0x10072670`) runs the effects in that order before ordinary damage. A
successful permanent capture of the first object on the selected cell layer
skips area damage, the explosion animation and the flash. Charge-drain
GenericWarhead remains outside this launch path.

With Ares Off, GenericWarhead's ordinary area damage uses the 3.0p1 cell
enumerator. `SW_GenericWarhead::Activate` calls `MapClass::DamageArea` at
`0x10072800` through `0x1000AC30` to `0x00489280`. Its hooks at `0x004895B8`,
`0x004895C7` and `0x004899BE` replace the stock ground offset table and visit
rings through band 256. Per-receiver distance and falloff use the shared
damage path. A stock area call with the same warhead still uses the stock
table when Ares is Off. In that mode the stock receiver still flashes when
an invulnerable target takes a hit.

Implementation references:

- [Ares GenericWarhead documentation](https://ares-developers.github.io/Ares-docs/new/superweapons/types/genericwarhead.html)
- [Ares GenericWarhead activation](https://github.com/Ares-Developers/Ares/blob/master/src/Misc/SWTypes/GenericWarhead.cpp)
- [Ares automatic targeting](https://github.com/Ares-Developers/Ares/blob/master/src/Ext/SWType/Hooks.Targeting.cpp)

The implementation combines the contributor-attributed port, available Ares
source reading and synthetic Rust regressions;
it does not claim a native executable comparison or a retail playtest.
