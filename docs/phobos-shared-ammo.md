# Shared transport ammo

`Ammo.Shared=yes` on an `OpenTopped=yes` transport lets it feed the rounds it
reloads to its passengers instead of keeping them. Two TechnoType keys
(`Ext/TechnoType/Body.cpp:955-956`, both `Valueable<T>` so an unset key keeps
its constructor default):

| key | default | effect |
|---|---|---|
| `Ammo.Shared` | `no` | the type takes part in the hand-out |
| `Ammo.Shared.Group` | `-1` | on the transport, selects which passengers it feeds |

## How a round moves

`TechnoExt::UpdateSharedAmmo` (`Ext/Techno/Body.Update.cpp:825`) runs from the
`0x006FB086` hook at the entry of `TechnoClass::StartReloading` (`0x006FB080`),
so every reload tick, every damage-readiness write and every fire-driven reload
passes through it:

- The transport needs `Ammo.Shared=yes`, `OpenTopped=yes` and a finite positive
  `Ammo=`.
- Passengers are visited in CargoClass list-head-first order. A passenger takes
  one round when its type sets `Ammo.Shared=yes`, the transport group is
  negative or equals the passenger's `Ammo.Shared.Group`, and its own count is
  below its type's `Ammo=`.
- Each visit moves at most one round; the transport's live count is re-read
  before every passenger, so a transport with fewer rounds than receivers feeds
  only the first ones.

Because the pass runs before the clock body, handing a round out of a
transport that reloaded to full still starts that transport's reload clock with
the reduced count.

## Residuals

- The hand-out runs only where the stock game reaches `StartReloading`. The
  Reload and fire callers require an expired reload clock first; the
  ReceiveDamage readiness path calls it unconditionally after its count write,
  so that path can still hand a round out while the clock is running.
- A transport type with `Ammo=-1` reaches neither the clock nor the hand-out.
- Phobos also calls `StartReloading` from `ReloadInTransport` (`0x51DF82`) and
  `UnitExt::HandleOnDeployAmmoChange` (`Ext/Unit/Body.cpp:252`), neither of
  which VERA reaches yet; when those keys land they need this hand-out too.
- Passenger `InitialAmmo=` is irrelevant here; only the type's `Ammo=` caps a
  receiver, as in Phobos.
