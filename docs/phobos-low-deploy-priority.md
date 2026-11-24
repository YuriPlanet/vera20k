# Low deploy priority

`LowDeployPriority=yes` marks a techno the deploy key may skip when the
selection also holds an object that is not low priority.

```ini
[Phobos]
PriorityDeployFiltering=yes

[HARV]
LowDeployPriority=yes
```

## Rules key

`[TechnoType] LowDeployPriority=` (`Valueable<bool>`, default `no`,
`Ext/TechnoType/Body.cpp:767`, declaration `Body.h:42`). Every techno category
reads it.

## Client switch

`[Phobos] PriorityDeployFiltering=`, default `yes` (`Phobos.INI.cpp:46`,
read `:94`). With it off the type key is inert.

## Native evidence

`ProcessDeployCommand_LowDeployPriority` hooks `0x730D0F`
(`Ext/Techno/Hooks.Misc.cpp:931-956`). With the switch on and more than one
object selected, an object whose type has `LowDeployPriority=yes` skips its
deploy event when `ObjectClass::CurrentObjects` holds any techno that is not
low priority; every other selected object deploys as usual. A selection of one
object, or of low-priority objects only, is never filtered.

## Where VERA20K owns it

- `rules::object_type::ObjectType::phobos_low_deploy_priority`.
- `app::persistence::phobos_settings::PhobosClientSettings::priority_deploy_filtering`.
- `app::presentation::state::MatchPresentationState::priority_deploy_filtering`.
- `app::input::dispatch::deploy_key_commands_filtered`: after the native
  admission and deployed/undeployed filters, the priority filter drops only
  low-priority objects while the selection holds a non-low-priority one.

## Residuals

- The native hook tests the raw `CurrentObjects` array, including objects the
  deploy walk would refuse; VERA applies the filter to the same ordered
  selection before dropping anything, so those objects still count as non-low
  priority.
- A techno whose type is unknown to the rules does not participate, which is
  the deploy walk's existing admission behavior.
