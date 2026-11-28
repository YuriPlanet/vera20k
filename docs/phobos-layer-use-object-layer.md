# Layer.UseObjectLayer

An animation attached to an object normally joins the Ground display layer
whatever its own `Layer=`. Phobos can instead let it follow its owner's layer
or explicitly keep the type's own layer.

```ini
[SOMEANIM]                         ; AnimationType / art section
Layer=Top
Layer.UseObjectLayer=yes           ; nullable boolean, unset = stock Ground
```

Reader: `Ext/AnimType/Body.cpp:92`, `Nullable<bool>` unset at
`Ext/AnimType/Body.h:47,99`.

## Native body

- `AnimClass::InWhichLayer` (`0x424CB0`) returns the attached anim's layer.
  `AnimClass_InWhichLayer_AttachedObjectLayer` (hook `0x424CB0`,
  `Ext/Anim/Hooks.cpp:354-375`) checks the anim's `OwnerObject` and the set
  nullable key: true returns `OwnerObject->InWhichLayer()`, false returns the
  anim type's `Layer`; unset leaves the stock attached answer (Ground).

## VERA

`AnimTypeRuntimeConfig::phobos_layer_use_object_layer` reads the key.
`Simulation::anim_display_layer` keeps the stock attached → Ground answer
unless the key is set; true returns
`Simulation::entity_display_layer(owner, rules)` (the `InWhichLayer` owner),
false falls through to the type's `Layer` arm used by unattached anims.
A terrain-owned anim (no entity owner) keeps Ground because the hook reads
`OwnerObject` only.

## Tests

- `src/sim/anim_class.rs` —
  `layer_use_object_layer_selects_between_the_owner_and_the_type`: a ground
  unit's attached Top anim goes to Ground with `yes`, to Top with `no`, and
  to Ground when unset.

Both the owner arm and the type arm were mutated red and restored.

## Residuals

- The key only affects display-layer membership (draw order); no simulation
  reader consumes `InWhichLayer`.
