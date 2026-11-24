# Credits indicator step limit

Phobos's `uimd.ini [Sidebar]` switches for the sidebar credits counter, whose
balance rolls toward the actual wallet one step per frame
(`CreditsClass::AI @ 0x004A2600`).

```ini
[Sidebar]                     ; uimd.ini
CreditsIndicator.MaxStep=143  ; integer, default 143
CreditsIndicator.Smooth=yes   ; boolean, default yes
```

## Behavior

The stock step is `|actual - displayed| / 8`, floored at 1 and capped at 143
(`0x4A2721..0x4A2730`). `CreditsIndicator.MaxStep` replaces that cap
(`0x4A2729`): a positive value clamps the step to it, and a value at or below
zero removes the upper bound so a large balance arrives in one frame. The
floor stays.

`CreditsIndicator.Smooth=no` jumps straight to the balance
(`0x4A26E8`): the displayed value is stored unchanged and the counter plays no
`CreditTicks` cue because the native animating/counting-up bytes are never set.

## Where VERA20K owns it

`rules::phobos_sidebar_indicators::SidebarIndicatorsUi` reads both keys once
from `uimd.ini` at startup. The room presentation passes them into
`SidebarProjectionState::advance_credits` at the gameplay-frame seam, and the
same step drives the `CreditTicks` sound. The indicator's per-owner displayed
value stays in the projection state and follows the existing save/load and
scenario-reset rules.

## Residuals

- These keys only shape the roll animation; the wallet and score stay with
  `sim::economy`. With both defaults the counter is the stock one.
