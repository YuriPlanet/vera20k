# AI Takeover（接管电脑玩家）

演示"驱动所属方"（[RFC 0001 第 6i 节](../../../../docs/rfcs/0001-wasm-extension-modules.zh-CN.md#6i-驱动所属方)）：模块接管电脑所属方，用一个很小的脚本 AI 来玩。全部通过命令（`order`），不凭空造单位。

## 它做什么

- **认领**：对每个拥有建造场或基地车的电脑所属方写 `ControlDomains=15`（生产、部队、能力、超级武器四个领域），读回确认；对人类玩家写同样的值会被拒绝。认领后原生 AI 让出这些决定。
- **建基地**：展开基地车（`DeployMcv`），按顺序造电厂、兵营、矿厂、战车工厂（按它的基地车选苏军或盟军建筑）。建筑造好（`Factory.Building.Ready=1`）后，用 `find` 在建造场附近找 `Placeable=1` 的格子，再用 `PlaceReadyBuilding` 放下。
- **出兵和进攻**：有兵营就训练步兵，有战车工厂就造坦克，造好的用 `TakeProduct` 放出来；战车工厂建成、部队凑够 3 个后，用 `AttackMove` 打向人类玩家的基地，之后每 450 帧再派一次。
- **判断建筑是否已建成**：`find` 也会返回工厂里正在造、还没放到地图上的产品，所以某类建筑的数量要多于建筑工厂手里那一个，才算已经建成。
- **结果**：代码里建的触发器监听 `Commanded`（绑定到单位类型）、`OrderRefused`（全局）和 `Birth`，记录交给 `on_events`。开局会故意下一个造不了的命令（还没有战车工厂就造坦克），看它被执行步骤拒绝（原因 `Prerequisite`）。
- `on_frame` 每 15 帧跑一次，每次只处理一个被认领的所属方。

每个功能第一次生效时在日志里写一行 `DEMO-OK …`（`claim`、`deploy`、`build`、`place`、`train`、`attack`、`order-rejected`、`native-idle`，四座建筑都建成时写 `base`），供引擎测试 `ini_demo_mods_reach_their_marks` 检查。矿厂和战车工厂各要 2000 元，按原版速度各需约 1260 帧，所以这个测试要跑 4500 帧（`…|MARKS|4500`）。每 600 帧写一行状态。

## 构建

```sh
cd sdk/typescript && npm install
node bin/vera20k.js build examples/ai-takeover
```

得到 `build/rules.wasm`（TypeScript 写的，和 Rust 版 `sdk/examples/ai-takeover` 行为相同），和 `mod.ini`、`rulesmd.ini` 一起放进 mod 文件夹（见 [docs/wasm-modules/guide.md](../../../../docs/wasm-modules/guide.md)）。mod 的 `rulesmd.ini` 会整个替换原版，所以要把 `[VeraWasm]` 一节加进一份完整副本里。

## 怎么玩

1. 选这个 mod 建房间。`rulesmd.ini` 里的 `[VeraWasm] Controller=yes` 让大厅给每个电脑座位多一个"mod AI"选项（等大厅支持后）。
2. 把电脑座位设成"mod AI"：这个所属方开局就由模块控制，原生 AI 一刻也不运行。
3. 开局后它自己展开基地车、造房子、出兵，几分钟后来打你的基地。

大厅还没有"mod AI"选项时，普通电脑座位也行：模块在第一帧认领它们，原生 AI 从那时起让出。
