# 通用 HUD 库交接文档

日期：2026-11-22。设计与接口语义见 [技术方案](gui-hud-design.zh-CN.md)。

## 1. 一句话现状

通用 HUD 库已实现并通过测试，**尚未被引擎中的任何界面使用**，游戏画面没有变化。

| 项 | 值 |
|---|---|
| 分支 | `feature/gui-hud-foundation` |
| 工作树 | `/Volumes/扩展盘/vera20k-build/worktrees/gui-hud` |
| 基点 | 上游 main `257abddce`（写本文时上游 main 为 `79edebfa2`） |
| 最后一个代码提交 | `c9e629170` |
| 推送、PR | 都没有 |
| 前置工作 | 菜单的 gui crate 迁移（#4973）已合并 |
| 对局内的内置界面 | 未改动，不在本分支范围内 |

## 2. 提交

| 提交 | 内容 |
|---|---|
| `6ce8c8610` | 公共契约：节点种类与属性、层、来源、各模块签名 |
| `5fd73859f` | 布局、有序绘制列表 |
| `bad56e72c` | 状态模板、锚定绘制列表 |
| `497c3eb09` | 输入路由 |
| `ec6f73d9c` | 黑盒测试、基准 |
| `c9e629170` | 修契约缺口，技术方案入库 |

## 3. 代码位置

| 文件 | 内容 |
|---|---|
| `gui/src/hud/mod.rs` | `Hud`、层、来源、查找、额度、revision、`recycle` |
| `gui/src/hud/layout.rs` | 锚点、行列、裁剪 |
| `gui/src/hud/input.rs` | 命中、捕获、悬停、滚轮、关闭 |
| `gui/src/hud/paint.rs` | 绘制命令、合批 |
| `gui/src/hud/world.rs` | 模板、绘制列表、世界命令合批 |
| `gui/src/hud/contract_tests.rs` | 契约回归测试 |
| `gui/src/external_client_tests.rs` | 只用公开接口的黑盒测试（运行器只接受 `--lib`，所以放在 crate 内） |
| `gui/examples/hud_bench.rs` | 性能基准 |

改动过的共享代码：`gui/src/node.rs`（新增种类、属性、`clear`，删除改为非递归）和
`gui/src/shell/geom.rs`（`contains`、`translate` 不再溢出）。现有菜单也使用这两个文件。

## 4. 已验证

| 项目 | 结果 |
|---|---|
| gui crate 测试 | 191 通过，1 个默认忽略 |
| 被忽略的计时测试（release） | 单独运行通过 |
| `cargo check -p vera20k --lib --bins` | 通过，gui crate 无告警 |
| `vera20k` 的 `ui::` 测试，不带零售数据 | 718 通过 |
| 5000 对象，1 / 4 份模板，p95 | 0.051 / 0.174 ms |
| 300 节点，布局 + 绘制列表，p95 | 0.012 ms |

性能数来自 Apple M4、release，只含库内 CPU。

## 5. 没有验证

- 浏览器（wasm32）编译和性能。
- GPU、文字整形、真实渲染与合批。
- 带零售数据的测试、`app::` 和 `render::` 测试。
- 手动运行游戏。
- 独立审查：契约缺口只来自实现者报告和主会话复核，各模块没有被逐行独立审过。
- Clippy。

## 6. 复现

在工作树根目录执行，编译产物放在外置盘：

```sh
export CARGO_TARGET_DIR='/Volumes/扩展盘/vera20k-build/cargo-target/vera20k-gui-hud'
export TMPDIR='/Volumes/扩展盘/vera20k-build/tmp'
run() { python3 -m tools.cargo_run --cache-gib 200 --incremental-gib 64 -- "$@"; }

run test -p vera20k-gui --lib
run test -p vera20k-gui --lib --release -- --ignored
run check -p vera20k --lib --bins
run test -p vera20k --lib ui::
run build -p vera20k-gui --release --example hud_bench
"$(find "$CARGO_TARGET_DIR" -type f -name hud_bench -path '*release/examples*' | head -1)"
```

- 两个缓存参数不能省：默认预算会清掉其他任务一小时未用的编译缓存。
- 测试运行期间不要改源码，否则该次结果作废。
- `docs/*` 被 gitignore，新文档要 `git add -f`。

## 7. 使用时必须知道的

- 帧流程：布局、输入、处理事件、改节点、再布局、绘制。几何修改到下一次布局才生效。
- 库不是安全边界。`set`、`remove` 和模板修改不检查调用者；桥接层须按来源查询自行检查。
- `remove_origin` 不撤销该来源对他人节点或模板的修改。
- 事件取出时，节点可能已经删除，要容忍 `Stale`。
- 文字不参与自动尺寸：没有宽高的文字节点不绘制。
- `Checkbox`、`Combo`、`List`、`Trackbar`、`Edit`、`ScrollBar` 在 HUD 中只画背景，也没有对应交互。
- 绘制命令借用库内数据；跨帧复用缓冲要经过 `recycle`。
- 命令必须按输出顺序画，只能合并相邻的同材质命令。
- 非宿主来源合计只能用到各上限的四分之三；非宿主来源之间没有各自的配额。
- 句柄只在各自的 `Hud`、`World` 内有效，不能混用。
- 枚举没有稳定的线上编号。

## 8. 已知问题与风险

| 问题 | 影响 | 建议 |
|---|---|---|
| 尚无使用者 | 接口可能不合用 | 先接一个小的原创界面再扩展 |
| 命中为线性扫描 | 数千个重叠嵌套节点时变慢 | 对象级显示用模板，不用节点 |
| 模板匹配随模板数线性增长 | 256 份模板乘 5000 对象会超过 1 ms | 需要时按类型建索引 |
| 每个对象最多 512 条命令 | 超出的模板被静默丢弃 | 接入时加诊断 |
| 外部浮层仍会吞掉世界上的按下 | 须关闭后才能点世界 | 由宿主决定何时移除 |
| 仓库契约写的规模是 2 万单位 | 本库只按 5000 验收 | PR 中写明口径 |

## 9. 后续任务

| 顺序 | 任务 | 完成标准 |
|---|---|---|
| 1 | 独立审查、浏览器编译检查 | 问题修完，检查通过 |
| 2 | 渲染适配器：四种节点命令、五种世界命令，共用实例池 | 顺序正确，缓冲跨帧复用 |
| 3 | 第一个使用者：性能面板（`src/app/diagnostics/performance_overlay.rs`，项目自研） | 内容与开关不变，旧绘制删除，开销不高于 main |
| 4 | mod 作者在布局文件中自定义的按钮和文字 | 命令与显示不变，旧路径删除；不含内置部件和图标网格 |
| 5 | 原创 HUD | 先有自己的设计和素材，再按技术方案第 12 节接入 |
| 6 | 按使用情况修订接口、补文档，提 PR | PR 用英文，写明范围、验证和未覆盖项 |

推送和开 PR 之前须得到用户同意。提交信息和 PR 描述不加任何 AI 署名。
