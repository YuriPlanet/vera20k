# Modern picture

A VERA-only presentation extension: glow, sharpen, contrast and saturation
over the battlefield. gamemd has no equivalent, so there is no native address
to cite. Nothing reaches `sim/`, the network, a save or a replay; two clients
may differ without a desync.

## The controls

Four sliders like Team Color Strength, each 0 (off) to 10. They sit on the
main menu's **Display** page and on the in-game menu's **Graphics** tab, where
a change shows on the next frame once the menu closes.

| Slider | `[VERA]` key | At 10 |
|---|---|---|
| Glow | `LookGlow=` | light gathered from the pixels around each pixel above a luminance threshold, strength 2.8 |
| Sharpen | `LookSharpen=` | unsharp mask against the four neighbours, amount 0.9 |
| Contrast | `LookContrast=` | contrast about mid grey, ×1.16 |
| Saturation | `LookSaturation=` | saturation, ×1.44 |

All four at 0 is the default and the **Classic** picture: the pass is skipped
and the frame is the original, byte for byte. A value outside 0..=10 reads as 0.

## What it covers

`render::modern_graphics` runs once over the finished frame, before the
colour-vision filter, built like that filter (a snapshot copy, then one draw).
It is scissored to the battlefield, the tactical rect in window pixels
(`window_ui::battlefield_window_rect`), and reads only inside it, so the
sidebar, the bottom strip and their text keep the original picture and their
light never glows into the battlefield's edge. It is skipped while the in-game
menu is open. The glow's radius follows the window's height (18 pixels at
1080), so it reads the same at 1080p and 4K.

## The in-game tab

The in-game menu has a third page tab, Graphics (the Interface category's
in-game page). A sidebar with no free row for it, as at 640x480, shows the
first two tabs only, and the sliders stay on the main menu's Display page.

## Residuals

- The pass works on the composed picture, so it cannot add detail the art does
  not have: it changes light and colour only.
- The glow takes 40 samples per battlefield pixel; on a weak GPU a high Glow at
  4K can cost frame time. Glow 0 skips those samples' effect but not the pass
  while another slider is above 0.
