# wgpu-gui：保留式界面库 API（v6）

状态：草案 v6。相对 v5 的变更见 §18。v6 补齐 v5 盲评留下的类型与属性表缺口，并为每条规则与设计空白各定一条决定。

当前实现见 [`vera20k-gui`](../gui/src/lib.rs)。游戏由 `Gui` 管理节点树和各层的布局。宿主用
[`HudPaint`](../src/render/hud_paint.rs) 把绘制命令转换为渲染实例。单位状态条由
[状态模板](../gui/src/status.rs) 和[游戏侧的数据适配器](../src/app/presentation/health_templates.rs)生成。
对局控件的命中与指针路由见 §8.10。其余章节给出设计草案；实际的 Rust 接口以源码为准。

本库从 vera20k 抽离出来，用保留式节点树描述界面，覆盖当前引擎的全部界面功能。它与两处已有设计保持一致：

- `docs/interface-layout.md`：模组作者使用的 `veraui.ini` 格式，键名长期稳定；
- `docs/rfcs/0001-wasm-extension-modules*.md`：客户端 WASM 模块的 UI 树、节点种类和 `client_*` 接口。

## 0. 证据与约定

- 代码证据来自对当前 `main` 的只读分析，行号写作 `文件:行号`，写入前需复核。
- Rust 签名只说明语义，不是最终可编译代码。
- 正文只使用 §2.1 和 §2.3 表格中已列出的方法与属性名。

## 1. 目标与非目标

目标：

1. 保留式：界面是持久的节点树，节点有稳定 `NodeId`，由宿主增删改。
2. 覆盖现有全部界面：主菜单与对话框、对局设置、联机大厅、选项、计分、对局内侧边栏、消息、计时器、横幅、提示框、小地图、玩家自定义布局。
3. 两种布局入口：Rust 代码（`NodeSpec`）与布局文件（`veraui.ini` 语法）。两者生成同一种 `NodeSpec` 树。
4. RFC 的客户端接口是这套 API 的 ABI 翻译层（§15）。
5. 渲染走 wgpu；宿主拥有窗口、`Device`、`Queue` 和 `Renderer`。

非目标：窗口与事件循环、游戏规则与模拟、资源解码、联机、启动流程、种子与地图存储。

## 2. 核心 API

### 2.1 `Gui` 与宿主循环

```rust
pub struct Gui { /* 节点树、对话框栈、动画、事件队列、布局表。不持有 GPU 对象 */ }

impl Gui {
    pub fn new(theme: Theme, assets: Box<dyn Assets>, text: Box<dyn TextSource>) -> Gui;

    // 节点
    pub fn create(&mut self, kind: NodeKind, parent: Option<NodeId>, owner: Owner) -> Result<NodeId, Refused>;
    pub fn create_tree(&mut self, spec: &NodeSpec, parent: Option<NodeId>) -> Result<NodeHandle, Refused>;
    pub fn remove(&mut self, id: NodeId) -> Result<(), Refused>;          // 级联删除
    pub fn find(&self, name: &str) -> Option<NodeId>;                     // 布局中的 Name
    pub fn parent(&self, id: NodeId) -> Option<NodeId>;
    pub fn children(&self, id: NodeId) -> &[NodeId];

    // 属性（按 Prop 分派，§2.3）
    pub fn set(&mut self, id: NodeId, prop: Prop, value: Value) -> Result<(), Refused>;
    pub fn get(&self, id: NodeId, prop: Prop) -> Result<Value, Refused>;
    pub fn set_anchor(&mut self, id: NodeId, anchor: Anchor) -> Result<(), Refused>;
    pub fn set_custom(&mut self, id: NodeId, draw: CustomDraw) -> Result<(), Refused>;  // §12.6

    // 绑定：一个名字可以绑定到多个节点（§7.4）
    pub fn bind(&mut self, name: &str, value: BoundValue);

    // 焦点（§8.4）
    pub fn set_focus(&mut self, id: NodeId) -> Result<(), Refused>;
    pub fn focus(&self) -> Option<NodeId>;
    pub fn focus_rect(&self) -> Option<Rect>;                              // IME 区域

    // 列表与聊天（§5.3、§5.4）
    pub fn row_rect(&self, id: NodeId, row: u32) -> Option<Rect>;
    pub fn chat_append(&mut self, id: NodeId, line: ChatLine) -> Result<(), Refused>;
    pub fn chat_clear(&mut self, id: NodeId) -> Result<(), Refused>;

    // 对话框（§6）
    pub fn push_dialog(&mut self, spec: DialogSpec) -> Result<DialogHandle, Refused>;
    pub fn pop_dialog(&mut self, id: DialogId, result: ModalResult) -> Result<(), Refused>;
    pub fn dialog_root(&self, id: DialogId) -> Option<NodeId>;

    // 动画（§9.2）
    pub fn animate(&mut self, id: NodeId, anim: Anim) -> Result<AnimId, Refused>;
    pub fn cancel_anim(&mut self, anim: AnimId);                           // 不发出完成事件

    // 输入（§8）
    pub fn input(&mut self, event: InputEvent, ui_now: Instant) -> InputResult;

    // 时间（§9.1）
    pub fn update(&mut self, ui_now: Instant, game_now: GameTime);

    // 事件（§10）
    pub fn drain_events(&mut self) -> Vec<UiEvent>;

    // 布局（§11）
    pub fn layout(&mut self, viewport: Viewport);
    pub fn load_layout(&mut self, side: &SideId, mod_file: &LayoutFile, player: Option<&LayoutFile>) -> LayoutOutcome;  // 记录模组基线并合并玩家覆盖层（§11.6、§11.7）
    pub fn begin_edit(&mut self, side: &SideId) -> Result<(), Refused>;    // 编辑模式（§11.6）
    pub fn end_edit(&mut self, side: &SideId, save: bool) -> Result<(), Refused>;  // save = false 等同于 Esc（§11.6）
    pub fn slide_in(&mut self, dialog: DialogId, from: Side, duration_ms: u32) -> Result<AnimId, Refused>;  // 从视口边缘滑入（§9.2）
    pub fn preload(&mut self, names: &[&str], renderer: &mut Renderer);     // 提前上传命名图片（§12.2）

    // 绘制（§12）：渲染器由宿主持有，每帧传入
    pub fn draw(&mut self, frame: &mut FrameTarget, renderer: &mut Renderer);
}

pub struct InputResult { pub consumed: bool, pub cursor: Option<CursorShape> }
// 现有代码：pub struct InputResult { pub consumed: bool, pub passthrough: Option<NodeId> }（§8.10）
pub struct NodeHandle { pub root: NodeId, pub named: BTreeMap<String, NodeId> }
pub struct DialogHandle { pub id: DialogId, pub root: NodeId, pub named: BTreeMap<String, NodeId> }
pub struct Refused { pub reason: RefuseReason }      // NotFound, WrongOwner, WrongKind, Limit, Stale, Locked, NoSuchName
pub enum Owner { Engine, Mod, MapLayout, MapModule, Host }
pub enum Clock { Ui, Game }
```

规则：

- 变更方法返回 `Result<_, Refused>`，失败时状态不变。
- `NodeId` 带代数；删除后的旧 ID 返回 `Refused::Stale`。
- `Owner` 由宿主设置，库只检查（§15.3）。
- `Gui` 不持有 GPU 对象；渲染器由宿主持有，在 `draw` 时传入（§12.1）。
- 线程：`Gui` 只在 UI 线程使用（不是 `Sync`）。模拟线程把值经宿主自己的通道送到 UI 线程，再调用 `bind` 或 `set`。

### 2.2 节点种类 `NodeKind`

| 种类 | 用途 | 子节点（API） | 可聚焦 |
|---|---|---|---|
| `Panel` | 背景、分组 | 任意 | 否 |
| `Dialog` | 对话框根（页面、模态、弹出） | 任意 | 否 |
| `ScrollArea` | 裁剪并滚动的容器 | 任意 | 否（子节点可聚焦） |
| `Row`、`Column` | 顺序排列 | 任意（含 `Row`、`Column`） | 否 |
| `Field` | 战场区域（§11.4） | 任意 | 否 |
| `Text` | 静态或绑定文字 | 无 | 否 |
| `Image` | 图片 | 无 | 否 |
| `Button` | 按钮 | 无 | 是 |
| `Checkbox` | 布尔值 | 无 | 是 |
| `Combo` | 下拉框 | 无 | 是 |
| `List` | 多列列表 | 无 | 是 |
| `Trackbar` | 滑块 | 无 | 是 |
| `ScrollBar` | 滚动条 | 无 | 是 |
| `Edit` | 单行文本框 | 无 | 是 |
| `ChatLog` | 带滚动的消息列表 | 无 | 否 |
| `Bar` | 分段或连续进度条 | 无 | 否 |
| `CameoGrid` | 造物图标网格 | 无 | 是（点击） |
| `Tooltip` | 提示框 | 无 | 否 |
| `External` | 外部纹理（小地图） | 无 | 是（指针） |
| `Decoration` | 附着在世界对象上的装饰 | 无 | 否 |
| `MapMarker` | 小地图或地图上的标记 | 无 | 否 |
| `Custom` | 宿主绘制回调（§12.6） | 无 | 否 |

INI 布局仍然只允许行、列不嵌套（`interface-layout.md`，“A row or column can't hold another row or column yet”），加载器对嵌套报告 `LayoutProblem`。

