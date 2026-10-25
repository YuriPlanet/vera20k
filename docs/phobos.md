# Optional Phobos compatibility

VERA20K can build in partial, Rust-native support for some Phobos extensions. It does not load the
Phobos DLL and is not general mod support.

Default builds use the retail readers and dispatch; an INI or client setting cannot enable omitted
extensions. Build the supported extensions explicitly:

```sh
cargo run --release --bin vera20k --features phobos
```

The capability is independent of Ares and NPatch: a Phobos build needs neither feature, and all
three combine with `--features ares,phobos,npatch`. Every implemented Phobos chain must keep working
when the other capabilities are compiled in — the compatibility rule and the pattern for two
extensions touching one native site are in
[NPatch placeholder](npatch.md#cross-extension-compatibility).

Phobos builds identify themselves in the window title, the startup log, and the build description
drawn at the tactical view's top right during a match. That description is Phobos's own text: the
product name, the version and the build-type warning derived exactly as `Phobos.version.h` derives
them, so a Phobos build prints

```
Phobos v0.5.0.0-alpha1. Please test the build before shipping.
```

in red on black, ten pixels in from the view's right edge — the same string, site and colors as
Phobos's `GScreenClass::DrawText` hook. Saves retain the build capability and cannot load in the
other mode, even with identical retail INIs.

Supported extensions:

- [Custom radiation types](phobos-custom-radiation.md)
- [Shields](phobos-shields.md) (damage absorption, shield bar, shield tint, break, respawn and self-healing)
- [Custom ore spawners](phobos-ore-spawners.md)
- [Custom gather animations](phobos-harvester-gather-animations.md)
- [Ore-stage animation hiding](phobos-hide-if-no-ore-threshold.md)
- [Superweapon techno gates](phobos-superweapon-technos.md)
- [Digital display](phobos-digital-display.md) (numeric health/ammo/cargo/timer values near the health bar)
- [Placement preview](phobos-placement-preview.md) (building artwork and grid translucency while placing)
- [Sidebar positions](phobos-sidebar-gdi.md) (per-side GDI/NOD sidebar coordinates)
- [Building production queue](phobos-building-queue.md) (queue buildings and start the next after placement)
