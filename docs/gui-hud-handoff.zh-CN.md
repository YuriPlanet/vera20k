# 通用 HUD 库交接文档

日期：2026-11-23。设计与接口语义见 [技术方案](gui-hud-design.zh-CN.md)，计划见 `/Users/libi/Documents/phase2-plan.zh-CN.md`。

## 1. 现状

库已审查修复；引擎里迁到 gui 的有：性能面板、mod 在 `veraui.ini` 中加的按钮和文字、命令栏、侧边栏面板、单位与建筑血条、
超武计时器、聊天与系统消息、提示框、结算横幅、布局编辑覆盖层、小地图（宿主区域）、状态点、单格状态标记、Phobos 护盾条。每项都与它替换的旧代码逐位对比过，旧绘制已删。侧边栏的模型已从视图中拆出（SidebarModel）。

| 项 | 值 |
|---|---|
| 分支 | `feature/gui-hud-foundation` |
| 工作树 | `/Volumes/扩展盘/vera20k-build/worktrees/gui-hud` |
| 基点 | 上游 main `257abddce`，未合入新的 main |
| 最后一个代码提交 | `08823ed19` |
| 推送、PR | 都没有；英文草稿 `/Volumes/扩展盘/vera20k-build/tmp/gui-hud-pr.md`（只写到 11-22 的内容） |

素材：原创机械金属风的界面素材在 `/Volumes/扩展盘/vera20k-build/tmp/gui-art/dist/final/expandmd97.mix`，与原版文件尺寸、帧数、编号一致，
放进游戏目录即替换，删掉即还原；引擎读取逻辑未改。

## 2. 按计划逐期对照（估计整体约 60%）

| 期 | 已做 | 未做 |
|---|---|---|
| 0 基线 | 5000 对象 CPU 基准、wasm32 检查、逐项新旧对比；地图观察截图做端到端对照 | 固定的截图与输入回放基线、GPU 基准 |
| 1 底座 | 库修复、渲染适配器、SidebarModel、消息与计时器的绘制副作用移到 update | 重绘闸门接 revision（推迟，见下） |
| 2 试点 | 命令栏走节点树，血条走状态模板（5000 单位 p95 0.19 ms），公开接口验收测试 `gui/src/hud/client_bridge_tests.rs` | — |
| 3A | 计时器、消息、提示框、结算横幅、布局编辑覆盖层 | 局内对话框的手绘绘制（盘点见下；用户 11-23 定为最后做） |
| 3B | 侧边栏面板（2-5 列、veraui.ini 布局）、mod 部件、小地图作为 External 节点 | 内置部件按 veraui.ini 名字命名（计划放到 3C 后） |
| 3C | 血条；状态点（`scale.*`）；炸弹钟、扳手、断电、等级、自愈、护盾底框、对话气泡（`status.*`）；护盾条（`shield.*`）；编队号、超武倒计时、信息行、建筑状态、数字显示、信标、飘字（`label.*`） | — |
| 4 | 每项已删旧绘制 | 接口文档、全量点击回归 |

用户 2026-11-23 的决定：
- 原版部件的迁移照做（默认素材已是原创，界面不再还原原版外观）。
- 输入挂载（计划第 2 步，CameoGrid 接管点击）跳过：`ui::gadget` 是原版输入的逐条仿真，搬进 gui 会把原版细节带进与游戏无关的库；
  几何已只有一份。将来外部 ui.wasm 的按钮按 mod 按钮的方式登记成 gadget 即可（那时加通用的“HUD 节点 → gadget”桥）。
- 建造栏的图标、进度遮罩、变暗遮罩、文字底条不再包一层节点树：只是原样转交实例，没有结构收益，反而每帧多开销。
- 性能取舍（例如只给屏幕附近的对象抓状态条数据）先找用户权衡，不擅自改。
- 文字类状态（编队号、各类标签、飘字、气泡）不建逐对象节点：只经族模板决定显示与否和整体偏移，颜色仍按阵营动态；
  宿主在偏移为零时不做任何运算，保证逐位一致。电力/主工厂标签的零售测试因中文版 csf 跑不到绘制，证据只有零偏移路径与单元测试。
- 代码量：本分支引擎净增约 2000 行（保留原版精确算式 + 节点树一层）。用户 11-23 定：先按逐字节一致把 3C 做完，
  减量（删对话框手绘、把位置交给树的布局并放宽到“察觉不到”）最后统一做。
- 局内对话框盘点（11-23）：输入与命中已全部走 gui（`DialogController`），绘制多为手工推实例；暂停菜单约 100 行、声音约 125、
  外交约 100、键位约 475、连接约 95、消息框约 170、计分页数百行；`gui_paint.rs` 缺侧边按钮样式、Image、Bar、分组框。
