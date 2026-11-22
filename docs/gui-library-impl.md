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
| `shell` | 公开的控件几何与行为辅助：`geom`（`RectPx`=`Rect`、`ShellScale`、`ScaledRects` 与 `scaled_rects!`、`fit_window`、`layout_on`）、`list`（列表与滚动条几何）、`combobox`（下拉窗）、`new_edit`（NewEdit 文本、光标与横向滚动）、`trackbar`、`button`（按钮帧与闪烁） |
| `time` | 宿主时钟（原生 `std::time`，浏览器 `web-time`） |
| `widget`（私有） | 各种控件对按下、拖动、释放、滚轮、按键和计时器的反应 |

`ShellScale` 不认识任何游戏尺寸：它把宿主给的基准尺寸（`ShellScale::unscaled(w, h)`、`ShellScale::fit(base_w, base_h, screen_w, screen_h)`、`in_window`）等比缩放进窗口；`is_unscaled` 时一切原样。`list`、`combobox`、`trackbar` 里的像素常量是从 gamemd 原生控件测得的**缺省度量**，宿主可按自己的文字另设（`RowHeight`、`ListMetrics`）。

尤里的复仇前端自己的布局知识不在库里，而在引擎的 `src/ui/shell/`：`geom`（800×600 基准 `SHELL_BASE_W/H`、`RETAIL_SCALE`、`shell_scale`、只有 800×600 与 640×480 保持原版布局的 `shell_scale_for_window`（`AllowModeToggle`）、`fit_window`、DLU→像素、右侧面板与 SDBTNANM 按钮列）、`checkbox`（`CheckboxFace`：复选框的 `cue_i`/`cce_i*.pcx` 外观与 `0x4E5` 双态规则）、`descriptor`（对话框资源 id `DialogResourceId` 与锚定规则）、`layout`（布局过程与状态行）、`paging`、`static_reveal`、`warning_monitor`。

库的测试只用 `gui/` 内的东西。用 `tools/storage_oracle/` 原生执行结果的测试（对话框重排、静态计时器，以及按钮帧、列表滑块高度、滑杆的原生对照）在引擎的 `src/ui/shell/native_fixtures.rs`（`tests_ui` 组）。

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
| `Items`、`RowCount`、`Selected`、`Top`、`VisibleRows` | List、Combo（`RowCount`、`VisibleRows` 也用于 ScrollBar 的滑块） | 行、选择、首行 |
| `RowHeight` | List、Combo | 行高（ScrollBar 不接受） |
| `NoSelectionFill`、`MultiSelect`、`SelectedRows`、`WheelStep` | List | 聊天记录、多选、每格滚轮行数（缺省 3） |
| `Open`、`OpenAnywhere`、`PopupBottom`、`PopupWiden`、`PopupFlipBelow`、`PopupRect`、`PopupHidden`、`Swatches` | Combo | 下拉窗 |
| `WheelPicks` | Combo | 下拉窗打开时滚轮选相邻行并静默关闭（§4.2） |
| `FocusTarget` | 全部（标签） | 按下它时先把焦点给该宿主 id 的编辑框（§4.1） |
| `MaxLen`、`MaxBytes`、`ByteOnly`、`Rejected`、`Allowed`、`Password`、`SelectOnFocus`、`AllSelected`、`Caret` | Edit | 输入过滤与光标；`Top` 为首个可见单元 |
| `KeepsBlink` | Edit | Tab 给它焦点时不重启光标闪烁 |
| `Multiline` | Edit | NewEdit 多行样式：Enter 把 CRLF 追加到末尾（`0x00614DEB`，不受 `MaxLen` 限制，光标不动），发 `TextChanged` 与 `Submitted`；去掉末尾 CR/LF 后文字非空时才继续激活 `Default` 按钮 |
| `TextInset` | Edit | 带框编辑框的文字区内边距；`keep_caret_in_view` 据此算可显示宽度 |
| `TabStop` | 可交互节点 | Tab 停靠点（`Edit` 缺省是，其它种类需设置） |
| `PressThrough` | 全部 | 悬停，但按下与释放落到下面的节点 |
| `Draggable`、`DragThreshold`、`DropTarget` | 全部 | 按下可拖起、拖起前的像素距离、接收被拖节点（§4.1） |

## 3. 对话框栈