### 2.3 属性 `Prop` 与值 `Value`（完整表）

```rust
pub enum Value {
    Bool(bool), Int(i32), Float(f32), Text(String), Bytes(Vec<u8>),
    Node(Option<NodeId>), Rgba(Rgba), Rect(Rect), Hold(Hold), Anchor(Anchor),
    Items(Vec<Item>),            // Combo、CameoGrid
    Rows(Vec<Row>),              // List、Row（List 的数据行）
    Columns(Vec<Column>),        // List
    Segments(Vec<Segment>),      // Bar
    Picture(Picture),
    Texture(ExternalId),
    Cells(Vec<Cell>),
    Flags(Vec<bool>),            // CameoGrid.ItemReady
    Ints(Vec<i32>),              // CameoGrid.ItemProgress
    Time(GameTime),
    Result(ModalResult),
    Size(Size),                  // Width、Height（§7.3）
    Fit(Fit),                    // Picture 显示方式（§12.2）
    ActivateOn(ActivateOn),      // Button（§8.2）
    FontRole(FontRole),          // Font（§12.4）
    Align(Align),                // List 列对齐（§5.3）
}
```

| `Prop` | 类型 | 适用种类 | 说明 |
|---|---|---|---|
| `Visible` | Bool | 全部 | 不可见时不绘制、不接收输入、不参与焦点 |
| `Enabled` | Bool | 全部 | 禁用时灰显并禁止交互；作用于子树（§8.9） |
| `Text` | Text | Text、Button、Edit、Dialog 正文 | 直接文字；与 `Label` 互斥 |
| `Label` | Text | Text、Button、Checkbox | CSF 键或字面量（找不到键时显示字面量） |
| `Bind` | Text | Text、Bar | 绑定名（§7.4） |
| `Format` | Text | Text（绑定 Time 时） | 时间格式，默认 `m:ss` |
| `Placeholder` | Text | Edit | 空文本时显示的灰字 |
| `Wrap` | Bool | Text | 按宽度换行；高度随文字增加 |
| `Picture` | Picture | Button、Image、Decoration、Item、Cell | 图片名或纹理（§12.2） |
| `Fit` | Fit | Image、External、Picture 的显示 | `Native`（默认）、`Contain`、`Cover`、`Stretch` |
| `Command` | Text | Button | 激活时在 `Activated` 中回传的命令名（§5.5） |
| `Target` | Node | Button | 命令 `Scroll:±n` 的作用对象（§5.5） |
| `Result` | Result | Button | 激活时关闭所在对话框的结果（§6.2） |
| `ActivateOn` | ActivateOn | Button | `Release`（默认）或 `Press`（§8.2） |
| `Toggled` | Bool | Button | 按下态（用于标签页、模式按钮，§5.5） |
| `Checked` | Bool | Checkbox | |
| `Pos` | Int | Trackbar、ScrollBar | 当前位置 |
| `ShowValue` | Bool | Trackbar | 在滑块上显示数值（§5.7） |
| `Min`、`Max`、`Step` | Int | Trackbar、ScrollBar | |
| `Fill` | Int | Bar | 0..100 |
| `Segments` | Segments | Bar | §4.3 |
| `Smooth` | Int（毫秒） | Bar | 绑定值的平滑时间，默认 0（§5.6） |
| `Items` | Items | Combo、CameoGrid | §5.1 |
| `Rows` | Rows | List | 每行是一组 `Cell`（§5.3） |
| `Columns` | Columns | List（`Column` 定义）、CameoGrid（Int：列数） | 类型由种类决定 |
| `Selected` | Int | Combo、List | 行号或项号，−1 为无 |
| `ItemProgress` | Ints | CameoGrid | 每项 −1..100，−1 不显示 |
| `ItemReady` | Flags | CameoGrid | 每项是否显示 Ready |
| `Tab` | Text | CameoGrid | `Building`、`Defense`、`Infantry`、`Vehicle` |
| `GridRows` | Int | CameoGrid | 行数 1..60 |
| `CellSize` | Int | CameoGrid | 图标的边长（正方形，§5.5） |
| `Scroll` | Int | ScrollArea、ChatLog、CameoGrid、List | 像素（ScrollArea）或行（其余） |
| `ScrollBar` | Bool | ScrollArea、List、ChatLog | 是否显示附属滚动条，默认 1 |
| `Dropped` | Int（只读） | ChatLog | 因历史上限被淘汰的行数 |
| `AutoClose` | Bool | Dialog | 按钮带 `Result` 时是否自动关闭，默认 1（§6.2） |
| `MaxHeight` | Int | Dialog | 消息框的最大高度（屏幕像素，默认屏幕高度的 80%） |
| `Password` | Bool | Edit | 显示为星号 |
| `MaxLen` | Int | Edit | |
| `MaxBytes` | Int | Edit | 文本在宿主代码页中的字节上限（`Gui::set_byte_len`） |
| `SelectOnFocus`、`AllSelected` | Bool | Edit | 每次按下都全选；全选时输入替换全文，退格/删除清空，移动光标取消 |
| `History`、`FadeMs`、`MaxRows` | Int | ChatLog | 历史上限（默认 200）、淘汰淡出时长（默认 800 毫秒）、同时可见行数（默认 8）（§5.4） |
| `Title` | Text | Dialog | 对话框标题（标题栏，消息框与页面共用） |
| `Rect` | Rect | Custom | 宿主每帧设置的矩形数据，作为回调的第三个参数（§12.6） |
| `Gap` | Int | Row、Column | |
| `Hold` | Hold | 布局中的部件；`Dialog`、`Field`、`Panel` 的子节点 | 九个位置或四个边（§11.2） |
| `X`、`Y`、`Width`、`Height` | Int 或 Size | 同上 | 见 §7.2 |
| `Anchor` | Anchor | 任意（通过 `set_anchor`） | 设置后取代 `Hold`、`X`、`Y` |
| `OffsetX`、`OffsetY` | Int | 任意（动画用） | 作用于节点及其子树，包括命中测试（§9.2） |
| `Color` | Rgba | Text、Bar | 文字颜色；Bar 的颜色见 §4.3 |
| `Font` | FontRole | Text、Button、Edit、Combo、List | `Body`、`Title`、`Small`、`Bitmap` |
| `Tooltip` | Text | 全部可交互 | 悬停时显示的文字 |
| `TooltipDelay` | Int（毫秒） | 全部可交互 | 默认取主题 |
| `Clickable`、`Hoverable`、`Dismissable` | Bool | 全部 | 与 RFC 同名（§3.4） |
| `HitTest` | Bool | 全部 | 0 时节点不接收指针（穿透），默认 1（§12.5） |
| `Movable` | Bool | 布局中的部件 | 玩家可在编辑模式拖动（§11.6） |
| `Name` | Text | 全部 | 布局名，用于 `find` |
| `Texture` | Texture | External | 外部纹理 ID（§12.5） |
| `Mapping` | Rect | External | 图像在外部节点内的位置与大小 |

`set` 对类型不符的值返回 `Refused::WrongKind`。`Command`、`Target`、`Result`、`ActivateOn` 只能用于 `Button`。`Result` 为空时按钮不关闭对话框。

### 2.4 Rust 示例：选项页的复选框

```rust
let cb = gui.create(NodeKind::Checkbox, Some(page), Owner::Mod)?;
gui.set(cb, Prop::Label, Value::Text("CSF:Option_Shadows".into()))?;
gui.set(cb, Prop::Checked, Value::Bool(settings.shadows))?;
// 用户切换后，drain_events() 返回 UiEvent::Changed { node: cb, value: Value::Bool(true), final_: true }
```

## 3. 事件

### 3.1 `UiEvent`（完整枚举）

```rust
pub enum UiEvent {
    Activated { node: NodeId, command: Option<Box<str>> },   // Button；command 来自 Prop::Command
    Hovered { node: NodeId, entered: bool },
    PointerLeave { node: NodeId },
    Changed { node: NodeId, value: Value, final_: bool },    // Checkbox、Trackbar、Combo、ScrollBar
    TextChanged { node: NodeId, text: String },              // Edit，每次文字变化
    Submitted { node: NodeId, text: String },                // Edit：Enter（§8.5）
    Cancelled { node: NodeId },                              // Edit：Escape（§8.5）
    ListSelected { node: NodeId, row: u32 },                 // 仅用户操作（§8.3）
    ListDoubleClicked { node: NodeId, row: u32 },            // 双击，或在行上按 Enter
    ListColumnClicked { node: NodeId, column: u32 },
    GridClicked { node: NodeId, index: u32, button: MouseButton },  // 按下时发出
    GridHovered { node: NodeId, index: Option<u32> },
    GridWheel { node: NodeId, lines: i32 },                  // 库已滚动 Scroll
    Scrolled { node: NodeId, position: i32 },                // 用户滚动
    Dragged { node: NodeId, dx: i32, dy: i32, phase: DragPhase },  // 仅编辑模式的 Movable 节点；库已处理，宿主可忽略
    ExternalPointer { node: NodeId, local: Vec2, button: MouseButton, phase: PointerPhase }, // 仅 External
    Dismissed { node: NodeId },                              // Dismissable 节点，或 Popup 的外部按下
    DialogClosed { dialog: DialogId, result: ModalResult, values: Vec<(String, Value)> },  // §6.3
    AnimationFinished { node: NodeId, anim: AnimId },
    Removed { node: NodeId },
    LayoutEdited { side: SideId, changes: Vec<OverrideChange> },  // 保存（§11.6）
    LayoutReset { side: SideId, changes: Vec<OverrideChange> },   // 重置
    LayoutCancelled { side: SideId },                             // Esc 放弃
}

pub enum DragPhase { Start, Move, End, Cancel }
pub enum PointerPhase { Down, Move, Up }
```