- 重绘闸门接 revision 推迟：迁过来的节点树都是每次绘制时从状态推出的视图，闸门已按这些状态判断（计时器闪烁截止、消息逐字显示等），
  加 revision 不改变行为。等外部模块能直接改树时再接，届时它就是调用者。

## 3. 代码位置

| 文件 | 内容 |
|---|---|
| `gui/src/hud/` | 库；本轮新增 `Element::Pips`（格数由 max 决定）、`Element::Slots` 与 `ObjectView::slots`（宿主给格号、代码、上抬） |
| `src/render/hud_paint.rs` | 适配器；`NodeDraw` 可带深度、宿主算好的整条实例、逐字显示的标签 |
| `src/app/presentation/picture_hud.rs` | 图片节点树：命令栏、侧边栏面板 |
| `src/app/presentation/text_hud.rs` | 底框加文字的节点树：计时器、消息、提示框、横幅文字 |
| `src/app/presentation/health_templates.rs` | 全部内置状态模板（血条、状态点、单格标记、护盾）与按原版浮点步骤定位 |
| `src/app/sidebar_projection.rs` | `SidebarModel`（模拟与存档读的部分）与视图 `SidebarProjectionState` |
| `src/app/presentation/super_timers.rs`、`src/app/input/messages.rs` | update 与只读绘制分开 |

线程：模拟线程只跑 tick；快照抓取（含 `HudFacts`）在主线程加线程池；生成实例、上传、绘制都在主线程，没有单独的渲染线程。

## 4. 已验证

| 项目 | 结果 |
|---|---|
| 命令栏新旧对比 | 4,368,000 种情形、253,746,880 个四边形逐字节一致 |
| 血条新旧对比 | 178,704 种情形、6,462,000 个图元逐位一致；5000 单位 p95 0.190 ms（旧 0.183） |
| 侧边栏面板新旧对比 | 80,000 组随机输入（一半为打乱、隐藏、改坐标的 veraui.ini 式布局）、20,037,053 个四边形逐字节一致 |
| 计时器新旧对比 | 60,000 组、2,977,629 个四边形逐字节一致（两种字体、中文） |
| 消息新旧对比 | 60,000 组、1,223,836 个四边形逐字节一致（逐字显示进度、Tab、中文） |
| 提示框新旧对比 | 60,000 组、1,596,635 个四边形逐字节一致（换行、固定角落、任意色调、有无底框） |
| 横幅文字新旧对比 | 40,000 组、419,409 个字形逐字节一致 |
| 公开接口验收 | 外部来源建按钮面板、收右键点击和命令名、提交血条模板、按来源卸载：通过 |
| 布局编辑覆盖层新旧对比 | 50,000 组、613,073 个四边形逐字节一致 |
| 状态点新旧对比 | 60,000 组、1,110,518 个点逐位一致（含上抬 3 像素的弹药行；首版漏了它，由零售 GPU 测试抓到） |
| 护盾新旧对比 | 80,000 组、4,153,666 个格逐位一致（含 -0.0 锚点） |
| 单格标记 | 单元测试 + 零售 GPU 状态测试（等级、扳手、炸弹钟、自愈）；带与不带 phobos 的界面测试 |
| 小地图 | 雷达开着的铁幕、核弹截图与改前逐像素一致 |
| 5000 单位状态 | 每族一个 World 后：血条 p95 0.224 ms（旧 0.187）；血条+状态点+三族查询 p95 0.538 ms（合在一个 World 时 1.28 ms，已修） |
| 带零售素材与 GPU 的测试 | 命令栏截图对照、侧边栏 8 项、血条与缩放等状态条 15 项通过；两项文字标签测试因本机是中文版 `ra2md.csf` 而失败，与改动无关 |
| 地图观察截图 | 盟军/苏军建造放置局、铁幕与核弹计时器局：与改动前逐像素一致，只有两个矿石闪光像素在任意两次运行间都会变 |
| 客户端全部库测试 | 只有 `architecture_guards::production_dependency_edges` 失败，报的是本分支没改的 `ui/*` 文件 |

## 5. 没有验证

- 浏览器、低端机、Clippy；2 万单位（本阶段口径 5000）。
- 地图观察的 `compare` 拒绝比较两个不同程序，所以截图对照只比画面。
- 3C 未迁部分、提示框等仍是旧代码，不在对比范围内。

## 6. 复现

```sh
export CARGO_TARGET_DIR=/Volumes/扩展盘/vera20k-build/cargo-target/vera20k
export TMPDIR=/Volumes/扩展盘/vera20k-build/tmp/
run() { python3 -m tools.cargo_run --cache-gib 200 --incremental-gib 64 -- "$@"; }
run test -p vera20k-gui --lib
run test -p vera20k --lib -- app::presentation:: app::input:: app::sidebar_projection render::hud_paint
```

新旧对比测试含旧代码副本，故意不入库，都在 `/Volumes/扩展盘/vera20k-build/tmp/`。跑之前在对应文件末尾追加挂钩，跑完删掉：

