# 通用 HUD 库交接文档

日期：2026-11-22。设计与接口语义见 [技术方案](gui-hud-design.zh-CN.md)。

## 1. 现状

库已通过独立审查并修复，引擎里有两个只用于绘制的使用者：性能面板、mod 在 `veraui.ini` 中自定义的按钮和文字。
两者的旧绘制已删除，输出与被删的代码一致。

| 项 | 值 |
|---|---|
| 分支 | `feature/gui-hud-foundation` |
| 工作树 | `/Volumes/扩展盘/vera20k-build/worktrees/gui-hud` |
| 基点 | 上游 main `257abddce`（写本文时上游为 `79edebfa2`，未合入） |
| 最后一个代码提交 | `ec76fe362` |
| 推送、PR | 都没有；英文草稿在 `/Volumes/扩展盘/vera20k-build/tmp/gui-hud-pr.md` |

## 2. 按阶段 2 计划逐期对照

| 期 | 已做 | 未做 |
|---|---|---|
| 0 基线 | 5000 对象合成 CPU 基准、wasm32 编译检查、被迁部分的新旧对比 | 对局截图和输入回放基线、GPU 基准 |
| 1 底座 | 库修复、渲染适配器、两处绘制挂载 | 输入挂载、重绘闸门接 revision、SidebarModel、绘制副作用移出 |
| 2 试点 | 性能面板代替原计划的试点 | 命令栏、血条；状态模板没有使用者 |
| 3A | 无 | 全部 |
| 3B | mod 自定义按钮和文字的绘制 | 侧边栏、小地图框、编辑器、其他原版部件 |
| 3C | 无 | 全部 |
| 4 收尾 | 两个使用者的旧绘制已删、文档已更新 | 全量点击回归、其他旧路径 |

未做的原因：

- 原版部件（SidebarModel、命令栏、血条、3A-3C）：本次执行开发的 AI 助手判断这属于复刻现有游戏的界面，所以没有做。
  用户不同意这个判断，计划本身也包含这些项；这不是项目或用户的决定。
- 输入挂载：mod 按钮沿用原 gadget 链，保证点击顺序不变；库的输入目前只有测试在用。
- 重绘闸门：两个使用者在绘制时按原有状态更新，原闸门条件仍然成立。第一个自己持有状态或输入的 HUD 必须接 revision。
- 原创 HUD：等确认设计。

## 3. 代码位置

| 文件 | 内容 |
|---|---|
| `gui/src/hud/` | 库：核心、布局、输入、绘制、世界 |
| `src/render/hud_paint.rs` | 适配器：填充、图片、文字，有序输出、CPU 裁剪、缓冲复用 |
| `src/app/diagnostics/performance_overlay/view.rs` | 性能面板的节点树 |
| `src/app/presentation/layout_parts.rs` | mod 自定义部件的节点树；几何仍只由 `placed` 计算 |

适配器没有实现 `External` 和五种世界命令，等第一个使用者再加。

## 4. 已验证（最终代码 `ec76fe362`）

| 项目 | 结果 |
|---|---|
| 性能面板新旧对比，release | 12,636 个场景、1,045,126 个四边形逐位相同，顺序和分段相同 |
| mod 部件新旧对比，release | 6 组全过：全部实例字段及各通道内顺序 |
| gui 测试 | 208 通过；被忽略的计时测试单独通过 |
| 客户端测试，不带零售数据 | 1306 通过、105 忽略（`app::presentation`、`app::diagnostics`、`app::input::gadget_input`、`render::hud_paint`、`ui::`） |
| 引擎检查，普通与 `--features wasm` | 通过 |
| gui 的 wasm32 检查 | 通过 |
| 5000 对象，1 / 4 份模板，p95 | 0.046 / 0.184 ms |
| 性能面板每帧 CPU，新 / 旧 | 两轮 p95：12.54 / 12.63、23.17 / 24.71 µs；p50 慢 0.13-0.21 µs（不到 2%） |

## 5. 没有验证