- `Changed` 的 `final_` 为 `false` 表示拖动中的中间值（每帧最多一次，§5.7），为 `true` 表示最终值。`Checkbox`、`Combo` 总是 `final_ = true`。
- `Dismissed`、`DialogClosed` 的顺序见 §6.4；事件的顺序总是与产生它们的输入的顺序一致。

### 3.2 与 RFC 事件的映射

| RFC 事件 | 库事件 |
|---|---|
| `Click` | `Activated`（可点击节点） |
| `Hover` | `Hovered` |
| `Dismiss` | `Dismissed` |
| `MapClick` | 由宿主处理（§13.3） |
| `Render` | 宿主的帧循环（不是库事件） |

### 3.3 与 RFC 节点的映射

RFC §7.10 的节点种类全部在 §2.2。库额外的种类（`Panel`、`Dialog`、`ScrollArea`、`Edit`、`Checkbox`、`Combo`、`Trackbar`、`ScrollBar`、`ChatLog`、`Tooltip`、`External`、`Custom`）建议加入 RFC 的 v1 列表（回答 RFC 开放问题 14）。

### 3.4 RFC 与 INI 的属性差异

- `Clickable`：INI 的 `Button` 默认 1，其余默认 0（与 RFC 相同）。
- `Dismissable`：只对可见节点生效（§8.4）。
- `Visible`：INI 不暴露。隐藏仍由“不在 `Parts=` 中”表达。

## 4. 样式与主题

### 4.1 `Theme`

```rust
pub struct Theme {
    pub text: Rgba,
    pub text_warn: Rgba,
    pub text_error: Rgba,
    pub panel: Option<ArtName>,
    pub button_frames: ButtonFrames,   // 默认、悬停、按下（SDBTNANM 2/3/4）
    pub tooltip_delay_ms: u32,
    pub double_click_ms: u32,          // 400，列表双击的间隔（§5.3）
    pub hover_flash_hz: f32,           // 1.0
    pub base_size: (u32, u32),         // 800×600（DLU 画布）
}
```

引擎提供 retail 主题。模组主题不在 v5 范围（§17）。

### 4.2 文字颜色

`Text.Color` 与按钮文字颜色从 `Theme` 取，节点可覆盖。状态行变色（如登录失败变红）：宿主设置 `Color`。

### 4.3 进度条颜色

```rust
pub struct Segment { pub from: u8, pub to: u8, pub color: Rgba, pub blink: bool }  // 按 Fill 0..100 映射
```

侧边栏电量条的绿、黄、红分段是三个 `Segment`。`blink = true` 的段在 `Anim::Flash` 下闪烁（§9.2）。段数是 `Segments` 的长度；连续条使用一个覆盖 0..100 的段。

### 4.4 提示框位置

- 悬停提示（`Tooltip`）：位于指针右下方 12 × 16 像素；超出视口时翻到左上；仍超出则夹紧。`TooltipDelay` 之后显示。
- 提示框是覆盖层，不被任何 `ScrollArea` 裁剪。

## 5. 集合类控件

### 5.1 `Item`（Combo 与 CameoGrid）

```rust
pub struct Item { pub label: String, pub enabled: bool, pub swatch: Option<Rgba>, pub picture: Option<Picture>, pub tooltip: Option<String> }
```

- `Combo`：下拉框。关闭时显示当前项的 `swatch` 与文字。`enabled = false` 的项灰显且不可选。字体与 `Font` 属性相同。
- 下拉列表绘制在覆盖层，不受其所在的 `ScrollArea` 裁剪。下拉列表打开时：**滚动所在的容器会关闭它**（不发出 `Changed`）；点击外部或按 Escape 也关闭它，不发出 `Changed`，且该次 Escape 不触发对话框的 `EscapePolicy`。
- 选中项时发出 `Changed { value: Int(index), final_: true }`。
- 列表超出可见高度时，下拉列表内部滚动（`Scroll`，行号）。
- `OpenAnywhere`（Bool）：按下框面任意处都打开列表，而不只是箭头区（VERA 的选项页列表）。
- `PopupBottom`（Int）：下拉列表上移，使其底边不低于此 y，但不越过窗口顶部（VERA 的 Esc 菜单）。

### 5.2 `ScrollArea`

- 子节点超出时由 `ScrollArea` 裁剪；`ScrollBar` 属性（默认 1）决定是否绘制附属滚动条。
- `Scroll` 为像素偏移，范围 `0..content_height − viewport_height`。
- 滚轮在 `ScrollArea` 上滚动一个步长（默认 30 像素）；库自行滚动并发出 `Scrolled`。
- 设置页的每一行是 `ScrollArea` 的子节点 `Row`（API 允许嵌套，INI 不允许）。

### 5.3 `List`

```rust
pub struct Column { pub title: Option<String>, pub width: Size, pub align: Align }   // Size 只能是 Px 或 Rest
pub struct Row { pub cells: Vec<Cell> }
pub enum Align { Left, Center, Right }
pub struct Cell { pub text: String, pub color: Option<Rgba>, pub picture: Option<Picture> }
```

- `List` 只使用 `Columns` 与 `Rows`。`Items` 不用于 `List`。
- 键盘：`Up`、`Down`、`PageUp`、`PageDown`、`Home`、`End` 改变 `Selected`，发出 `ListSelected`（用户操作）；`Enter` 发出 `ListDoubleClicked`（打开当前行）。
- 双击的时间间隔使用主题的 `double_click_ms`（默认 400 毫秒，与系统一致）。
- `DoubleClick`（Bool，默认开）：关闭时每次按下都选中行，不检查双击（VERA 的 Mods 页）。
- 标题行可点击，发出 `ListColumnClicked`。列宽不能由用户调整（v5 不支持）。
- 滚动条默认显示（`ScrollBar`）。
- 程序设置 `Selected` 不发出事件。
- `row_rect(id, row)` 返回行在视口中的矩形（§7.5）。

### 5.4 `ChatLog`

```rust
pub struct ChatLine { pub text: String, pub color: Rgba, pub expires: Option<Expiry>, pub sticky: bool }
pub struct Expiry { pub ms: u32, pub clock: Clock }
```

- `chat_append(id, line)` 追加一行。超过 `History`（默认 200）时淘汰最旧的行，`Dropped` 计数加一。
- `sticky = true` 的行永不过期，与 `expires` 无关。`sticky = false` 且 `expires = None` 的行也不过期，只在被淘汰时消失。
- `expires` 到期后，行在 `FadeMs`（默认 800）内 alpha 线性降到 0，然后移除。
- `MaxRows`（默认 8）决定同时可见的行数。
- 贴底规则：默认自动跟随最新行。用户向上滚动后停止跟随；滚回底部后恢复跟随。新行到达时只有在跟随状态下才滚动。
- 输入行是独立的 `Edit`，由宿主放在 `ChatLog` 下方的 `Row` 中。`ChatLog` 不包含输入行（§13.1）。
- `ChatLog` 不可聚焦，但接收滚轮（§8.8）。
- `chat_clear(id)` 清空所有行并把 `Dropped` 归零。

### 5.5 `CameoGrid`

| 属性 | 说明 |
|---|---|
| `Tab` | `Building`、`Defense`、`Infantry`、`Vehicle`。由游戏侧的造物列表填充 `Items` |
| `Items` | 由宿主设置；玩家不能通过界面改变它（§7.1） |
| `Columns`（Int）、`GridRows` | 列数 1..8、行数 1..60 |
| `CellSize`（Int） | 图标的边长（正方形）。INI 的 `CellWidth`、`CellHeight` 已由布局读取器支持（侧边栏的 `Cameos`），本草案的 API 仍只有 `CellSize` |
| `Scroll` | 当前行偏移 |
| `ItemProgress`、`ItemReady` | 每项的进度与就绪标记 |

- 现有代码没有 `CameoGrid` 节点：造物格是 chrome 层的 `PressActivates` 按钮（§8.10），图标与遮罩由宿主直接生成实例。下文是草案。
- 标签页：每个标签页是一个 `Button`，`Command = "Tab:Infantry"`，`ActivateOn = Press`，`Target` 为网格。宿主收到 `Activated` 后设置网格的 `Tab` 并重新填充 `Items`；当前标签页的按钮设 `Toggled = true`。
- 滚动按钮：`Button`，`Command = "Scroll:-1"` 或 `"Scroll:+1"`，`Target` 为网格。**库**直接改变 `Target` 的 `Scroll`，不发出 `Activated`。（v4 中“宿主或库”的歧义到此消除。）
- `Ready` 标签的文字为 CSF 键 `CSF:Ready`（主题提供默认文字）。
- `GridClicked` 在按下时发出，携带按钮（右键取消由宿主处理）。
- 滚轮由库处理：库更新 `Scroll` 并发出 `GridWheel`。

### 5.6 `Bar`

