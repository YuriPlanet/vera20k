# Optional Ares Prism forwarding

Build with `--features ares`. Default builds ignore every key below and keep
the stock Prism support: one supporter recruited per Mission_Attack visit,
the bonus counted in `+0x664`.

```ini
[ATESLA]                                ; BuildingType
PrismForwarding=yes                     ; yes, no, forward (support only), attack (fire only);
                                        ; [General]PrismType starts at yes
PrismForwarding.Targets=ATESLA          ; types it may support; default its own type
PrismForwarding.MaxFeeds=8              ; towers that may feed it; else PrismSupportMax; <0 none
PrismForwarding.MaxChainLength=1        ; links behind a firing tower of this type; <0 none
PrismForwarding.MaxNetworkSize=8        ; towers one shot recruits; else PrismSupportMax; <0 none
PrismForwarding.SupportModifier=150     ; percent its support adds; else PrismSupportModifier
PrismForwarding.DamageAdd=0             ; flat damage its support (or shot) adds
PrismForwarding.Intensity=-2            ; supported beam thickness: <0 absolute, >0 per link
PrismForwarding.ChargeDelay=1           ; frames per link behind it before charging (at least 1)
PrismForwarding.ToAllies=no             ; may support allied towers
PrismForwarding.BreakSupport=no         ; a charging supporter may still fire itself
PrismForwarding.SupportWeapon=SupBeam   ; written to the first free slot of 2..17
PrismForwarding.EliteSupportWeapon=     ; same, elite array
```

## Behaviour in Ares builds

- A firing tower recruits its whole network in one Mission_Attack visit,
  stage by stage, nearest first: each tower in range (the support weapon's
  `Range`/`MinimumRange`, else the primary's `Range` plus one cell), rearmed
  on its ReloadTimer, not attacking, building, selling, drained, warped,
  unpowered or unoperated, of a listed type and the same House (or allied
  with `ToAllies`).
- Towers further down a chain start charging first, so every beam arrives
  just before the shot. Each beam adds its modifier and `DamageAdd` to the
  tower it feeds, which passes the sum on; the firing tower's bullet takes
  `((sum + 100) * 256) / 100` and the flat damage.
- A support beam restarts the supporter's ReloadTimer (Techno+1FC, the ammo
  reload clock) with the support weapon's `ROF=` or `PrismSupportDelay=`; its
  weapon rearm timer is untouched.
- A tower destroyed, sold, detached, powered down, drained or warped leaves
  its network at once; a captured one leaves it unless it is a `ToAllies`
  supporter whose new owner stays allied with the chain.
- Every `IsAnimDelayedFire=` building's delayed shot goes through the same
  Update_Prism, and the cleanup events above cancel it.

## Where VERA20K owns it

- `rules/ares_prism_forwarding.rs`: the keys (Initialize `100105A0`,
  LoadFromINIFile `10010680`), retained in `BuildingTypeExtRules`; the
  support weapons' slot, NeverUse and C4Warhead writes (`100103F0`) in
  `rules/native_processing.rs`.
- `sim/world/techno_ai/ares_prism_forwarding.rs`: the network state
  (BuildingExt `+0x10`, saved and hashed), Mi_Attack_IsPrism `100167E0`
  (hook `0x44B2FE`), AcquireSlaves `100185B0`/`10018630`,
  ValidateSupportTower `10018F10`, SetChargeDelay `10018CD0`, Update_Prism
  `100169A0` (`0x4503F0`), FireLaser `10016490` (`0x44ABD0`) and
  RemoveFromNetwork `10018BB0`.
- Hooks at their owners: GetFireError `10016360` (`0x447FAE`,
  `combat/fire_error.rs`), the bullet bonus (`combat/world_fire.rs`), the
  master beam's Intensity of Fire_IsLaser `100563D0` (`0x6FF4DE`,
  `combat/laser.rs`), ReceiveDamage `0x4424EF`, Sell `0x447113`,
  ChangeOwner `100163B0` (`0x448277`, with the radar jammer clear),
  Detach_All `0x44EBF0`, UpdatePowered `0x454B3D`, Install_Drain_Link
  `0x70FD9A`, and TemporalClass::InitiateWarp `0x71AF76` with UnwarpableA
  `0x71AF2B`. `[General]PrismSupportModifier` keeps its Ares percent fix
  (`1005D040`, hook `0x671152`) in `rules/ruleset.rs`.

## Not covered

- `IsRadBeam=` and `IsElectricBolt=` support weapons draw nothing; their
  `Report=` and `ROF=` still apply.
- The Hospital/Armory eviction of the Sell and ChangeOwner hooks.
- The BuildingExt destructor's cleanup for a building removed without a
  Detach_All; the network walks skip such ids.
