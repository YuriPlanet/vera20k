# vera20k-gui：保留式界面库（现状）

本文描述 `gui/`（crate `vera20k-gui`）当前实际提供的 API 与行为。设计草案（v6）见 `docs/gui-library-api.md`；本文记录其已实现部分与实际差异，草案中尚未实现的部分列在 §9。

库只负责节点树、输入路由、对话框栈和控件状态。**绘制全部由宿主完成**：宿主读取树（属性、`list_view`、`combo_drop_down`、`caret_visible` 等）并用自己的渲染器画出来。库里没有 `Renderer`、`Theme`、资源或文字测量。

控件行为移植自 gamemd 的外壳 owner-draw 控件，各模块在代码旁注明原生函数地址。

## 1. 模块

| 模块 | 内容 |
|---|---|
| `gui` | `Gui`、输入与事件类型、对话框栈 |
| `node` | `Tree`、`NodeId`、`NodeKind`、`Prop`、`Value`、`Refused` |
| `spec` | `NodeSpec` 与 `create_tree`、`find_id`、`id_of` |
| `shell` | 公开的零售外壳几何与行为辅助：`geom`（DLU→像素、`ShellScale`、`RectPx`=`Rect`）、`layout`、`descriptor`（对话框 id）、`list`（列表与滚动条几何）、`combobox`（下拉窗）、`new_edit`（NewEdit 文本、光标与横向滚动）、`trackbar`、`button`、`paging`、`static_reveal`、`warning_monitor` |
| `time` | 宿主时钟（原生 `std::time`，浏览器 `web-time`） |
| `widget`（私有） | 各种控件对按下、拖动、释放、滚轮、按键和计时器的反应 |

## 2. 树与节点

- `Gui::new(max_nodes)`：节点数达到上限时 `create` 返回 `Refused::Limit`。
- `create(kind, parent)`、`set(id, prop, value)`、`get(id, prop)`、`remove(id)`（连同子树）。`NodeId` 带代数，删除后的旧 id 返回 `Refused::Stale`；属性值类型或节点种类不符返回 `Refused::WrongKind`。
- `NodeSpec` 声明式地描述子树：`NodeSpec::new(kind).id(..).label(..).tooltip(..).with(prop, value).child(..)`；`create_tree(&spec, parent)` 全部建成或什么都不留，返回根的 `NodeId`。
- 控件用宿主的整数 id（`Prop::Id`，通常是对话框资源 id）寻址：`find_id(root, id)` 按树序找第一个，`id_of(node)` 反查。没有本地名字或句柄类型。
- `Rect` 是屏幕像素（不相对父节点）。`Visible` 与 `Enabled` 沿祖先继承。
- `Label`、`Text`、`Tooltip` 是 CSF 键或字面文字，由宿主解析与绘制。

### 2.1 `NodeKind`

`Panel`、`Dialog`（容器）；`Text`；`Button`、`Checkbox`、`Combo`、`List`、`Trackbar`、`Edit`、`ScrollBar`（可交互）。`ScrollBar` 是独立的竖直滚动条，翻动宿主自己排布的行。

### 2.2 `Prop`

| 属性 | 用于 | 含义 |
|---|---|---|
| `Id` | 全部 | 宿主 id |
| `Visible`、`Enabled` | 全部 | 继承；禁用节点仍可悬停（状态行帮助），不接受按下 |
| `Rect`、`HitRect` | 全部 | 位置；指针命中区域（缺省为 `Rect`） |
| `Text`、`Label`、`Tooltip`、`Color`、`DisabledColor`、`Align`、`TextInset` | 显示 | 供宿主绘制 |
| `Toggled` | Button | 点亮外观（当前选中的按钮），由宿主设置 |
| `Default`、`Cancel` | Button | Enter / Escape 激活它（IDOK / IDCANCEL）；可以隐藏，代表没有按钮承载的命令 |
| `Checked` | Checkbox | 勾选 |
| `Pos`、`Min`、`Max`、`Step`、`ShowValue`、`ScaleScreen` | Trackbar（`Pos`、`Max` 也用于 ScrollBar） | 值与范围 |
| `Items`、`RowCount`、`Selected`、`Top`、`VisibleRows`、`RowHeight` | List、Combo、ScrollBar | 行、选择、首行 |
| `NoSelectionFill`、`MultiSelect`、`SelectedRows`、`WheelStep` | List | 聊天记录、多选、每格滚轮行数（缺省 3） |
| `Open`、`OpenAnywhere`、`PopupBottom`、`PopupWiden`、`PopupFlipBelow`、`PopupRect`、`PopupHidden`、`Swatches` | Combo | 下拉窗 |
| `MaxLen`、`MaxBytes`、`ByteOnly`、`Rejected`、`Allowed`、`Password`、`SelectOnFocus`、`AllSelected`、`Caret` | Edit | 输入过滤与光标；`Top` 为首个可见单元 |
| `KeepsBlink` | Edit | Tab 给它焦点时不重启光标闪烁 |
| `TabStop` | 可交互节点 | Tab 停靠点（`Edit` 缺省是，其它种类需设置） |
| `PressThrough` | 全部 | 悬停，但按下与释放落到下面的节点 |