- `Segments`（§4.3）、`Fill`（0..100）、`Bind`（绑定名，§7.4）、`Smooth`（毫秒）。段数是 `Segments` 的长度。
- 绘制（`vera20k_gui::shell::bar`，原版 0x0064A7B0）：`Background` 是一像素外框（缺省 255,0,0），`Color` 是其上的填充（缺省白色；对话框绘制取宿主样式色），填充内缩 2 像素、至少 6 像素宽，`Fill` 缺省 100。HUD 的绘制列表与宿主的对话框绘制只用这一种含义。
- 电量条：`Bind = "PowerRatio"`。宿主计算 `Power` 与 `Drain` 的比例并绑定为一个整数（0..100）。库不知道 Power 与 Drain 的存在。
- 平滑：`Bind` 设置目标值；库在 `Smooth` 毫秒内把显示的 `Fill` 从当前值移到目标值，动画由库内部启动，宿主不参与。`Smooth = 0` 时立即显示。绑定的 Bar 不接受 `Anim::Fill`（返回 `Refused::Locked`），因此不存在两者冲突。原版电量条是 CdTimer 步进，与线性平滑不同，列为已知差异（§16）。

### 5.7 `Trackbar` 与 `ScrollBar`

- `Trackbar` 水平绘制；`ShowValue = true` 在滑块上显示当前数值（格式为整数）。`ShowValue = false` 时没有数值牌，轨道占满整个窗口（`0x4AC` 设为 0）。
- 拖动中的 `Changed` 每帧最多发出一次（`final_ = false`），释放时发出 `final_ = true`。
- 键盘：左右方向键减、加一个 `Step`，发出 `Changed`（`final_ = true`）。

### 5.8 `Checkbox`

- 空格（焦点在 `Checkbox` 上）切换 `Checked`，发出 `Changed`（`final_ = true`）。
- 点击切换同样发出 `Changed`。
- `HitRect`（Rect，任意可交互节点）：指针命中的区域，代替 `Rect`。有它的 `Checkbox` 在其中任意处按下都切换（VERA 的 Esc 菜单行），否则只有左上角 18x18 的图标接受按下（`0x006163A0`）。

## 6. 对话框、模态框与消息框

### 6.1 `NodeSpec`、`DialogSpec`

```rust
pub struct NodeSpec {
    pub local: String,                  // 本地名字：在本树中唯一
    pub kind: NodeKind,
    pub props: Vec<(Prop, Value)>,
    pub children: Vec<NodeSpec>,        // 只有容器种类允许非空
    pub owner: Owner,
}

pub struct DialogSpec {
    pub root: NodeSpec,                 // root.kind 必须是 Dialog
    pub kind: DialogKind,               // Page | Modal | Popup
    pub backdrop: Backdrop,             // None | Dim(u8)
    pub default_button: Option<String>, // 本地名字；推送后解析为 NodeId
    pub cancel_button: Option<String>,
    pub initial_focus: Option<String>,
    pub escape: EscapePolicy,           // Cancel | Ignore | Dismiss
    pub auto_close: bool,               // 即 Dialog 的 AutoClose 属性，默认 true
    pub anchor: DialogAnchor,
    pub name: Option<String>,
    pub title: Option<String>,          // 即 Dialog 的 Title 属性（§2.3）
}
pub enum DialogKind { Page, Modal, Popup }
pub enum EscapePolicy { Cancel, Ignore, Dismiss }
pub enum DialogAnchor {
    Center,                                                // 相对视口居中
    Fixed(Rect),                                           // 绝对位置，夹紧到视口
    Beside { rect: Rect, side: Side, offset: i32 },        // 紧挨矩形（如列表行的 row_rect）
    Near { target: NodeId, side: Side, offset: i32 },      // 紧挨节点
}
pub enum Side { Left, Right, Above, Below }
```

- `default_button`、`cancel_button`、`initial_focus` 是本地名字，推送时解析为 `NodeId`。名字不存在时 `push_dialog` 返回 `Refused::NoSuchName`。
- `Beside` 与 `Near` 的位置：先按 `side` 与 `offset` 放置；超出视口时翻到另一侧；仍超出则夹紧。`Near` 的 `target` 必须是已存在的节点；`Beside` 的矩形在推送时取得，之后不跟随（列表滚动后，宿主应重新推送或改用 `Near`）。

### 6.2 按钮的 `Result` 与自动关闭

- 带 `Result` 的按钮激活时：发出 `Activated`；若所在对话框的 `AutoClose` 为 1，对话框立即以该 `Result` 关闭（发出 `DialogClosed`）。
- 没有 `Result` 的按钮只发出 `Activated`，对话框不关闭。宿主可以之后调用 `pop_dialog`。
- 需要校验的对话框（如名字为空时不能确定）：推送时设 `auto_close = false`。按钮激活后宿主校验，合法时调用 `pop_dialog(id, Ok)`；不合法时显示错误，对话框保持打开。这解决了 v4 的“没有否决”问题。
- 默认按钮与取消按钮是普通按钮，规则相同：默认按钮的 `Result` 通常为 `Ok`，取消按钮的 `Result` 为 `Cancel`。
- `pop_dialog` 直接关闭，不需要按钮。
- 忙碌状态（如正在连接）：宿主推送一个 `Modal`，内容为一个 `Text`，没有按钮，`escape = Ignore`。完成后宿主调用 `pop_dialog`。忙碌框之上可以再推送其它模态框；之下的层忽略输入（§6.4）。
- `EscapePolicy` 与 `auto_close` 在推送时固定。要改变它们，宿主关闭对话框并重新推送。
- 屏幕打开没有单独的事件：`push_dialog` 的返回值就是打开完成。需要在打开后执行的工作由宿主在调用之后执行。
- 消息框（§6.5）的所有按钮都带 `Result`，`AutoClose` 为 1。

### 6.3 `DialogClosed` 与值快照

- 对话框关闭时（自动或 `pop_dialog`），库先收集 `values`：对话框树中所有有本地名字的 `Edit`（`Text`）、`Checkbox`（`Bool`）、`Trackbar`、`Combo`、`List`（`Selected`，`Int`）的当前值；包括隐藏与禁用的节点，不包括其它对话框的节点，然后发出 `DialogClosed { result, values }`，最后移除对话框的节点。
- 宿主不再需要在关闭前读取文本框（v4 中“文字随关闭丢失”的问题到此解决）。
- 关闭后 `dialog_root` 与 `named` 中的 ID 失效（返回 `Stale`）。

### 6.4 输入与关闭的顺序

- `Popup` 的外部按下（指针按下，不在对话框矩形内）：库依次发出 `Dismissed { node: popup_root }`、`DialogClosed { result: Cancel }`，然后把该次按下交给下层处理（其事件在之后）。宿主可在处理 `DialogClosed` 时推送新的 `Popup`。
- `Modal` 与 `Page` 的外部按下：被消费，不穿透。
- 只有栈顶对话框接收键盘与指针输入。`Modal` 之下的层全部忽略输入。`Page` 替换栈底之上的页面，不影响 `Modal`。

### 6.5 `ModalResult` 与 `message_box`

```rust
pub enum ModalResult { Ok, Cancel, Yes, No, Custom(u16) }

pub fn message_box(text: &str, buttons: &[MsgButton], kind: MsgKind) -> DialogSpec;
pub struct MsgButton { pub label: String, pub result: ModalResult, pub default: bool, pub cancel: bool }
pub enum MsgKind { Info, Warning, Error, Question }
```

- `message_box` 返回的 `DialogSpec`：`kind = Modal`，`backdrop = Dim(160)`，`anchor = Center`，`auto_close = true`，`escape = Cancel`。
- 按钮的本地名字为 `"btn0"`、`"btn1"` …，按 `buttons` 的顺序。`default` 与 `cancel` 为真的按钮分别成为 `default_button` 与 `cancel_button`。
- 没有 `cancel` 的消息框：Escape 等同于关闭并返回 `Cancel`（`ModalResult::Cancel`），与按钮无关。
- `MsgKind` 决定图标与标题颜色（`Theme.text_warn`、`Theme.text_error`）。
- 文字为本地化键或字面量。种子名称等需要拼入的文字由宿主先拼成字面量。
- 长文字：`Wrap = true`，高度增长到 `MaxHeight`，再超出则出现 `ScrollArea`。

### 6.6 背景压暗

`Backdrop::Dim(alpha)` 在 `Modal` 之下绘制覆盖视口的半透明矩形。

## 7. 布局、绑定与数据流

### 7.1 数据流

```
宿主游戏状态 ──set / bind──► 节点树 ──layout──► 矩形 ──draw──► 渲染器
     ▲                           │
     └──── drain_events ◄────────┘
```

- 宿主每次模拟更新后调用 `bind` 或 `set`。库不轮询宿主状态。
- 造物网格的 `Items`、`ItemProgress`、`ItemReady` 由宿主设置。玩家不能通过界面改变它们。

### 7.2 定位规则

每个节点的位置由以下之一决定，优先级从高到低：

1. `Anchor`（`set_anchor`，或布局中的 `Hold`、`X`、`Y` 翻译而来）；
2. 父节点为 `Row` 或 `Column` 时，顺序布局：`Gap` 生效；`Row` 的子节点使用 `Width`（作为列宽），`Column` 的子节点使用 `Height`（作为行高），`X`、`Y` 被忽略；
3. 默认：`Space { Dlu, TopLeft, 0, 0 }`。

- `X`、`Y`、`Width`、`Height`、`Hold` 是 `Anchor` 的简写，代码与 INI 都可以使用。
- `Anchor` 的类型：

