# Convenience help / 便捷帮助

Convenience Help is an **information enhancement**, not a gameplay feature.
It gathers quick status rows that were previously missing or buried, and shows
them above units and buildings so the player can read the state at a glance.
It never changes simulation state — the lines are presentation only, derived
from frozen per-frame facts.

便捷帮助是对**信息的增强**，不是玩法功能。它把原本缺失或藏在深处的关键状态，
以快捷状态行的形式显示在单位与建筑上方，让玩家一眼就能读到当前状况。它不
改动任何 SIM 状态——这些状态行纯属表现层，取自每帧冻结的事实数据。

## What is included today / 当前包含

- **Detailed EVA text / EVA 增强** — fuller EVA messages for deaf and
  hard-of-hearing players, gated by its own `Detailed EVA Text` option.
- **Repair and reload countdowns / 维修与装填倒计时** — time-to-ready for
  friendly units under repair and for buildings reloading their weapon.

The two countdown rows are filtered to the player's own and allied objects;
enemy timings are never shown.

- **EVA 增强** — 为听障与重听玩家提供更完整的 EVA 文字，由独立的
  `Detailed EVA Text` 选项控制。
- **维修与装填倒计时** — 己方正在维修的单位、正在装填的建筑，显示就绪倒计时。

两条倒计时只对玩家自己和友军的对象显示，绝不泄露敌方时间信息。

## Toggle and persistence / 开关与持久化

- In Options → Interface → **Convenience Help**.
- Hotkey **Ctrl+Alt+H** (VeraX-registered key `ConvenienceHelp`, rebindable).
- Stored as `[VERA] ConvenienceHelp=` in `VERA.INI`; the in-match mirror is
  seeded from the profile at match start.

- 位置：Options → Interface → **Convenience Help**。
- 快捷键 **Ctrl+Alt+H**（键名 `ConvenienceHelp`，可重绑）。
- 写入 `VERA.INI` 的 `[VERA] ConvenienceHelp=`，开局从玩家配置载入对局镜像。

## Future scope / 后续

This switch is the umbrella for quick status rows. More informational lines
will be added under it over time; each new line stays presentation-only and
keeps the own/allied filtering rule. It is a growing surface, not a fixed set.

这个开关是快捷状态行的总开关。未来会持续往里补充更多帮助信息行；每条新增
信息保持纯表现层，并遵守"只显示自己/友军"的过滤规则。这是一个会持续扩充的
信息层，而非固定集合。
