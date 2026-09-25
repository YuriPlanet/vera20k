# Main menu Network without IPX: native evidence

Bounded evidence for Main Menu → Network on a machine without an IPX
transport. Retail `gamemd.exe` SHA-256
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`; control
flow read from instructions (Capstone).

## Route

- `0xE2`'s Network (`0x578`) writes 3 (`0x00532051`); `0xE2` is torn down with
  its slide-out (`0x00531F0E`).
- `Main__PrepareSession` state 3 (`0x0052DD75`) sets GameMode 3 and state
  0x10; for GameMode 3 it creates `IPXInterfaceClass` (`0x0052E35D`), runs its
  Winsock init (`0x007B1DE0`), draws the empty shell backdrop (`0x0052E40A`)
  and opens its socket through `IPXManagerClass::Init` (`0x00540A80` →
  `0x0053F540` → `0x007B10C0`: `socket(AF_IPX, SOCK_DGRAM, NSPROTO_IPX)`).
- When the socket fails, GameMode is reset and state 0x12 builds a new `0xE2`
  (`0x0052E425`), which plays its own entry slide. No message is shown on this
  path. With an IPX transport the LAN lobby `0xBB` opens instead.

Retail on macOS/Wine takes the failing branch: the frame captured 1.6 s after
the press shows a new `0xE2` entering (`lan-entry.png`), then the settled main
menu (`lan-steady.png`).

## VERA20k

VERA20k has no IPX transport, so Network always takes the failing branch:
`0xE2` slides out (`ShellExitThen::MainMenu(Network)`), then
`bounce_network_to_main_menu` drops the RA2TS movie session and the `0xE2`
instance, and the next frame builds a new `0xE2` with its entry slide.

## Production comparison (800x600, RGB565 units)

Release build, `--shell-capture main-menu-0xe2-network-bounce`: the route log
records the press, the end of the teardown slide, the new `0xE2`'s entry slide
and the settled menu.

| Comparison | Retail still (SHA-256 prefix) | Differing pixels |
|---|---|---|
| settled menu after the bounce (movie, monitor, cursor masked) | `lan-steady.png` (`02ea8c98`) | 0 |
| retail mid-bounce frame vs `main-menu-0xe2-entry-sequence` ticks (right panel and bottom strip, monitor masked) | `lan-entry.png` (`71ad979c`) | 0 at ticks 15–17 (4,010 at tick 14) |

Retail stills: cnc-ddraw windowed at desktop (560, 200), prefix `Screenshots/`
`yuri's_revenge_2026-09-25_12-15-*`.

## Evidence levels

- **Native behavior established:** route, socket failure branch and return,
  from instructions; the branch retail takes here, from the stills.
- **Parity demonstrated:** the two comparisons above.

## Residuals

- **LAN.** With an IPX transport retail opens the LAN lobby `0xBB`; VERA20k
  has no LAN transport or lobby.
- **Empty backdrop.** Retail draws the empty shell backdrop between the
  slide-out and the new `0xE2` for as long as the IPX init takes; VERA20k
  goes straight to the new `0xE2`.
