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

A LAN or online room plays its host's personal rules: the host's file, up to 8 KB,
goes to every player with the room's options, and each player's match plays it in
place of their own. To host a room without them, comment them out and restart VERA.
Your own file still applies to your offline games.
Quick Match plays the original game, without anyone's personal rules
([room rules](Networking.md#room-rules)). A game a launcher starts plays each
player's own file, so there every player needs the same one.

Changes to `[Countries]` or a country's own section reach the game itself, but the
country list on the setup screens keeps the game's own.

To return to the original behavior, comment out or remove your changes. An empty
or fully commented file has no effect. Deleting the file creates a fresh commented
starter on the next normal startup.