```rust
pub enum Anchor {
    Space { space: Space, hold: Hold, x: i32, y: i32 },
    Near { target: NodeId, side: Side, offset: i32 },     // 仅用于节点的内部定位（如 Tooltip）
    Stack,                                                // Row/Column 中的顺序
}
pub enum Space { Dlu, Logical, Screen }
```

- 兄弟节点使用同一种 `Space`。父子之间可以不同（`Dialog` 是 `Dlu`，其子节点同为 `Dlu`；`Field` 的子节点是 `Screen`）。
- 侧边栏改变边缘：`set(sidebar, Prop::Hold, ...)` 在运行时生效，库重新布局（§11.2）。
- 动画的位移用 `OffsetX`、`OffsetY`（§9.2），不改变 `Anchor`。

### 7.3 尺寸

```rust
pub enum Size { Px(i32), Rest, Auto }
```

- `Value::Int(n)` 等价于 `Size::Px(n)`。
- `Rest`：占满父节点剩余空间（INI 中只在文档允许的地方）。
- `Auto`：按文字或图片。
- `Auto` 不能用于 `List` 的列宽（只能 Px 或 Rest）。

### 7.4 绑定

```rust
pub enum BoundValue { Text(String), Int(i32), Time(GameTime), Credits(i64), Power(i32), Drain(i32), Count(u32) }
```

- `bind(name, value)` 更新所有绑定到 `name` 的节点（`Text.Bind`、`Bar.Bind`）。
- 时间格式化：`Time` 按 `Format`（默认 `m:ss`；超过一小时 `h:mm:ss`）。
- 计时器：`Timers` 是 `Column`；每个计时器是 `Row`，包含一个 `Text`（标签，`Label`）和一个 `Text`（数值，`Bind = "Timer.<n>"`，`Format`）。API 允许（§2.2）；INI 只显示数值（§13.1）。
- 布局中的 `Value=Credits` 等价于 `Text.Bind = "Credits"`，宿主负责 `bind("Credits", ...)`。

### 7.5 弹窗定位

- 指向列表行：`row_rect(list, row)` 得到矩形，`DialogAnchor::Beside { rect, side: Right, offset: 4 }`。
- 指向节点：`DialogAnchor::Near`。
- 居中：`DialogAnchor::Center`。

## 8. 输入与焦点

### 8.1 输入事件

```rust
pub enum InputEvent {
    PointerDown { pos: Vec2, button: MouseButton },
    PointerUp { pos: Vec2, button: MouseButton },
    PointerMove { pos: Vec2 },
    Wheel { lines: f32 },           // 宿主已换算（§8.8）；现有代码为 Wheel { lines: i32, x: i32, y: i32 }
    Key { key: Key, repeat: bool, mods: Mods },
    Text(String),
    Ime(ImeEvent),
}
pub enum ImeEvent { Preedit { text: String, cursor: Option<(usize, usize)> }, Commit(String), Enabled(bool) }
```

- `Key` 是库自己的枚举（与 RFC 键名一致）。
- IME 预编辑文字在 `Edit` 内以下划线绘制；`focus_rect()` 给出候选窗位置。

### 8.2 按钮的激活时机

- 默认 `ActivateOn::Release`：按下与释放落在同一节点上时发出 `Activated`。
- `ActivateOn::Press`：按下时发出（侧边栏按钮、造物网格、标签页）。

```rust
pub enum ActivateOn { Release, Press }
```

### 8.3 程序化与用户操作

- 用户操作（点击、键盘、拖动）发出事件。
- 程序通过 `set` 修改 `Selected`、`Checked`、`Pos`、`Fill`、`Items`、`Rows`、`Toggled`、`Text`（`Edit`）不发出事件。`TextChanged` 只由用户输入产生。

### 8.4 焦点范围

- 焦点存在于两种情况：栈顶对话框内（初始焦点为 `initial_focus`，否则为第一个可聚焦节点）；或宿主通过 `set_focus` 设置的 `Edit`（如聊天输入，即使没有对话框）。HUD 上的其它节点不能获得焦点。没有对话框且没有焦点在 `Edit` 上时，Tab 不被消费。
- Tab 顺序：栈顶对话框内的先序遍历，只包括可见、已启用且可聚焦的节点。`Shift+Tab` 反向。Tab 只改变焦点。
- 焦点节点被删除或不可见时，焦点移到下一个可聚焦节点（或无）。

### 8.5 键盘规则

- `Enter`：
  - 焦点在 `Button` 上：激活它；
  - 焦点在 `Edit` 上：发出 `Submitted`，**然后** 激活对话框的 `default_button`（若有）。两者都发生（v4 的“只发 Submitted”被改为两者，宿主可以在 `Submitted` 中处理，也可以只依赖按钮）；
  - 焦点在 `List` 上：发出 `ListDoubleClicked`；
  - 否则：激活对话框的 `default_button`（若有）。
- `Space`：焦点在 `Button` 上时激活它；焦点在 `Checkbox` 上时切换它（§5.8）。
- `Escape`：
  - 焦点在 `Edit` 上：先发出 `Cancelled`，然后按对话框的 `EscapePolicy` 处理（两者都发生）；
  - 焦点在 `Combo` 上且其下拉列表打开：关闭下拉列表，不继续处理；
  - 其余：`EscapePolicy`。`Cancel` 激活 `cancel_button`（若无则关闭并返回 `Cancel`）；`Ignore` 消费输入但不做事；`Dismiss` 发出 `Dismissed` 并关闭。
- 方向键：`Trackbar`、`ScrollBar` 左右减加；`Combo` 上下改变选择（`final_ = true`）；`List` 上下翻行（§5.3）。

### 8.6 `InputResult` 与层次

- `input()` 返回 `consumed`。
- 宿主规则（§14.2）：库消费了的事件不再传给游戏热键；未消费的事件进入游戏。
- 消费规则：
  - 鼠标：按下落在可见且可交互的节点、或 `Modal` 对话框上时消费；`Popup` 的外部按下不消费（§6.4）。`HitTest = 0` 的节点不接收指针。
  - 键盘：
  - 焦点在 `Edit` 上时，字符键与文字被消费；
  - 有 `Modal` 打开时，所有键与文字被消费（字母键不再传给游戏热键）；
  - 只有 `Page` 时，Tab、Enter、Space、Escape、方向键被消费，其它键（包括字母）交给游戏；
  - 没有对话框且焦点不在 `Edit` 上时，导航键不被消费。
- 布局编辑模式（§11.6）中，库消费所有指针事件与 Escape。

### 8.7 指针捕获、拖动与离开

- `PointerDown` 命中节点后，该节点捕获指针直到 `PointerUp`。
- `Movable` 节点（仅编辑模式）：按下后移动发出 `Dragged`，同时由库更新位置。
- `External` 节点的按下、移动、释放发出 `ExternalPointer`。`External` 不发出 `Dragged`。
- 鼠标离开节点发出 `PointerLeave`。
- 命中测试：取覆盖点的、可见、可交互且 `HitTest = 1` 的节点中绘制顺序最后的（后绘制优先）。

### 8.8 滚轮

- 宿主把像素增量换算为行（每 30 像素一行，限制在 ±3，`app/handler.rs` 滚轮处理，分析报告给出约 `:1916`）。
- 落在 `ScrollArea`、`ChatLog`、`List`、`CameoGrid` 上时由该节点处理；否则未消费。

### 8.9 `Enabled` 的作用范围

- `Enabled = 0` 作用于节点及其整个子树：子树中的交互节点不接收输入，显示为灰色。
- 对话框或面板被禁用时，其按钮不能激活，但 Escape 仍然按 `EscapePolicy` 工作（禁用只影响交互节点，不影响对话框自身的关闭）。

### 8.10 chrome 层（已实现）

宿主自己的常驻控件（对局的侧边栏按钮、造物格、命令栏、计时器停靠、布局添加的按钮、超武栏、战场与雷达的接收区）
放在 chrome 层：一棵 `Dialog` 根下的 `Button` 节点，位于全部对话框之下。规则是原版 gadget 链表的规则。

```rust
impl Gui {
    /// 把已有的 Dialog 节点压为 chrome 层（插在栈底）；同时只能有一个，已有时 Refused::Denied。
    /// 面积大于 hover_seed_area（像素²）的节点不参与悬停。
    pub fn push_chrome(&mut self, root: NodeId, hover_seed_area: i64) -> Result<DialogId, Refused>;
    pub fn chrome(&self) -> Option<DialogId>;
    /// chrome 层在 (x, y) 会悬停的节点：覆盖该点的最小的可用节点，不超过 hover_seed_area。
    pub fn chrome_hit(&self, x: i32, y: i32) -> Option<NodeId>;
}

pub enum InputEvent { /* … */ Wheel { lines: i32, x: i32, y: i32 }, /* … */ }
pub struct InputResult { pub consumed: bool, pub passthrough: Option<NodeId> }
pub enum UiEvent { /* … */ RightActivated { node: NodeId }, /* … */ }
```

只用于 chrome 层 `Button` 的属性：

| 属性 | 类型 | 含义 |
|---|---|---|
| `Buttons` | Int | 接收的鼠标键：位 1 左键，位 2 右键；未设时只收左键。不收的键穿过它交给下一个节点 |
| `PressActivates` | Bool | 按下即激活（左键 `Activated`，右键 `RightActivated`），不捕获，释放穿过 |
| `Passthrough` | Bool | 宿主自己处理的接收区：像按钮一样捕获，但不激活；它收下的输入以 `InputResult.passthrough = Some(node)`、`consumed = false` 交回宿主 |
| `ToggleOnRelease` | Int | 指针在矩形内释放并激活时：1 翻转 `Toggled`，2 置为开 |
| `Opaque` | Bool | 它不处理的按下或释放（不收的键、未捕获的释放）被它吞掉，下面的节点看不到 |

