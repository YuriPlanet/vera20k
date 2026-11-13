# Relation colours

A VERA feature; the original game has no such view.

With many players on the map, their colours get hard to tell apart. Press the
**Relation Colours** key (Alt+A by default) and the game draws everyone by how
they stand with you instead: your own side blue, your allies green and your
enemies red. Press it again for the real player colours.

- **What changes colour:** units and buildings, the outline of a building you
  are placing, bullets drawn in their firer's colours, the dots on the radar
  and the super weapon timers.
- **What keeps the real colours:** the sidebar, the score screen, the message
  list, target and planning lines, range rings, lasers and beacons.
- **Civilians and other passive houses** keep their own colours.
- **Alliances.** A house you ally with during the match turns green, and one
  whose alliance ends turns red.
- **Observers** have no side of their own, so the key changes nothing for them.

## What it is and is not

- Only your own screen changes. Nothing goes over the network, other players
  see their own colours, and replays and saves are not affected.
- Every match starts in the real player colours.

## Keys

The key is **Relation Colours** in the VERA category of Keyboard Options and
can be changed or removed there. In a keyboard file it is
`[Hotkey] RelationColors=`, and `RelationColors=0` removes it.

## Code

`MatchPresentationState::rebuild_relation_color_map` and
`relation_house_colors` in `src/app/presentation/state.rs`; the three colours
are the `RELATION_*` entries in `src/rules/house_colors.rs`.