- **没有运行游戏，没有看过实际画面**，也没有 GPU 回读。一致性来自实例数据对比。
- mod 部件的对比不覆盖 `placed` 和两个调用点；调用点里观战条与平滑侧边栏的处理顺序有挪动，只读代码确认过。
- mod 部件的每帧耗时。
- 带零售数据的测试、命令流与回放、浏览器运行、低端机、Clippy。
- 2 万单位：本阶段口径是 5000。

## 6. 复现

```sh
export CARGO_TARGET_DIR='/Volumes/扩展盘/vera20k-build/cargo-target/vera20k-gui-hud'
export TMPDIR='/Volumes/扩展盘/vera20k-build/tmp'
run() { python3 -m tools.cargo_run --cache-gib 200 --incremental-gib 64 -- "$@"; }

run test -p vera20k-gui --lib
run test -p vera20k --lib -- app::presentation:: app::diagnostics:: app::input::gadget_input:: render::hud_paint:: ui::
run check -p vera20k --lib --bins
run check -p vera20k --lib --bins --features wasm
run check -p vera20k-gui --lib --target wasm32-unknown-unknown
```

新旧对比含有旧绘制的副本，故意不入库。跑之前各追加一行，跑完删掉：

```rust
// src/app/diagnostics/performance_overlay.rs
#[cfg(test)] #[path = "/Volumes/扩展盘/vera20k-build/tmp/gui-hud-panel-parity.rs"] mod parity;
// src/app/presentation/mod.rs
#[cfg(all(test, tests_app_presentation))] #[path = "/Volumes/扩展盘/vera20k-build/tmp/gui-hud-mod-parts-parity.rs"] mod gui_hud_mod_parts_parity;
```

```sh
run test -p vera20k --lib --release -- gui_hud_mod_parts_parity performance_overlay::parity --include-ignored --nocapture --test-threads=1
```

测试运行期间不要改源码。`docs/*` 被 gitignore，新文档要 `git add -f`。

## 7. 使用时必须知道的

- 帧流程：布局、输入、处理事件、改节点、再布局、绘制。`layout` 也可能产生悬停事件、增加 revision。
- 库不是安全边界：`set`、`remove` 和模板修改不检查调用者。
- `remove_origin` 会连同别人挂在它节点下的子节点一起删除。
- 文字不参与自动尺寸；空矩形和负尺寸不绘制，需要时由宿主经 `NodeDraw` 给出精确位置和尺寸。
- 缩放下要与浮点旧代码一致，不能先取整再乘缩放：性能面板第一次对比因此差了 1 像素。
- 绘制命令借用库内数据，跨帧复用缓冲要经过 `recycle`。
- `Checkbox`、`Combo`、`List`、`Trackbar`、`Edit`、`ScrollBar` 在 HUD 中只画背景。

## 8. 已知问题

| 问题 | 影响 | 建议 |
|---|---|---|
| **main 已有 bug**：`text_engine` 的 `shape_tabbed` 对空段排版会 panic（`src/render/text_engine/mod.rs:561`） | 文字以 Tab 开头、结尾或有连续 Tab，且不走 GAME.FNT 原生路径时崩溃；标签来自 mod，可能被外部数据触发，未在对局中复现 | 单开 PR 修；mod 部件的矢量字体对比避开了这种输入 |
| 上层来源的阻挡面板仍会遮住 `Panels` 中宿主浮层的点外关闭 | Escape 仍可关闭 | 接入外部来源前再定 |
| 每个对象只给宿主保底 128 条命令 | 更大的宿主模板可能被省略 | 接入时加诊断 |
| 透明度为 0 的文字仍输出命令 | 只多一点开销 | 需要时再改 |
| `Hud::remove` 对节点总数线性 | 大量逐个删除时变慢 | 用 `remove_origin` 或复用节点 |

## 9. 后续任务

1. 实际运行一次：开性能面板，并加载带 Side、Field 自定义部件的布局，各看一遍。
2. 单独修 Tab 崩溃。
3. 合入最新 main，带零售数据跑相关测试，然后提 PR。
4. 原创 HUD：先确认设计，再接输入和重绘闸门。

推送和开 PR 之前须得到用户同意。提交信息和 PR 描述不加任何 AI 署名。