| 测试文件 | 挂在 | 测试组 |
|---|---|---|
| `gui-hud-command-bar-parity.rs`（模块名 `parity`） | `src/app/presentation/sidebar_command_bar.rs` | `tests_app_presentation` |
| `gui-hud-sidebar-chrome-parity.rs`（`chrome_parity`） | `src/app/presentation/sidebar_build.rs` | `tests_app_presentation` |
| `gui-hud-health-parity.rs`（`parity`） | `src/app/presentation/health_templates.rs` | `tests_app_presentation` |
| `gui-hud-timer-parity.rs`（`timer_parity`） | `src/app/presentation/super_timers.rs` | `tests_app_presentation` |
| `gui-hud-message-parity.rs`（`message_parity`） | `src/app/input/messages.rs` | `tests_app_input_d_z` |
| `gui-hud-tooltip-parity.rs`（`tooltip_parity`） | `src/app/input/tooltips.rs` | `tests_app_input_d_z` |
| `gui-hud-banner-parity.rs`（`banner_parity`） | `src/app/presentation/outcome_banner.rs` | `tests_app_presentation` |
| `gui-hud-layout-edit-parity.rs`（`edit_parity`） | `src/app/input/layout_edit.rs` | `tests_app_input_d_z` |
| `gui-hud-scale-parity.rs`（`scale_parity`）、`gui-hud-shield-parity.rs`（`shield_parity`） | `src/app/presentation/health_templates.rs` | `tests_app_presentation` |

挂钩写法：`#[cfg(all(test, tests_app_presentation))] #[path = "/Volumes/扩展盘/vera20k-build/tmp/<文件>"] mod <模块名>;`，
用 `run test -p vera20k --release --lib -- <模块路径> --nocapture` 跑。

带零售素材的测试要设 `RA2_DIR="$HOME/Desktop/Command & Conquer Red Alert II"` 并加 `--ignored`。
地图观察截图：`env -u RA2_DIR python3 -m tools.map_observation --build-profile fast --profile <配置> --contract $PWD/src/app/diagnostics/tactical_capture/contract.v2.json --cwd <目录> --output <新目录>`；
截图用的工作目录在 `/Volumes/扩展盘/vera20k-build/tmp/gui-art/capture/`（`cwd-retail` 原版，`cwd2` 新素材，`cwd3` 带示例地图），
`config.toml` 里 `input_delay_ticks = 2`；截图模式不读 `mod =`，地图按 `ra2_dir` 查找。

测试运行期间不要改源码。`docs/*` 被 gitignore，新文档要 `git add -f`。

## 7. 使用时必须知道的

- 节点树负责部件的有无、顺序、矩形和属性；精确的浮点位置由宿主按旧算式给出（`NodeDraw`），否则缩放和小数镜头下会差 1 像素。
- `build_text` 与 `build_text_label` 都只在开头加一次镜头偏移：宿主给“位置 + 偏移”、适配器传零偏移，结果逐位相同。
- 新调色板的界面素材要把 1 号色固定为近黑（全局的 DARKEN.SHP 用当前阵营调色板画）。
- 帧流程：布局、输入、处理事件、改节点、再布局、绘制。绘制函数只读状态，推进与音效放在它前面的 update。

## 8. 已知问题

| 问题 | 建议 |
|---|---|
| **main 已有 bug**：`text_engine` 的 `shape_tabbed` 对空段排版会 panic（`src/render/text_engine/mod.rs:561`），不走 GAME.FNT 原生路径、文字以 Tab 开头或结尾时 | 单开 PR 修 |
| `architecture_guards::production_dependency_edges` 在本分支基点就失败 | 与本分支无关，合入 main 后再看 |
| 建筑血条超过 512 格（地基深约 34 格以上）会被截断 | 原版与常见 mod 远不到 |
| 上层来源的阻挡面板会遮住 `Panels` 中宿主浮层的点外关闭 | 接外部来源前再定 |

## 9. 后续任务

1. 已完成：内置部件按 veraui.ini 键名成为命名节点（侧边栏 12 个部件、CommandBar、Timers、Messages、Banner、小地图 `minimap`），
   部件下挂其图块，隐藏部件即整组隐藏；侧边栏树有无部件节点每帧都约 7.0-7.2 µs（旧直接写法 0.7-0.9 µs）。
2. 可选：GPU 基准（一帧各段 CPU/GPU 耗时）；计时数字都是 CPU 准备实例的时间，不是 GPU。
3. 减量：局内对话框改按节点树绘制；把已迁部件的位置交给树的布局（验收放宽到“察觉不到”，需逐部件截图对照）。
4. 接口文档；全量点击回归；接外部模块时接重绘闸门 revision。
5. 单独修 Tab 崩溃；合入最新 main；提 PR（须用户同意）。

推送和开 PR 之前须得到用户同意。提交信息和 PR 描述不加任何 AI 署名。