- `push_dialog(root, DialogKind::Page | Modal)`：`root` 必须是已有的 `Dialog` 节点。只有栈顶对话框接收输入；`Modal` 消费所有未命中的输入。根节点设了 `Rect` 的 `Modal` 在矩形外被按下时发 `PressedOutside { dialog }`。
- `pop_dialog`（同时删除节点）、`raise_dialog`、`top_dialog`、`dialog_root`、`dialogs`。
- 每个对话框保存自己的指针状态：捕获、悬停、焦点、打开的下拉窗。被模态框盖住的页面保留自己的按下外观与焦点。`reset_pointer`、`drop_pointer_in`、`cancel_pointer` 丢弃捕获与悬停。
- `set_dialog_scale(id, ShellScale)`：把设备坐标映射回基准尺寸上的原生控件算术。
- `set_base_scale(ShellScale)`：宿主的不缩放映射，即新对话框在 `set_dialog_scale` 之前的缩放，也是滑杆 `ScaleScreen` 按其矩形尺寸 `in_window` 的基准；未设时为 `ShellScale::IDENTITY`（矩形原样映射）。

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
- 标签：左键按下悬停中的 `FocusTarget` 节点（启用与否）时，先把焦点给同一对话框中该宿主 id 的 `Edit`（同 `set_focus`：无事件，光标保持闪烁相位），然后按下照常处理。
- 悬停变化发 `Hovered { entered }`。
- 拖动：按下 `Draggable` 节点（且这次按下不是被已打开的下拉窗接收）开始一次拖动；指针在任一轴上离开按下点达到该节点的 `DragThreshold`（像素，0 为第一次移动）后才算拖起：关闭打开的下拉窗（无事件）并发 `DragStarted`，此后每次移动发 `DragMoved { x, y, target }`，`target` 为指针下树序第一个可见的 `DropTarget` 节点（它不必接收按下）。松开时若已拖起，先发 `Dropped { node, target }`，再照常处理松开；未拖起的按下照常是一次点击。`drop_pointer_in` / `cancel_pointer` 结束拖动。宿主用 `drag()`（`DragView { node, at, carrying, target }`）绘制被拖的内容，并自行决定目标是否接受。
- 模态框外按下：栈顶为 `Modal` 且其根节点设了 `Rect` 时，按在该矩形外发 `PressedOutside { dialog }`（点外面关闭的框）。

### 4.2 滚轮

落在悬停的 `List` 上时滚动 `WheelStep` 行；否则未消费（模态框除外）。打开的下拉窗所属组合框设了 `WheelPicks` 时，滚轮不论指针位置都由它接收：选中相邻行（向上一格为上一行，限于行内），关闭下拉窗且不发 `ComboClosed`（无关闭音效），选择变了才发 `Changed { Int }`。宿主附加入口：`wheel_list(node, lines, now)`（不论指针位置，按该列表所在对话框的缩放）、`wheel_popup(rows, x, y, now)`（打开的下拉窗）。

### 4.3 键盘

- 焦点只落在 `Edit` 上。隐藏或禁用（含祖先）的焦点编辑框保留焦点，但不接收按键与文字。编辑键与文字给焦点编辑框，按 `MaxLen`、`MaxBytes`（宿主用 `set_byte_len` 给出代码页长度）、`ByteOnly`、`Rejected`、`Allowed` 过滤；文字变化发 `TextChanged`。
- 下拉窗打开时它先拿到 `Enter` 与 `Escape`：`Enter` 选中热行并关闭（`ComboClosed`、`Changed`），不再激活 `Default` 按钮；`Escape` 只关闭它（无事件）。
- `Enter`：焦点编辑框先发 `Submitted`，然后激活对话框树序第一个 `Default` 按钮（须启用，可隐藏）。`Multiline` 编辑框先追加 CRLF，文字只剩 CR/LF 时 Enter 到此为止（WOL 寻呼回复框 `0x11E`）。
- `Escape`：焦点编辑框先发 `Cancelled`，然后激活 `Cancel` 按钮。
- `Tab` / `BackTab`：从焦点编辑框，或上次 Tab 停到的非编辑框停靠点，在栈顶对话框内可见、启用的停靠点之间按树序前进 / 后退并循环（GetNextDlgTabItem）；两者都没有时去第一个停靠点。停到非 `Edit` 时焦点离开编辑框，下一次 Tab 从该停靠点继续；宿主设置焦点时忘掉它。焦点编辑框本身不是停靠点时 Tab 不动。发 `FocusMoved { from, to }`。
- 下拉窗的键盘（宿主选用）：`key_popup(key, now)`：上下移动热行，Enter 选中，Escape 关闭（发 `ComboClosed`）。
- 消费：编辑框接受的键、激活了按钮、移动了焦点，或栈顶为 `Modal`。