## 3. 对话框栈

- `push_dialog(root, DialogKind::Page | Modal)`：`root` 必须是已有的 `Dialog` 节点。只有栈顶对话框接收输入；`Modal` 消费所有未命中的输入。
- `pop_dialog`（同时删除节点）、`raise_dialog`、`top_dialog`、`dialog_root`、`dialogs`。
- 每个对话框保存自己的指针状态：捕获、悬停、焦点、打开的下拉窗。被模态框盖住的页面保留自己的按下外观与焦点。`reset_pointer`、`drop_pointer_in`、`cancel_pointer` 丢弃捕获与悬停。
- `set_dialog_scale(id, ShellScale)`：把设备坐标映射回 800×600 的原生控件算术。

## 4. 输入

`input(InputEvent, now: UiMs) -> InputResult { consumed }`。时间由宿主提供（毫秒）；`poll(now)` 运行计时器（滚动箭头连发），返回下次截止时间。

`InputEvent`：`PointerDown`、`PointerUp`（`MouseButton::Left`/`Right`）、`PointerMove`、`Wheel { lines }`（宿主换算好的格数，正为向上）、`Key(Key)`、`Text(String)`（含 IME 提交）。

`Key`：`Enter`、`Escape`、`Tab`、`BackTab`（Shift+Tab）、方向键、`Home`、`End`、`PageUp`、`PageDown`、`Backspace`、`Delete`。

### 4.1 指针

- 命中：可见、命中区包含该点、种类可交互（或带 `Tooltip` 的 `Text`）的节点中，**树序第一个**（外壳控件不重叠）。按下时跳过 `PressThrough`。
- 按钮：按下捕获并发 `Pressed`；仍在其上时显示按下外观（`shows_pressed`）；在同一按钮上释放且启用时发 `Activated`。按住时被禁用则失去捕获。
- 复选框：按下即切换（只在左侧 18×18 图标上，设了 `HitRect` 则整个区域），发 `Changed { Bool }`。
- 滑杆与 `ScrollBar`：按滑块拖动、按轨道跳到位置；过程中 `Changed { final_: false }`，释放时 `final_: true`。
- 列表：选中行发 `ListSelected`，双击（`set_double_click` 设定时间与范围）发 `ListDoubleClicked`，滚动发 `Scrolled`；滚动条箭头按住连发。
- 组合框：按箭头区打开下拉窗（`ComboOpened`），下拉窗打开时先接收该对话框的全部指针输入；选中行发 `Changed { Int }` 并关闭（`ComboClosed`）。
- 编辑框：按下取得焦点并重启光标闪烁；不按位置放置光标。
- 悬停变化发 `Hovered { entered }`。

### 4.2 滚轮

落在悬停的 `List` 上时滚动 `WheelStep` 行；否则未消费（模态框除外）。宿主附加入口：`wheel_list(node, lines)`（不论指针位置）、`wheel_popup(rows, x, y)`（打开的下拉窗）。

### 4.3 键盘

