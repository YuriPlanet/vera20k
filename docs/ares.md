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
