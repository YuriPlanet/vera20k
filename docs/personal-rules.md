# Personal gameplay rules

VERA creates `VERARULES.INI` beside `VERA.INI` on normal startup. It contains a few
commented examples and changes nothing until you edit it. VERA never replaces an
existing copy or writes your changes to the original game files.

Use the same section names and keys as the selected game's rules INI. For example,
uncomment the GI example so it reads as follows to change its cost and maximum health:

```ini
[E1]
Cost=100
Strength=200
```

These are example values, not the original defaults. Settings you omit continue to
come from the game or selected mod. Names and keys are case-sensitive. This file
customizes rules supported by your build; it does not add new engine behavior or
replace art, sounds, AI scripts or other mod assets.

Restart VERA after editing and start a new game or mission. Loading an existing
save restores its saved rules.

## Mission and map priority

By default, missions, game modes and maps keep their own rule overrides. Your
changes still apply where those sources leave the settings alone.

To give your changes priority in rule sections those sources read, enable this in
`VERARULES.INI`:

```ini
[VERA]
OverrideMapRules=yes
```

The default is `no`. This option changes gameplay rules in memory; it never edits
the map file or replaces its objectives, triggers, layout or placed objects.
Changing mission rules can affect mission balance or progression. The game's
normal rule dependencies and derived values still apply.

In a network game, every player must use matching personal rules and priority.
They participate in the existing rules compatibility check.

To return to the original behavior, comment out or remove your changes. An empty
or fully commented file has no effect. Deleting the file creates a fresh commented
starter on the next normal startup.
