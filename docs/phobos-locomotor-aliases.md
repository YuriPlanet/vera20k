# Optional Phobos semantic locomotor aliases

Build with `--features phobos` to let `Locomotor=` name a movement class in
words instead of a braced CLSID.

```ini
[TANK]                         ; rulesmd.ini / artmd.ini type sections
Locomotor=Drive                ; instead of {4A58CF27-E899-11EC-B909-0800200C9A66}
```

The alias is a value without a leading `{`, matched ASCII
case-insensitively, in the shared resolver every TechnoType and WarheadType
read uses (Phobos `Utilities/TemplateDef.h`, `read<CLSID>`):

| Alias | Movement class |
| --- | --- |
| `Drive` | standard ground vehicles (the ground-mover baseline) |
| `Walk` | infantry ground movement |
| `Hover` | hovering vehicles (Robot Tank, Hover MLRS) |
| `Rocket` | spawned missiles (V3, Dreadnought) |
| `Fly` | true aircraft (Harrier) |
| `Teleport` | chrono movement (instant relocation) |
| `Jumpjet` | jumpjet hover-flight (Rocketeer, Kirov), not `Fly` |
| `Ship` | naval vessels |

Braced CLSIDs keep working unchanged, and a braced value is never treated as
an alias. The dormant Tiberian Sun classes (`Tunnel`, `Mech`, `DropPod`) have
no VERA movement kind, so their aliases resolve exactly like their CLSIDs —
to nothing — rather than selecting a different system. `Chrono` is not an
alias; it keeps the Teleport constructor seed.

## Where VERA20K owns it

- `rules/locomotor_type.rs`: `kind_from_clsid` tries the CLSID table first,
  then `kind_from_alias` under the `phobos` feature. TechnoType and Warhead
  reads share that resolver, so both sides accept the same spellings.

A build without the capability reads only the CLSID table, exactly as before.
