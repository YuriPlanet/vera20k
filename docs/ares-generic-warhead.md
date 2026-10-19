# Ares GenericWarhead

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
- `SW.AITargeting=Offensive` (default) uses the enemy house's eligible targets
  and existing ion-cannon target values, ignores cloaked targets and chooses
  between equal-valued targets with the Scenario RNG. `None` disables AI
  launching. Other Ares targeting modes are currently unsupported; an explicit
  unsupported mode logs a warning and disables AI launching for this type.

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
| `TryParseTargetMask`, `TryParseHouseMask` | `TargetMask::parse`, `HouseMask::parse`; preserve composable aliases and unknown-token rejection |
| `Evaluate` and its input/result types | `evaluate_target`; same ordered terrain/content/house/shroud/range gates, typed rejection reasons |
| `ResolveSuperweaponLaunchSites` | VERA's retained house building list and shared operational-state query; no OOP/ECS bridge copied |
| `EvaluateSuperweaponTargetPreflight` | Read-only occupancy, alliance and fog observations; the same gate serves cursor, command and AI |
| `TryQueueSupportWarheadSuperweapon` | Existing VERA synchronous area-damage owner, payload validation and source-house attribution |
| GenericWarhead house-filter regression | Ordinary HP damage substitutes for ra2-mono's AttachEffect observation; target requirements never replace warhead filters |

Intentional boundaries/adaptations:

- Only GenericWarhead opts in. Stock superweapon readers and dispatch remain
  unchanged. Automatic target requirements are independent of manual ones;
  the Offensive defaults are `all-technos` and `enemies` even when no explicit
  `SW.AITargeting` is authored.
- `SW.RequiresTarget`, `SW.RequiresHouse`, `SW.AIRequiresTarget`,
  `SW.AIRequiresHouse`, `SW.FireIntoShroud`, `SW.RangeMinimum` and
  `SW.RangeMaximum` are supported. Range limits are inclusive; negative limits
  are disabled. Missing/inactive launchers cannot satisfy a configured range.
- The Rust rules reader keeps VERA's native number parsing (including f32
  widening and leading numeric tokens), not C#'s strict numeric text parser.
  Non-finite or reversed ranges are rejected. A query uses isolated native-cell
  fallback state so hovering cannot change the authoritative shared Dummy.
- The pure kernel retains per-launcher range overrides from the source, but
  GenericWarhead has no per-launcher override producer. EMPulse weapon-range
  fallback, test-range synthetic launchers and additional Ares availability
  rules are not transplanted into this PR. Launch sites use VERA's existing
  provider owner, including native `AuxBuilding` prerequisites.
- ra2-mono's delayed support-effect state and custom extensions are not
  copied: this launch is synchronous as in Ares GenericWarhead. The source's
  broader pipeline is not a claim that VERA now implements AttachEffect/EMP.

- Target masks accept any occupant whose type and house both qualify. This
  differs from Ares's singular `Cell.GetContent()` selection in mixed-occupant
  cells. The Offensive selector uses VERA's stable entity order; exact Ares
  building tie order and RNG draw equivalence have not been demonstrated.

## Compatibility boundary

This is not general Ares compatibility and does not load an Ares DLL.
Only warhead behavior already represented by VERA's shared combat pipeline
is available. Ares warhead extensions such as EMP, attached effects, ripple,
iron-curtain effects and permanent mind control are not added by this feature.
Common Ares superweapon options such as designators/inhibitors,
auto-fire, custom cursors, launch animations/sounds and EVA
overrides remain separate work. Charge-drain superweapons remain unsupported.

Implementation references:

- [Ares GenericWarhead documentation](https://ares-developers.github.io/Ares-docs/new/superweapons/types/genericwarhead.html)
- [Ares GenericWarhead activation](https://github.com/Ares-Developers/Ares/blob/master/src/Misc/SWTypes/GenericWarhead.cpp)
- [Ares automatic targeting](https://github.com/Ares-Developers/Ares/blob/master/src/Ext/SWType/Hooks.Targeting.cpp)

The implementation combines the contributor-attributed port, available Ares
source reading and synthetic Rust regressions;
it does not claim a native executable comparison or a retail playtest.
