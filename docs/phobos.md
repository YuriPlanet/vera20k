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
- [Infantry auto deploy](phobos-infantry-auto-deploy.md) (`InfantryAutoDeploy` for AI GI-like infantry)
- [Fly wobble switch](phobos-fly-no-wobbles.md) (`FlyNoWobbles` for `Locomotor=Fly`, per type and globally)
- [Centred pause-menu background](phobos-center-pause-menu.md) (`uimd.ini [Sidebar] CenterPauseMenuBackground`)
- [Building production queue](phobos-building-queue.md) (queue buildings and start the next after placement)
- [Skirmish AI dehardcoding](phobos-ai-dehardcode.md) (last-legs sale/hunt switches and MCV deploy regroup)
- [Parallel AI queue limits](phobos-parallel-ai-queues.md) (per-category and per-type AI clone forbids)
- [Unlimited skirmish colours](phobos-skirmish-unlimited-colors.md) (spawner colour indices address any `[Colors]` entry)
- [Superweapon sidebar](phobos-superweapon-sidebar.md) (left-edge superweapon cameos, toggle and positional hotkeys)
- [Prone movement speed](phobos-prone-speed.md) (`ProneSpeed` for crawling and non-crawling prone infantry)
- [Type select for buildings](phobos-building-type-selectable.md) (`BuildingTypeSelectable` for every building type)

## Original project and authors

VERA20K implements the behavior in Rust. It does not load `Phobos.dll`, import
Phobos C++ source or redistribute Phobos binaries; the native identities cited
by these pages are the Phobos sources named in each page.

Phobos is developed by the Phobos Developers. The features ported here
originate with:

- Custom radiation types - **Otamaa**, who ported and fixed the **Ares
  developers**' unfinished RadTypes code.
- Shields - **Uranusian**, **secsome**, **Belonit**; shield warhead
  modifications **Starkku**; absorption/passthrough **Morton**; respawn
  animation and weapon **Ollerus**.
- Custom ore spawners - **Kerbiter**.
- Custom gather animations - **secsome**, **Uranusian**.
- Ore-stage animation hiding (`HideIfNoOre` threshold) - **Otamaa**.
- Superweapon techno gates (`SW.AuxTechnos`, `SW.NegTechnos`,
  `SW.TechLevel`) - **NetsuNegi**; their building-upgrade counts -
  **NetsuNegi**, **Ollerus**.
- Digital display - **ststl**, **FlyStar**, **NaotoYuuki**, **Saigyouji**,
  **JunJacobYoung**, **CrimRecya**.
- Placement preview - **Otamaa**, **Belonit**.
- Sidebar GDI/NOD positions - **Belonit**.
- Infantry auto deploy - **TaranDahl**.
- Fly wobble switch (`FlyNoWobbles`) - **Noble_Fish**.
- Centred pause-menu background - **Starkku**.
- Building production queue - **CrimRecya**.
- INI `$Include` / `$Inherits` - **Morton**.
- Skirmish AI dehardcoding (last-legs sale/hunt, MCV deploy regroup) -
  **TaranDahl**.
- Parallel AI queue forbids - **Trsdy**, **NetsuNegi** (by category),
  **Starkku** (by TechnoType).
- Unlimited skirmish colours - **Morton**.
- Superweapon sidebar - **NetsuNegi**, **CrimRecya**.
- Prone speed customization - **TaranDahl**.
- Type select for buildings - **TaranDahl** (code), **Ollerus** (doc).
- Flashing Technos on selecting - **Fryone**.

The project's full contributor list is in
[Phobos CREDITS.md](https://github.com/Phobos-developers/Phobos/blob/develop/CREDITS.md).
VERA's include and inheritance traversal also adapts ra2-mono's `IniFile`;
see [INI directives](ini-directives.md#source-mapping).

简体中文：[扩展支持与原作者](zh-CN/credits.md)。
