# 佣兵点兵场（Muster Field）

VERA20K Wasm 扩展模块的演示：一个小型自走棋战役，由 `rules.wasm`（玩法）和
`ui.wasm`（界面）组成。这是 Rust 版（`sdk/examples/muster-field`）的 TypeScript
移植，行为与 Rust 版相同。玩法见 [docs/wasm-modules/guide.md](../../../../docs/wasm-modules/guide.md)。

## 构建

```sh
cd sdk/typescript
npm install
node bin/vera20k.js build examples/muster-field/rules   # rules/build/rules.wasm
node bin/vera20k.js build examples/muster-field/ui      # ui/build/ui.wasm
```

把 `rules/build/rules.wasm` 和 `ui/build/ui.wasm` 放进同一个 mod 文件夹
（docs/wasm-modules/guide.md）。`data/` 是两个模块共用的表格和协议（Rust 版的
`muster-field-data` crate），不含任何引擎调用。

## 权利声明

该扩展模块（佣兵点兵场 / Muster Field）是 VERA20K Wasm 扩展模块的演示，玩法参考了
B 站 UP 主 MK 的「红警酒馆」。招募、三合一合成、补给、波次作战这套思路来自 MK 的作品，
在此致谢。

该扩展模块代码仅为演示效果，不得单独作为独立 mod 发布。

游戏中的单位、建筑、音效等原版内容属于《命令与征服：红色警戒 2》及《尤里的复仇》的
版权方（Electronic Arts）。该扩展模块不包含这些原版资源，运行时需要自备原版游戏。
