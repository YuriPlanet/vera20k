# Optional Ares compatibility

VERA20K can build in partial, Rust-native support for some Ares extensions. It does not load the
Ares DLL and is not general mod support.

Default builds use the retail readers and dispatch; an INI or client setting cannot enable omitted
extensions. Build the supported extensions explicitly:

```sh
cargo run --release --bin vera20k --features ares
```

Ares builds identify themselves in the window title and startup log. Saves retain the build
capability and cannot load in the other mode, even with identical retail INIs.

For ordinary selected-map offline loading, Ares Country loading fields are read
from the retained Country identity. `File.LoadScreenPAL` and the loading text
color bind before Full_Init; the background filename and `LoadScreenText` labels
bind after its second primary Country read and before Type reset. The loading
attempt retains that resolved palette until it retires, instead of reopening a
later Country palette. CnCNet's offline route admits Countries through the same
reader without replaying cached shell settings or consuming a PortPool RNG draw.
Campaign, observer and generated-map presentation retain their existing paths;
this support does not certify custom participant insignia or whole-frame parity.

For offline CnCNet campaign launches, Ares builds resolve the client's logical `Scenario`
against the retained `BATTLEMD.INI` campaign catalog before preparing the ordinary campaign
startup. A matching record keeps its actual index, final movie and continuation identity;
`CampaignID` is client metadata and never selects a retail array entry. The adapter accepts
byte-wide campaign `Side` values without interpreting them as Country indices. If no Scenario
matches, Side 0/1 retain the existing ALL1/SOV1 explicit-mission override when those records
exist; this fallback does not check which campaign owns an arbitrary mission. Other unmatched
selections fail before consuming a match seed. Default builds retain the stock Side 0/1 path.
This is an external-launch adapter capability, not native CnCNet patch parity or a claim that
all custom campaigns and their gameplay extensions are supported.

Supported extensions:

- [Custom armor](ares-custom-armor.md)
- [EVA voice tags and Side selection](ares-eva.md)
- [Generic production prerequisites](ares-prerequisites.md)
- [GenericWarhead](ares-generic-warhead.md)
- [House survival](ares-house-survival.md)
- [Superweapon building gates](ares-superweapon-building-gates.md)
- [Superweapon money](ares-superweapon-money.md)
- [UnitDelivery](ares-unit-delivery.md)
- [INI Include and inheritance](ini-directives.md). The read-only `asset ini-expand` probe works
  in every build.