路由（只在没有对话框压在 chrome 层之上时）：

- 移动：悬停取最小的可用节点（同面积取后者）；捕获中的按钮按指针是否在矩形内显示按下。悬停在非 `Passthrough` 节点上时消费。
- 按下：交给树序中第一个收该键的可用节点；`PressActivates` 立即激活，否则捕获（非 `Passthrough` 的发出 `Pressed`）。
- 捕获期间，持有者收下所有按键消息（可用与否），直到它所收的键释放；最后一次移动把指针留在矩形内时，释放才激活它。
- 滚轮：交给 `(x, y)` 处的节点；`Passthrough` 节点交回宿主，其它节点消费。对话框中的滚轮仍交给悬停节点（§8.8）。
- 键与文字不经 chrome 层。

侧边栏与命令栏的按下顺序由宿主保留，以便在按钮图像重叠时选中正确的控件。见[对局输入路由](../src/app/input/chrome_input.rs)和[命中测试](../gui/src/chrome_tests.rs)。

## 9. 动画与时间

### 9.1 两个时钟

- `Clock::Ui`：墙上时间，用于悬停、闪烁、提示延迟、逐字显示、菜单滑动。
- `Clock::Game`：游戏时间（暂停时停止），用于计时器、电量条、造物进度、聊天过期（可选）。

`update(ui_now, game_now)` 每帧调用。

### 9.2 动画

```rust
pub enum Anim {
    Slide { axis: SlideAxis, from: i32, to: i32, duration_ms: u32, curve: Curve, clock: Clock },
    Flash { period_ms: u32, clock: Clock },                    // 闪烁：Bar 的 blink 段、非按钮节点
    Reveal { step_ms: u32, step_chars: u32, clock: Clock },    // 逐字显示
    Fill { to: i32, duration_ms: u32, clock: Clock },          // 仅用于未绑定的 Bar（§5.6）
}
pub enum SlideAxis { X, Y }
pub enum Curve { Linear, Wave(RetailWave) }
pub struct RetailWave { pub tick_ms: u32, pub tail_ticks: u32, pub ramp_steps: u32 }
```

- `Slide` 修改 `OffsetX` 或 `OffsetY`；`duration_ms` 决定时长，`Wave` 的节拍只决定曲线的采样（不额外延长时长）。
- `from`、`to` 的单位是逻辑画布像素（800 × 600，已乘以 `ShellScale`）。`from` 为负值时对话框在视口左侧或上方外。`slide_in(dialog, edge, ms)` 计算从视口边缘外到 0 的 `from`，是对话框滑入的标准写法（§2.1）。
- 动画阻塞的范围：
  - `Slide` 运行期间，该对话框忽略所有输入（指针、键盘、文字），直到动画结束；
  - `Reveal` 不阻塞输入：按钮与 `Edit` 始终可用；只有在正在显示的 `Text` 上按下才会跳过显示；
  - `Fill`（未绑定的 Bar）与 `Flash` 不阻塞输入。
- 动画结束发出 `AnimationFinished`。宿主用它在“滑出结束”后切换页面。
- 滑出后关闭：宿主监听 `AnimationFinished`，再调用 `pop_dialog`。对话框不会在滑出期间因默认按钮而提前关闭：动画期间按钮的激活被忽略（与输入忽略一致）。
- `Reveal` 作用于 `Text` 的内容；内容改变时重新开始。
- 逐字显示的跳过：指针在正在显示的 `Text` 上按下时，库立即完成显示（发出 `AnimationFinished`）。
- 节拍与步长沿用原版：菜单滑动 30 毫秒、6 个尾部节拍、6 个渐变步（`ui/shell/slide.rs` 约 `:29-34`）；标题逐字 `step_ms=30, step_chars=1`；状态行 `step_ms=15, step_chars=3`（`ui/shell/static_reveal.rs:13`）。
- `cancel_anim(id)` 取消，不发出完成事件。

### 9.3 悬停闪烁

`Button` 悬停时的闪烁由 `Theme.hover_flash_hz` 自动驱动，不能对单个按钮关闭（主题值全局生效）。`Anim::Flash` 不用于按钮。

## 10. 事件的读取

- 宿主每帧调用一次 `drain_events()`，按发生顺序取得 `Vec<UiEvent>`。
- 同一帧内事件可能指向已删除的节点。宿主用 `find`、`get` 或 `NodeId` 的代数判断是否仍有效。

## 11. 布局与编辑

### 11.1 布局文件的结构与键

```ini
[Layouts]
GDI=Mine
Nod=Mine
Default=Plain

[Mine]
Version=1
Parts=Field,Side,Bar

[Mine.Side]
Hold=Left
```

- `[Layouts]`：每侧一行（`SideId` 即侧名，如 `GDI`）；`Default=` 给没有列出的侧。
- 没有列出且没有 `Default=` 的侧：使用 `RetailLayout`（§11.8）。
- 没有 `[Layouts]` 段：所有侧使用 `RetailLayout`，不记录问题。
- 文件编码：UTF-8，允许 BOM。不是合法 UTF-8 时整个文件作废，记录一条 `LayoutProblem`，所有侧使用 `RetailLayout`。
- 布局段 `[名字]`：`Version`、`Parts`。
- 部件段 `[名字.部件]`：键与 `interface-layout.md` 相同。

INI 的 `Type` 允许的值（完整表）：

| `Type` | 节点种类 | 备注 |
|---|---|---|
| `Button`、`Text`、`Row`、`Column`、`CameoGrid` | 同名 | 文档允许的新增部件 |
| `Battlefield` | `Field` | 别名 |
| `PowerBar` | `Bar` | 别名 |
| `Panel` | `Panel` | 只能改数值，不能改种类 |
| `Radar` | `External`（小地图，§12.5） | 只能改数值 |
| `Tabs` | `Row`，内含四个 `Button`（`Command = Tab:<名>`，`ActivateOn = Press`） | 由库生成；标签顺序与 `interface-layout.md` 一致 |
| 其它 | — | `LayoutProblem`，跳过 |

INI 键与属性的对应：`Art` → `Picture`（Button、Image、Decoration）；`Rows` → 网格中为 `GridRows`，列表中不适用；`Columns` → `Columns`（网格的列数）；`Movable=yes|no` → `Movable`（默认：侧边栏的 `Credits`、`Top`、`Radar`、`Middle`、`Cameos`、`Power`，以及战场的 `Messages`、`Timers`、`Banner`，按 `interface-layout.md` 的编辑列表）。

### 11.2 `Hold` 的取值

九个位置（`TopLeft`、`Top`、`TopRight`、`Left`、`Middle`、`Right`、`BottomLeft`、`Bottom`、`BottomRight`）或四个边（侧边栏的 `Left`、`Right`、`Top`、`Bottom`）。INI 大小写不敏感。侧边栏的 `Left`、`Right` 边：子节点纵向堆叠（`Column`）；`Top`、`Bottom` 边：子节点横向排列（`Row`）。

### 11.3 尺寸的 INI 写法

- `Rest` 只用于文档允许的地方；其它地方是 `LayoutProblem`。

### 11.4 `Field`

- `Field` 是 `Screen` 空间的节点。尺寸是视口减去侧边栏与命令栏后的剩余空间。
- 子节点：`Messages`（`ChatLog`）、`Timers`（`Column`）、`Banner`（`Text`，`Space { Screen, Middle }`）、`Powers`（`Column` 的 `Button`）。
- 横幅居中以 `Field` 为准。

### 11.5 计时器

`Timers` 是 `Column`，`Hold=BottomRight`。INI 版本只有数值，API 版本每个计时器是 `Row { Text(Label), Text(Bind="Timer.<n>") }`。

### 11.6 玩家布局编辑

库负责整个编辑界面：

- `begin_edit(side)`：进入编辑模式。只有 `Movable` 节点参与；库绘制轮廓、名字标签，以及 **保存**、**重置** 两个按钮（库内置，不是宿主节点）。
- 拖动：库更新 `X`、`Y`（相对节点的 `Anchor` 空间：`Dlu`、`Logical` 或 `Screen`，与节点的空间一致）。`Hold` 不能用拖动改变。
- 保存：库计算 `OverrideChange` 列表，退出编辑模式，发出 `LayoutEdited { side, changes }`。宿主把 `changes` 写入玩家的 `veraui.ini`。
- 重置：库把该侧恢复为 **模组布局**（`load_layout` 记录的 `mod_file` 副本，不含玩家覆盖层），发出 `LayoutReset { side, changes }`；仍处于编辑模式。之后的保存只比较当前拖动相对于模组布局的差异。
- Esc：库退出编辑模式、还原所有拖动，发出 `LayoutCancelled { side }`。
- `end_edit(side, save)`：宿主结束编辑（如对局开始）。`save = true` 等同于保存（发出 `LayoutEdited`），`save = false` 等同于 Esc（发出 `LayoutCancelled`）。
- 库不做 I/O。`OverrideChange { section: String, key: String, value: Option<String> }`；`section` 为完整段名，如 `Mine.Side`；`value = None` 表示删除。

