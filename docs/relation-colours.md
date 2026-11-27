# Relation colours and team colours

Two VERA views; the original game has neither.

With many players on the map, their colours get hard to tell apart. Press the
**Relation Colours** key (Alt+A by default) and the game draws everyone by how
they stand with you instead: your own side blue, your allies green and your
enemies red. Press it again for the real player colours.

Press the **Team Colours** key (Alt+T by default) for the same view with the
enemy side split by team: your own house stays blue, your allies stay green,
and each enemy team takes a colour of its own instead of the single hostile
red. The colours are the ones the lobby already offers — a warm entry of the
rules' `[Colors]` list per team, the first enemy team nearest red, each next
team the colour farthest from those already taken — so a mod's palette stays
in charge and the swatches are ones a player has seen. An enemy the lobby left
off every team takes one of the colours no team holds, and shares once they
are all taken, as the lobby's Random colour does. So do teams past the number
of warm colours the rules have: stock rules give a dozen, which covers every
team of a normal match and reuses colours only in a fifteen-team battle. A
second press returns to the player colours. The two views are exclusive:
turning one on turns the other off.

- **What changes colour:** units and buildings, the outline of a building you
  are placing, bullets drawn in their firer's colours, the dots on the radar
  and the super weapon timers.
- **What keeps the real colours:** the sidebar, the score screen, the message
  list, target and planning lines, range rings, lasers and beacons.
- **Civilians and other passive houses** keep their own colours.
- **Alliances.** Under Relation Colours a house you ally with during the match
  turns green, and one whose alliance ends turns red. Team Colours decides
  blue and green the same way, and takes the teams themselves from the launch
  session, which never changes. It ranks the hostile teams present, so a team
  that leaves the hostile side stops taking a colour: once every house of a
  team is yours, the hostile teams below it move up a colour.
- **Observers** keep a current house even though it is not on the map, and they
  have no allies, so both keys still recolour what the observer sees: Relation
  Colours paints the field red, and Team Colours gives every team its warm
  colour — there is no own blue or ally green to show.
- **Teamless houses.** An enemy the lobby left off every team — every enemy, in
  a free-for-all with Teams off — has no team to group by, so it takes a warm
  colour of its own, drawn from its name so it stays put from frame to frame. A
  player the lobby left off every team still sees the teams around them and
  keeps the blue of their own side.

## What they are and are not

- Only your own screen changes. Nothing goes over the network, other players
  see their own colours, and replays and saves are not affected.
- Every match starts in the real player colours.
- Team Colours does not change the colours a match is played with: the lobby's
  colour list, `[Colors]` and `SkirmishSetups.ini` are untouched, so a player
  can still read the real colours by pressing the key again.

## Keys

Both keys are in the VERA category of Keyboard Options and can be changed or
removed there. In a keyboard file they are `[Hotkey] RelationColors=` and
`[Hotkey] TeamColours=`; `=0` removes the binding. Team Colours ships on Alt+T
and leaves Ctrl+Alt+T (Super Weapon Building Timers) alone.

## Code

`MatchPresentationState::colour_override` and `set_colour_override` in
`src/app/presentation/state.rs` publish the map drawing reads; the view
functions there are `relation_house_colors` and `team_house_colors`. The three
relation colours are the `RELATION_*` entries in `src/rules/house_colors.rs`;
the enemy-team palette is `src/rules/team_colors.rs`, computed from
`[Colors]`/`[SkirmishColors]` at rules load into `RuleSet::enemy_team_palette`
and uploaded with the match's other ramp rows
(`app::loading::init_helpers::voxel_palette_demand`).