- 焦点只落在 `Edit` 上。编辑键与文字给焦点编辑框，按 `MaxLen`、`MaxBytes`（宿主用 `set_byte_len` 给出代码页长度）、`ByteOnly`、`Rejected`、`Allowed` 过滤；文字变化发 `TextChanged`。
- `Enter`：焦点编辑框先发 `Submitted`，然后激活对话框树序第一个 `Default` 按钮（须启用，可隐藏）。
- `Escape`：下拉窗打开时只关闭它（无事件）；否则焦点编辑框先发 `Cancelled`，然后激活 `Cancel` 按钮。
- `Tab` / `BackTab`：在栈顶对话框内可见、启用的停靠点之间按树序前进 / 后退并循环（GetNextDlgTabItem）；没有焦点时两者都去第一个停靠点。停到非 `Edit` 时焦点离开编辑框。发 `FocusMoved { from, to }`。
- 下拉窗的键盘（宿主选用）：`key_popup(key)`：上下移动热行，Enter 选中，Escape 关闭。
- 消费：编辑框接受的键、激活了按钮、移动了焦点，或栈顶为 `Modal`。

### 4.4 焦点与光标

`focus`、`focus_in(dialog)`、`set_focus`（不重启闪烁）、`focus_edit` / `focus_edit_in`（焦点改变时重启 NewEdit 的两秒闪烁）、`caret_visible(node, now)`、`caret_visible_in`。

编辑框的横向滚动由宿主测量：宿主用 `shell::new_edit::NewEdit::scroll_to_caret` 和自己的字体宽度算出首个可见单元，写入 `Top`（引擎侧为 `ui::shell::page::scroll_edit`）。

## 5. 事件

`drain_events()` 按发生顺序返回 `UiEvent`：`Activated`、`Hovered`、`Pressed`、`Changed { value, final_ }`、`ListSelected`、`ListDoubleClicked`、`Scrolled { top }`、`ComboOpened`、`ComboClosed`、`TextChanged`、`Submitted`、`Cancelled`、`FocusMoved`。宿主自己调用 `set` 不产生事件。音效由宿主根据事件播放。

## 6. 读取与宿主辅助

- 状态：`pressed`、`hovered`、`pressed_in`、`hovered_in`、`dragging`、`shows_pressed`、`popup`。
- 列表：`list_view`（`ListView { rows, top, selected, pressed }`）、`list_geometry`、`list_row_at`、`selected_rows` / `set_selected_rows`、`log_rows(node, rows, dropped)`（聊天记录：在末尾时跟随新行，否则保持显示同样的行）。
- 下拉窗：`combo_drop_down(node)`（`ComboDropDown` 几何、热行、按下的滚动条部件）、`open_popup(node, top)`、`close_popup`（宿主操作，无事件）。
- 命中：`hit_at(root, x, y, press)`，供宿主在下拉窗下方做悬停与状态行判断。

## 7. 引擎侧接入（vera20k）

- `ui::shell::controller::DialogController` 把引擎的对话框 id 映射到 `Gui` 的对话框栈，提供 `push`、`push_modal`、`push_overlay`、`reset_to`、`ensure_active`、`on_input`、`poll`、按控件 id 读写属性与焦点。
- 唯一的键盘适配器：`controller::gui_key(KeyCode, shift)` 与 `gui_input(code, shift, text)`。
- `ui::shell::page`：`ensure_built`、`sync_rects`（无矩形的控件隐藏）、`page_events`（把 `UiEvent` 转成按控件 id 的 `PageEvent` 与音效）、`scroll_edit`。
- 消息框（`ui::shell::modal::ShellBox`）带一个隐藏的 IDOK 按钮（`Default`+`Cancel`），所以 Enter 与 Escape 都得到回答 1，与 MessageBoxProc `0x005D36A0` 一致。

## 8. 约定

- 输入与时间都由宿主喂入，测试与回放可控。
- 行为改动以原生代码为准，在代码旁注明地址。玩家可见的行为（像素、交互、各屏幕的按键效果）在迁移中保持不变。

## 9. 尚未实现 / 未来

早先草案中的以下内容没有实现，需要时随具体屏幕加入：

- 主题、渲染器、资源与文字测量，`Image`、`Bar`、`ChatLog`、`ScrollArea`、`CameoGrid`、`Custom` 等节点种类，动画与游戏时钟，悬停闪烁；
- 布局引擎（锚点、`Row`/`Column`、自动尺寸）、`veraui.ini` 布局文件与玩家布局编辑；
- `DialogSpec`、本地名字、`initial_focus`、`ModalResult`、`message_box`、`EscapePolicy`、`Dismissed`；
- 按钮与列表的键盘焦点、`Space`、滑杆 / 列表 / 组合框的方向键（组合框见 `key_popup`）；
- 对局内界面（侧边栏、小地图、提示框）与 WASM RFC 的 ABI 翻译。