### 4.4 焦点与光标

`focus`、`focus_in(dialog)`、`set_focus`（不重启闪烁）、`focus_edit` / `focus_edit_in`（焦点改变时重启 NewEdit 的两秒闪烁）、`caret_visible(node, now)`、`caret_visible_in`。

编辑框的横向滚动（NewEdit `scroll_to_caret`，`0x00623880` 从首个可见单元起绘制）由 gui 计算：`keep_caret_in_view(node, measure)` 把 `Top` 设为使光标前文字放得进文字区的首个单元。文字区为节点 `Rect` 按其对话框缩放换算回基准像素的宽度，减去 NewEdit 的 3 像素；有 `TextInset` 的带框编辑框改减内边距左右与光标的 2 像素。`measure` 是宿主字体的文字宽度（引擎传 `BitFont::text_width`）。字体归渲染器所有、每帧可变（`begin_frame`、换场景重建、网页字体陆续到达），gui 无法长期持有它，所以宿主在原来滚动的时机借给它测量函数；重复调用不会改变结果。

## 5. 事件

`drain_events()` 按发生顺序返回 `UiEvent`：`Activated`、`Hovered`、`Pressed`、`Changed { value, final_ }`、`ListSelected`、`ListDoubleClicked`、`Scrolled { top }`、`ComboOpened`、`ComboClosed`、`TextChanged`、`Submitted`、`Cancelled`、`FocusMoved`、`PressedOutside`、`DragStarted`、`DragMoved`、`Dropped`。宿主自己调用 `set` 不产生事件。音效由宿主根据事件播放。

未取走的事件会合并（`append_events`，引擎的 `DialogController` 积攒时也用它）：尚未取走的进入悬停在离开时与之抵消；同一节点紧挨着的 `Scrolled` 只留最新的（首行从节点读）。其它事件不合并。

## 6. 读取与宿主辅助

- 状态：`pressed`、`hovered`、`pressed_in`、`hovered_in`、`dragging`、`drag`、`shows_pressed`、`popup`。
- 列表：`list_view`（`ListView { rows, top, selected, pressed }`）、`list_geometry`、`list_row_at`、`selected_rows` / `set_selected_rows`、`log_rows(node, rows, dropped)`（聊天记录：在末尾时跟随新行，否则保持显示同样的行）、`ensure_row_visible(node, row)`（以最小滚动显示整行：在首行之上则成为首行，在末个可见行之下则成为末行；首行限于滚动范围；无事件）。
- 下拉窗：`combo_drop_down(node)`（`ComboDropDown` 几何、热行、按下的滚动条部件）、`open_popup(node, top)`、`close_popup`（宿主操作，无事件）。
- 滚动条：`scrollbar_view(node)`（`ScrollBarView { bar, thumb, held }`，按所在对话框的缩放）。
- 编辑框：`edit_state(node)`（`NewEdit`：文字、光标、`Top`、遮罩，是否持有栈顶焦点）、`set_edit_state(node, &NewEdit)`（写回文字、光标、`Top`，无事件）。
- 命中：`hit_at(root, x, y, press)`，供宿主在下拉窗下方做悬停与状态行判断。

## 7. 引擎侧接入（vera20k）

- `ui::shell::controller::DialogController` 创建 `Gui` 时设 `set_base_scale(RETAIL_SCALE)`，把引擎的对话框 id 映射到 `Gui` 的对话框栈，提供 `push`、`push_modal`、`push_overlay`、`reset_to`、`ensure_active`、`on_input`、`poll`、按控件 id 读写属性与焦点。
- 唯一的键盘适配器：`controller::gui_key(KeyCode, shift)` 与 `gui_input(code, shift, text)`。系统双击时间与范围（`set_double_click` 的输入）：`controller::host_double_click_limits`。
- `ui::shell::page`：`ensure_built`、`sync_rects`（无矩形的控件隐藏）、`page_events`（把 `UiEvent` 转成按控件 id 的 `PageEvent` 与音效）。
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
