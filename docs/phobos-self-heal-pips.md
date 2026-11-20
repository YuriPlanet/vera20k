# Phobos self-heal status pips (`Pips.SelfHeal.*`)

The stock self-heal marker is drawn inside `DrawPipScalePips`
(`0x0070A50C..0x0070A6BB`): an Infantry or an organic Unit takes the
red-cross frame `0x0D`, a non-organic Unit the wrench frame `0x14`, each
gated on its owner house's `InfantrySelfHeal`/`UnitsSelfHeal` counter and
placed at fixed offsets. Phobos replaces the whole function at `0x70A4FB`
(`Ext/Techno/Hooks.Pips.cpp:426-433` ->
`TechnoExt::DrawSelfHealPips`, `Ext/Techno/Body.Visuals.cpp:7-120`) and
exposes six `[AudioVisual]` keys:

| Key | Phobos read | Constructor (`Body.h:614-619`) | VERA |
|---|---|---|---|
| `Pips.SelfHeal.Infantry` | `Body.cpp:154` `Valueable<Point2D>` | `13,20` | the Infantry class pair; the arm picks X or Y |
| `Pips.SelfHeal.Units` | `Body.cpp:155` | `13,20` | the Unit/Aircraft class pair; the arm picks X or Y |
| `Pips.SelfHeal.Buildings` | `Body.cpp:156` | `13,20` | the Building class pair (residual) |
| `Pips.SelfHeal.Infantry.Offset` | `Body.cpp:157` | `25,-35` | the Infantry class native offset |
| `Pips.SelfHeal.Units.Offset` | `Body.cpp:158` | `33,-32` | the Unit/Aircraft class native offset |
| `Pips.SelfHeal.Buildings.Offset` | `Body.cpp:159` | `15,10` | the Building class native offset (residual) |

All six are `Valueable<Point2D>` read with `read_phobos_int_pair`. VERA's
owners are `src/rules/phobos_globals.rs` and
`app/presentation/ui_overlays.rs::self_heal_marker`.

## Behaviour

- The arm comes from the type's `SelfHealGainType` (`Ext/TechnoType/
  Body.cpp:957`, already read): `NoHeal` draws nothing; `Infantry` takes the
  hospital arm; `Units` the units arm. Unset keeps the stock classification
  (Infantry and organic Units hospital, other Units units, Aircraft and
  Structures nothing). The classification and the MultiplayPassive gate are
  owned once in `sim/self_heal.rs` and shared with the heal owner.
- The pair, offset and pLocation anchor follow the **drawn class**
  (`Body.Visuals.cpp:91-111`): Infantry uses the Infantry pair/offset, every
  other non-building the Units pair/offset. The arm only picks the pair's
  component: X for the hospital arm, Y for the units arm (`:114`). So an
  organic Unit or a Unit authoring `SelfHealGainType=Infantry` draws the
  Units pair's X.
- The house scan is the heal owner's own
  `phobos_self_heal_source_count` (`sim/world/techno_ai.rs`): the owner
  counter, plus `GainSelfHealFromPlayerControl` (campaign) and
  `GainSelfHealFromAllies` sources, so a drawn marker always matches a heal
  source. `GainSelfHealAllowMultiplayPassive=no` closes it for a
  MultiplayPassive owner.
- The draw offset is the class `Offset` plus the native `pLocation` anchor
  (health anchor `(-15,+10)`, plus the Infantry-only `+11` X at `0x709B14`)
  and, for the Phobos arm, the type's `PixelSelectionBracketDelta` on Y.
- The damaged pulse is unchanged: the session frame modulo the arm's engine
  `SelfHealInfantryFrames`/`SelfHealUnitFrames` period, remainder at most 5
  and Health below Strength selects the darkening material.

## Tests

- `the_self_heal_arm_follows_gain_type`, `authored_self_heal_frames_and_offsets_apply`,
  `house_sources_and_the_damage_pulse_gate_the_marker` in
  `src/app/presentation/self_heal_pip_tests.rs` (production marker owner).
- `the_stock_marker_keeps_its_frames_and_offsets` (stock build).
- `the_self_heal_pip_globals_default_and_read_from_audio_visual` in
  `src/rules/phobos_globals_tests.rs`; the existing show/heal sim tests run
  through the extracted source-count owner.

## Residuals

- The Building arm is not drawn: it needs the building strip's self-heal
  material path and the rules `Height=` anchor, which VERA does not own.
  Trigger: a Building authoring `SelfHealGainType=Infantry/Units` (no retail
  type does); effect: no marker; risk: a custom rules display gap.
- The offset-to-anchor mapping is derived from the stock call site
  (`pLocation` = health anchor `(-15,+10)` + Infantry `+11` X). Phobos'
  defaults therefore land 5 px (units) / 6 px (infantry) from the stock
  centered positions; no native Phobos capture distinguishes that from the
  source formula. Trigger: every Phobos build; effect: a small marker shift;
  risk: cosmetic only.
- A negative authored frame or offset clamps to 0 in `SelfHealMarker`;
  native passes the raw int. Rules authoring error only.
