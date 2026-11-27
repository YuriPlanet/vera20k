# VERA20K development priorities

We want a 20,000-unit battle to feel as natural to play as a small Yuri's Revenge skirmish.
These priorities guide future work. If a released build crashes, goes out of sync or keeps
players from starting a match, fixing that comes first.

## 1. Performance

Keep the game responsive on older PCs, from the first few tanks to a 20,000-unit fight
with 30 players. A short freeze matters even when the average looks good. Use the
30-house benchmark to track average and 99th-percentile simulation tick times alongside
client frame time. Then check how it feels to scroll and give orders in a busy skirmish
on an older PC. [Scale benchmark and targets](scale-benchmark.md)

## 2. Pathfinding

A player should be able to send a group of tanks through a choke point with one order.
Take a replay where tanks pile up at a bridge, repeat the same order, and compare how many
get across, when the last one arrives and whether any need a second order. Keep the
original movement as the default. If a new routing mode changes how armies move, let
players opt in. [Pathfinding at scale](../src/sim/pathfinding/README.md)

## 3. UI/UX

Someone opening VERA20K for the first time should be able to find their game files,
start a skirmish and tell whether their units took an order. Watch a new player do
those three things without coaching. Where they hesitate, check what the screen tells
them. In a crowded fight, selected units, their destination and the production queue
should still be easy to see. [UI and audio tracker](https://github.com/YuriPlanet/vera20k/issues/3833)

## Beyond the first three

- **Reliable multiplayer.** Friends on different systems should be able to join the same
  room and finish a match in sync. If their maps or rules differ, the lobby should tell
  them what to change so they can play. [Multiplayer tracker](https://github.com/YuriPlanet/vera20k/issues/3832)
- **Skirmish AI and campaigns.** A skirmish opponent should build an economy and bring
  an army to the player's base. A campaign player should be able to save mid-mission,
  finish the objective after loading and reach the next mission. [Gameplay and combat](https://github.com/YuriPlanet/vera20k/issues/3830) · [Campaigns](https://github.com/YuriPlanet/vera20k/issues/3834)
- **Community maps and mods.** A map maker should be able to share a map and have a
  friend find it, see which mod it needs and play it. Use that whole trip to decide
  what to improve in the tools and in the game. [Modding guide](modding.md) · [Mod compatibility](https://github.com/YuriPlanet/vera20k/issues/3835)