### 11.7 布局加载器与合并

```rust
pub fn parse_layout(bytes: &[u8], ctx: &LayoutContext) -> LayoutOutcome;
pub fn merge_layout(base: &LayoutFile, overlay: &LayoutFile) -> LayoutFile;   // 按键合并，overlay 优先
pub fn build_layout(layout: &NamedLayout, retail: &RetailLayout) -> NodeSpec;

pub struct LayoutContext<'a> {
    pub known_commands: &'a dyn Fn(&str) -> bool,
    pub tab_items: &'a dyn Fn(&str) -> Vec<Item>,
    pub supported_version: i32,               // 库当前为 1
}

pub struct LayoutOutcome {
    pub file: LayoutFile,                      // 总是返回：可用的部分照常加载
    pub problems: Vec<LayoutProblem>,
}
pub struct LayoutFile { pub sides: BTreeMap<SideId, NamedLayout>, pub default: Option<NamedLayout> }
pub struct NamedLayout { pub name: String, pub parts: Vec<NodeSpec>, pub version_ok: bool }
pub struct LayoutProblem { pub line: u32, pub section: String, pub key: String, pub reason: String }
```

- 玩家覆盖层的使用：宿主先 `parse_layout` 模组文件，再 `parse_layout` 玩家文件，用 `merge_layout(base, overlay)` 合并。覆盖层的键胜出；覆盖层中有、模组中没有的节，视为“不在该布局中”，不被使用（§11.6 中“其它模组下不生效”）。
- 解析总是返回 `LayoutOutcome`。
- `version_ok = false`：该侧使用 `RetailLayout`，并记录问题。
- `Command=` 不被 `known_commands` 认可：该部件不显示，并记录问题。
- `Tab=` 经 `tab_items` 展开为该标签页的可造物件。
- `Label` 与 `Value` 同时出现：记录问题，使用 `Value`。
- 嵌套的行或列：记录问题，嵌套部分被忽略。
- 未知键、未知部件、不允许的 `Type`：记录问题，跳过。

### 11.8 原始布局

- `RetailLayout` 是库的内置数值（`retail_layout()`），由游戏提供原始的数值表（`ui/match_layout.rs` 的原始数值）。库不内置资源。

## 12. 渲染与后端

### 12.1 渲染器

```rust
pub struct Renderer { /* wgpu 管线、字形图集、纹理表。由宿主持有 */ }

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Renderer;   // 格式在创建时固定
    pub fn set_smooth(&mut self, on: bool);
    pub fn register_texture(&mut self, w: u32, h: u32, rgba: &[u8]) -> TextureId;
    pub fn update_texture(&mut self, id: TextureId, rgba: &[u8]);
    pub fn register_external(&mut self, view: &wgpu::TextureView, size: (u32, u32)) -> ExternalId;
    pub fn update_external(&mut self, id: ExternalId, view: &wgpu::TextureView, size: (u32, u32));
}

pub struct FrameTarget<'a> {
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub view: &'a wgpu::TextureView,         // 呈现前的 sRGB 目标
    pub size: (u32, u32),
    pub pre_pass: &'a mut dyn FnMut(&mut wgpu::CommandEncoder),
}
```

- `Renderer` 在 `new` 时保存 `Device` 与 `Queue` 的克隆（wgpu 句柄是引用计数的）。宿主可以在之后丢弃自己的引用。
- 目标格式固定：要改格式需要新建 `Renderer`。
- `Gui::draw(&mut frame, &mut renderer)`：先调用 `pre_pass`，再绘制界面，最后（若开启）经过滤镜。

### 12.2 图片与预览

```rust
pub enum Picture { Named(String), Texture(TextureId) }
pub trait Assets {
    fn resolve_image(&self, name: &str) -> Option<ImageRef>;
    fn image_pixels(&self, image: ImageRef) -> Option<RgbaImage>;
}
```

- `Named` 在 `draw` 时首次使用时上传。宿主可以在进入界面前调用 `preload(names, renderer)` 提前上传，避免第一帧卡顿（§2.1）。上传后缓存。
- `Texture` 由宿主注册（如运行时生成的地图预览），宿主调用 `update_texture` 改变内容。
- `pub enum Fit { Native, Contain, Cover, Stretch }`。
- `Fit`：`Native` 原始大小不缩放；`Contain` 等比放入；`Cover` 等比铺满并裁剪；`Stretch` 拉伸。预览图使用 `Contain`。

### 12.3 滤镜

- `set_smooth(true)`：界面通道的输出经过平滑滤镜（原版的 `SmoothOnlineMenus` 效果）。
- 顺序：`pre_pass`（宿主工作）→ 界面绘制 → 滤镜（只作用于界面层；库把界面层渲染到内部中间纹理，滤镜后按 alpha 合成到 `pre_pass` 的内容之上）→ 宿主量化。
- 内部中间纹理使用与 `Renderer::new` 的 `format` 相同的格式；库不另选格式。

### 12.4 文字与字体

```rust
pub enum FontRole { Body, Title, Small, Bitmap }
```

- 字体：内置字体（21 个文件）、GAME.FNT 位图字体（`FontRole::Bitmap`，依赖 `assets::fnt_file`，约 405 行，自包含）、parley 0.12 + swash 0.2.10（其它 `FontRole`）。
- `is_cjk_sized`（`i18n/mod.rs:121`）属于文字测量，迁入库。
- `CustomCanvas` 不能画文字；需要文字时宿主先把它渲染成纹理，再用 `Picture::Texture` 显示。

### 12.5 外部纹理（小地图）

- 雷达是 `External` 节点，`Texture` 指向 `ExternalId`，`Mapping` 为图像在节点内的位置与大小（INI 的 `MapX`、`MapY`、`MapWidth`、`MapHeight`）。
- `ExternalPointer.local` 是 **纹理像素坐标**（范围 0..width、0..height，与 `register_external` 的 `size` 相同）。库把节点内的指针位置经 `Mapping` 与 `Fit` 逆变换后给出，与节点左上角无关。
- 宿主每帧调用 `update_external`；库不缓存外部纹理。
- 视口框（白色矩形）：`Custom` 节点，`HitTest = 0`，因此不挡住地图点击（v4 的问题）。
- 小地图的大小与位置：运行时改变 `Width`、`Height`、`Mapping`；库重新布局。
- 原始侧边栏的“雷达”部件（`Radar`）在布局中成为 `External` 节点（§13.1）。

### 12.6 `Custom` 节点

```rust
pub type CustomDraw = Box<dyn FnMut(&mut CustomCanvas, Rect, Option<Rect>)>;  // 节点矩形，宿主每帧数据（Prop::Rect）
pub struct CustomCanvas { /* fill(rect, rgba)、outline(rect, rgba, width)、picture(Picture, rect)；无文字，无 GPU */ }
```

- 回调在界面通道中执行，只能使用 `CustomCanvas` 的方法。
- 第三个参数是宿主每帧通过 `set(id, Prop::Rect, Value::Rect(r))` 设置的值（未设置则为 `None`），用于视口框等每帧数据。
- `set_custom(id, draw)` 注册回调（§2.1）。

### 12.7 光标

库不绘制光标。`InputResult.cursor` 返回宿主应显示的形状；游戏的 SHP 光标由宿主绘制。

## 13. 与 INI 的一致性

### 13.1 统一规则与已知差异

| 规则 | 决定 |
|---|---|
| `Type` 别名 | `PowerBar` 为 `Bar`，`Battlefield` 为 `Field`（§11.1 的完整表） |
| `Text` 的 `Label` 与 `Value` | 同时出现：`LayoutProblem`，使用 `Value` |
| `CellX`、`CellY` | 第一个图标的位置（原始键，保留） |
| `CellWidth`、`CellHeight` | 布局读取器已支持（`Cameos`）；本草案的 API 使用 `CellSize` |
| `ActivateOn=Press` | INI 中尚未支持（§16） |
| 聊天输入行 | 独立的 `Edit`，不属于 `ChatLog` |
| 行与列嵌套 | API 允许；INI 不允许（`LayoutProblem`） |
| 计时器标签 | API：`Row { Label, Bind }`；INI 只有数值（已知差异） |
| 雷达 | INI 的 `Radar` 部件对应 `External`；`MapX` 等对应 `Mapping` |
| 侧边栏边缘 | INI 的 `Hold` 在布局加载时决定；运行时改变用 `set(Prop::Hold)`（§7.2） |

### 13.2 雷达的运行时形状

- 小地图显示或隐藏：`Visible`。
- 大小切换：宿主设置 `Width`、`Height` 与 `Mapping`，库重新布局。

### 13.3 地图点击

- 地图点击（战场上的左键或右键）不经过界面库的 `External` 节点，`External` 只覆盖小地图。
- 宿主在 `input()` 未消费事件时，自行判断是否点击在战场上，并处理地图点击。RFC 的 `CaptureMapClick` 语义由宿主 WASM 桥实现。

## 14. 宿主适配器

库不依赖 winit。宿主适配器负责：

- 键码翻译（平台 → `Key`）；
- 滚轮换算（像素 → 行，§8.8）；
- 光标位置与修饰键；
- IME 的预编辑与提交（区域来自 `focus_rect()`）；
- 重绘门、表面尺寸变化；
- 分层分发（§14.2）。

### 14.1 游戏侧留下的部分

