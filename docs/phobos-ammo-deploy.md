# Deploy ammo

Phobos lets a deployable vehicle change its ammo on a deploy, require an ammo
band to deploy, and auto-deploy inside another band
(`Ext/UnitType/Body.cpp:45-49`; consumers `UnitExt::HandleOnDeployAmmoChange`
`Ext/Unit/Body.cpp:239-256`, `HasAmmoToDeploy` `:218-237`,
`DepletedAmmoActions` `:149-195`).

```ini
[DEPLOYER]                        ; UnitType
Ammo.AddOnDeploy=0                ; int, default 0 (no change)
Ammo.DeployUnlockMinimumAmount=-1 ; int, default -1 (unbounded)
Ammo.DeployUnlockMaximumAmount=-1
Ammo.AutoDeployMinimumAmount=-1
Ammo.AutoDeployMaximumAmount=-1
```

## Behavior

- `Ammo.AddOnDeploy` applies when a deploy completes and again when an
  undeploy completes: `ammo = clamp(ammo + add, 0, Ammo)`. A changed count
  starts the reload clock (`StartReloading`).
- `Ammo.DeployUnlock*` is the `HasAmmoToDeploy` band: with both unset every
  count admits; otherwise the native deploy admission (deploy key,
  `UnitClass::Deploy`, AI/scripted `DeploysInto`) refuses outside the band.
  A bunker-linked Unit is exempt. The simple-deployer Unload mission does not
  pass through this admission.
- `Ammo.AutoDeploy*`: after `FootClass::AI` in `UnitClass::AI` (hook
  `0x736480`), a deploy-capable Unit queues/commences the Unload mission while
  its ammo is inside the auto-deploy band and also admitted by the
  deploy-unlock band, and queues/commences Guard when it leaves. A simple
  deployer leaving the band while airborne re-enters
  `JumpjetLocomotionClass::State::Ascending`.

## Where VERA20K owns it

`rules::phobos_ammo_deploy` reads the five keys. `sim::unit_simple_deploy`
owns the deploy/undeploy completion rewrite (`apply_deploy_ammo_change`), the
`UnitClass::AI` action (`depleted_ammo_actions`) and the admission gate
(`Simulation::phobos_unit_has_ammo_to_deploy`), which
`sim::deploy::can_deploy_slash_unload` calls as the one
`CanDeploySlashUnload` owner. Ammo writes go through `TechnoAmmo` and the
existing `shared_ammo::start_reloading` owner. The add clamp follows
`std::clamp`'s ordered form, so a type without `Ammo=` (`-1`) answers 0 or -1
instead of panicking.

## Residuals

- `Mark(MarkType::Change)` is not modeled; VERA redraws from live state.
- The cancel arm's `pThis->InAir` is tested as `GetHeight() > 0`.
- `Convert.Deploy`'s Ammo override is read only in `ares,phobos` builds, where
  the conversion exists; `Ammo.AutoConvert*` and the rest of `Convert.*` are a
  separate chain.
