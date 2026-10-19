# Superweapon cameo admission and pointer modes

Replay with a local Yuri's Revenge install:

```sh
RA2_DIR=/path/to/yr python -m tools.sidebar_oracle.superweapon_click --check
```

`superweapon_click.json` is native output, consumed by
`sim::superweapon::can_fire_tests` and
`app::input::superweapon_cameo::tests`. The sidecar records the executable hash,
entry points, supplied state and substitutions.

The shared sidebar VM executes original `SuperClass::CanFire` (`6CC360`) and the
whole `SelectClass::Action` (`6AAD00`) Super branch. The original Select constructor
(`6AACB0`) supplies its vtable. The visible entry is RTTI31, index0; the scenarios
supply existing House/Super/type objects and prior local pointer modes. Audio
requests are observed at their device boundary. Immediate `SPECIAL_PLACE` event
construction and enqueue execute; event delivery is outside the fixture.

`CanFire` rejects `Super+70` (suspended). Otherwise `Type+E5` (`UseChargeDrain`)
selects the signed `Super+7C != 0` test; ordinary weapons use `Super+6F`
(`IsReady`). This differs from `ClickFire6CB920`'s timer/grant checks, and does not
govern the Chronosphere's second, post-click map targeting stage.

In Select, right-click clears only the selected Super (`6AAE94`). Left-click
requests `GUIBuildSound` before `CanFire` (`6AAEC0..6AAEEA`). Refusal calls
`ImpatientClick6CC070`, a no-op, preserving placement/repair/sell and the previous
Super selection. An admitted nonzero Action clears those modes, sets the Super
index, calls `UnselectAll48DC90`, and requests `EVA_SelectTarget`
(`6AAF46..6AAFAC`). Clicking the selected cameo again keeps it selected.
Action zero instead queues `SPECIAL_PLACE` at cell(0,0), without that cleanup or
EVA request (`6AAF0E..6AAF44`).

The eighteen controls include ready/charging/held, both mouse buttons, repeated
left-click, Action zero, and charge-drain states0/1/2/-1. One ungranted-but-ready
control isolates the literal CanFire body; it is not evidence of a visible
ungranted cameo. Rust's live view producer exports only granted instances.

Repair and Sell are read independently from Sidebar `+11B0/+11B1`
(`880998/880999`), through their setters `4AC8C0` and `4AC660`. The three
additional mode bytes `88099A..88099C` are also recorded, but do not stand in
for Repair/Sell. Admitted and held cases exercise each mode separately.
The prepared complete Sidebar/Mouse object uses the original `7E1964` vtable
(installed by `5BDA40` at `5BDA4A`). Its cursor reset runs through `5BDA80` and
`5BDC80` with no cursor image loaded (`ABF2DD=1`, `ABF294=null`), taking the
original return before drawing; no cursor setter is substituted.

These are component comparisons, not a whole native power-loss replay, physical
audio or rendered cursor comparison. Retail rules loading and power loss/recovery
are covered separately through Rust's production frame, sidebar and gadget path.
The existing charge-drain launch/lifecycle residual in `superweapon/fire.rs` is
outside this sidebar selection comparison; stock retail rules do not enable it.