`ModalPage`、`ScreenPage`、`GameScreen` 枚举；计划模式与热键；网络等待页；模拟线程的事件保持；RGB565 呈现；SHP 光标；CSF 文本；原始布局数值表（`RetailLayout`）；地图点击（§13.3）。

### 14.2 输入的层次

1. 若 IME 处于组合中，宿主先把事件交给 IME。
2. 调用 `Gui::input(event, now)`；`consumed` 为真则结束。
3. `passthrough` 为 `Some(node)` 时，交给该接收区的宿主处理（对局中是战场或雷达）。
4. 否则交给游戏（热键、命令、地图点击）。

## 15. RFC 的 ABI 翻译

### 15.1 调用对应

| RFC 接口 | 库调用 |
|---|---|
| `client_node_find(name)` | `find` |
| `client_node_create(kind, parent)` | `create(kind, parent, owner)` |
| `client_node_remove(id)` | `remove` |
| `client_set`、`client_set_bytes` | `set`（字节值转为 `Value::Bytes` 或 `Value::Text`） |
| `client_get` | `get` |

### 15.2 事件对应

`Click`、`Hover`、`Dismiss` 对应 `Activated`、`Hovered`、`Dismissed`（§3.2）。

### 15.3 所有权

- `Owner` 由宿主设置。
- RFC 的权限规则由宿主 WASM 桥检查；库只拒绝不存在的节点、类型不符与被锁定的写入（`Refused`）。
- `MapUI=overlay`：地图模块的新顶层节点用 `Popup` 或单独的覆盖根节点实现。

## 16. 开放问题

1. `ActivateOn=Press` 的 INI 键：是否新增（建议新增）。
2. 模组主题（颜色、字体覆盖）：建议 v6 再决定。
3. 菜单页的布局是否走 INI：建议先保持代码定义。
4. 世界空间动画（`Decoration`）：建议交给游戏侧的 `Custom`。
5. RFC 的 `Tick` 事件使用哪个时钟：建议两者都提供，由 `Clock` 决定。
6. `Radio` 控件：保留描述符，不做实现。
7. `Key` 与 RFC 键名的完全一致性：需要逐一核对。
8. 电量条的平滑（`Smooth`，线性）与原版 CdTimer 步进的差异：需要在原型中与原版对比，必要时增加步进模式。
9. 选项页滚动步长（默认 30 像素）是否允许模组配置：建议 v7 再决定。

## 17. 迁移步骤

与 v2 §14 相同，并在第 0 步增加：先写 `UiEvent`、`Value`、`Prop` 的测试夹具，验证每个界面的 `NodeSpec` 可以构造。

## 18. 相对 v4 的变更

| v4 盲评问题 | v5 决定 |
|---|---|
| 按钮关闭规则只对默认与取消按钮有定义；`MsgButton.result` 没有接入 | 按钮可带 `Result`；带 `Result` 的按钮激活时自动关闭（`AutoClose`），`Result` 进入 `DialogClosed`（§6.2） |
| 没有校验后再关闭 | `auto_close = false`，宿主校验后 `pop_dialog`（§6.2） |
| 对话框关闭后文本丢失 | `DialogClosed` 附带命名控件的值快照（§6.3） |
| Escape 与 Enter 在文本框中的冲突 | Edit 的 Escape 先发 `Cancelled` 再按 `EscapePolicy`；Enter 先发 `Submitted` 再激活默认按钮（§8.5） |
| `Fixed` 没有 side 与 offset | `Beside { rect, side, offset }` 与 `Near`（§6.1、§7.5） |
| Popup 穿透与事件顺序 | 先 `Dismissed`、`DialogClosed`，再交给下层（§6.4） |
| Enabled 作用范围 | 作用于子树，不影响对话框自身的关闭（§8.9） |
| 按钮没有按下态 | `Toggled` 属性（§2.3、§5.5） |
| 侧边栏重排与边缘切换 | `Hold` 运行时可改，库重新布局（§7.2） |
| 电量条分段闪烁 | `Segment.blink` 与 `Anim::Flash`（§4.3、§9.2） |
| 滚动按钮的执行者 | `Command` + `Target`，由库执行，不发出 `Activated`（§5.5） |
| 雷达白框挡住点击 | `HitTest` 属性（§2.3、§12.5） |
| `set_custom` 缺失 | 加入 §2.1 |
| `Clock` 未定义；`sticky` 含义；`Count` 重名 | `Clock` 枚举（§2.1）；`sticky` 与 `expires` 的规则（§5.4）；掉线计数改为 `Dropped`（§2.3） |
| 布局文件的 `Art`、`Rows`、`Movable`、合并 | 键对应表与 `merge_layout`（§11.1、§11.7） |
| 没有 `Art`、`Type` 完整表 | §11.1 的完整表 |
| HUD 上的焦点范围 | 焦点只在对话框内或 `Edit`（§8.4） |
| 下拉框在滚动容器中的裁剪 | 不受裁剪；滚动时关闭（§5.1） |
| 行内子节点宽度 | `Row` 子节点用 `Width`，`Column` 子节点用 `Height`（§7.2） |
| `Trackbar` 数值显示与连续发出 | `ShowValue`；每帧最多一次（§5.7） |
| 聊天贴底规则矛盾 | 明确的跟随规则（§5.4） |
| `List` 键盘与滚动条 | 键盘规则与 `ScrollBar` 默认显示（§5.3） |
| 双击间隔 | 主题的 `double_click_ms`（§5.3） |
| `message_box` 的 `cancel` 类型与 Escape | `MsgButton.cancel: bool`；无取消按钮时 Escape 关闭（§6.5） |
| 逐字显示的跳过 | 按下完成显示（§9.2） |
| 布局的 `reset_layout` 与重置按钮重叠 | 删除 `reset_layout`；只保留库内的重置按钮（§2.1、§11.6） |
| `Dragged` 既消费又发出 | 库已处理，宿主可忽略（§3.1） |
| 地图点击的路径 | `External` 只覆盖小地图；地图点击在宿主（§13.3） |
| 缺少 `Target` 与 `Result` 属性 | 加入 §2.3 |
| 滑动与时长 | `duration_ms` 决定时长，`Wave` 只决定采样（§9.2） |
| Value 缺少 Size、Fit、ActivateOn、FontRole、Align | 加入 `Value` 变体（§2.3）；`Width` 接受 `Value::Int`（§7.3） |
| `History`、`FadeMs`、`MaxRows` 不在属性表 | 加入属性表（§2.3） |
| `Theme` 缺 `double_click_ms`；对话框没有标题 | `Theme.double_click_ms`（§4.1）；`Title` 属性与 `DialogSpec.title`（§2.3、§6.1） |
| `Count` 与三个 `Segments` 重复 | 删除 `Count`；段数是 `Segments` 的长度（§4.3、§5.6） |
| 电量条绑定与 `Anim::Fill` 冲突 | `Smooth` 属性：库内部平滑；绑定的 Bar 拒绝 `Anim::Fill`（§5.6） |
| 滑动单位与起点未定义 | 逻辑画布像素；`slide_in` 从视口边缘滑入（§9.2） |
| 动画阻塞输入的范围 | `Slide` 阻塞整个对话框；`Reveal` 不阻塞（§9.2） |
| 模态框打开时字母键穿透 | `Modal` 消费所有键与文字；`Page` 只消费导航键（§8.6） |
| §8.4 与 §8.6 的 HUD 焦点矛盾 | 焦点只在栈顶对话框内，或由 `set_focus` 设置的 `Edit`（§8.4） |
| `DialogClosed` 值快照范围 | 对话框树内全部命名节点，含隐藏与禁用，含 `List.Selected`（§6.3） |
| 忙碌状态与 `EscapePolicy` 推送后不可改 | 忙碌框模式；推送时固定，改变需重新推送（§6.2） |
| `Gui` 线程 | 只在 UI 线程使用；模拟线程经宿主通道（§2.1） |
| `Edit.Text` 由代码设置是否发 `TextChanged` | 不发出；只有用户输入发出（§8.3） |
| 光标处提示框位置 | 指针右下 12×16，翻转与夹紧（§4.4） |
| 布局缺结束编辑接口与基线 | `end_edit(side, save)`；`load_layout` 记录模组基线（§2.1、§11.6） |
| `Tabs`、`Radar`、`Panel` 映射 | `Panel`→`Panel`；`Radar`→`External`；`Tabs`→`Row` 加四个按钮（§11.1） |
| 缺少 `[Layouts]` 时的行为；编码 | 全部 `RetailLayout`，无问题；UTF-8 与 BOM（§11.1） |
| 侧边栏上下边缘的排列 | 左右纵向，上下横向（§11.2） |
| 图片上传时机 | `preload(names, renderer)`；默认在首次绘制时上传（§12.2） |
| 中间纹理格式 | 与呈现格式相同（§12.3） |
| 雷达视口框的每帧数据 | `Prop::Rect` 传给 `CustomDraw`（§12.6） |
| `ExternalPointer.local` 坐标空间 | 纹理像素坐标（§12.5） |
| `OverrideChange.section` 格式 | 完整段名 `Mine.Side`（§11.7） |
| `ActivateOn`、`FontRole`、`Fit` 的定义位置 | 分别定义于 §8.2、§12.4、§12.2 |

## 19. 不在本文档中的内容

游戏规则、模拟、造物顺序、电量与生产逻辑、雷达迷雾与 ping 历史、联机协议、启动流程、资源格式解码、随机地图生成（rmg）、种子与地图的保存格式、WASM 运行时本身。
