# Optional Ares compatibility

VERA20K can build in partial, Rust-native support for some Ares extensions. It does not load the
Ares DLL and is not general mod support.

Default builds use the retail readers and dispatch; an INI or client setting cannot enable omitted
extensions. Build the supported extensions explicitly:

```sh
cargo run --release --bin vera20k --features ares
```

Ares builds identify themselves in the window title and startup log. Saves retain the build
capability and cannot load in another capability combination, even with identical retail INIs.

Ares is one of three orthogonal capabilities: it combines with `phobos` and `npatch`
(`--features ares,phobos,npatch`), and every implemented feature must keep working when the others
are compiled in. The rule and the pattern for two extensions touching one native site are in
[NPatch placeholder](npatch.md#cross-extension-compatibility).

For ordinary selected-map offline loading, Ares Country loading fields are read
from the retained Country identity. `File.LoadScreenPAL` and the loading text
color bind before Full_Init; the background filename and `LoadScreenText` labels
bind after its second primary Country read and before Type reset. The loading
attempt retains that resolved palette until it retires, instead of reopening a
later Country palette. CnCNet's offline route admits Countries through the same
reader without replaying cached shell settings or consuming a PortPool RNG draw.
Campaign, observer and generated-map presentation retain their existing paths;
this support does not certify custom participant insignia or whole-frame parity.

For offline CnCNet campaign launches, both builds pass the client's explicit `Scenario`
independently of campaign record 0, as the upstream spawner's
`StartScenario(ScenarioName, 1, 0)` call does. The retained first record supplies its real CD,
final movie and continuation identity even when its authored Scenario differs. `CampaignID`
and `Side` do not select a retail array entry; an empty catalog fails before reading the seed.
Ares builds additionally accept byte-wide campaign `Side` metadata, while default builds
retain their existing Side 0/1 admission. This does not change named campaigns in VERA's menu.
The [native prefix controls](../tools/input_oracle/campaign_start_prefix.md) demonstrate
filename/index/CD independence; they do not certify the installed legacy mod spawner,
complete mission loading or all downstream campaign extensions.

Supported extensions:

- [Ordinary ground and tower EMP](ares-emp.md) (bounded human Drive/Walk, unteamed AI Drive and independent powered defensive buildings)
- [Custom armor](ares-custom-armor.md)
- [Supplemental string tables](../tools/rules_oracle/ares_csf.md), shared by
  game loading and default `asset csf-get` / `csf-grep` queries.
- [Per-type SHP building palettes](ares-building-palettes.md)
- [EVA voice tags and Side selection](ares-eva.md)
- [Generic production prerequisites](ares-prerequisites.md)
- [Factory-source restrictions](../src/sim/production/can_build.rs) (`BuiltAt`,
  `Factory.ExplicitOnly`) and [Country plan qualification](../tools/rules_oracle/ares_factory_owners.md)
  (`FactoryOwners`, forbidden Countries and permanent plans).
- [NoManualFire](../src/app/input/context_order.rs) blocks ordinary manual attacks;
  C4 and infiltration remain incomplete.
- [AttachEffect](ares-attach-effect.md): ground warhead effects and type-owned
  self effects, lifetime, animation, speed/firepower/armor multipliers, temporary
  cloakability and forced decloaking. ROF, Temporal and airborne victim
  collection remain outside this chain; manual game acceptance remains pending.
- [GenericWarhead](ares-generic-warhead.md)
- [House survival](ares-house-survival.md)
- [Superweapon building gates](ares-superweapon-building-gates.md)
- [Superweapon money](ares-superweapon-money.md)
- [UnitDelivery](ares-unit-delivery.md)
- [INI Include and inheritance](ini-directives.md). The read-only `asset ini-expand` probe works
  in every build.
- [Extended authored House ally lists](../tools/input_oracle/ares_allies_reader.md).
