# veraui.ini 节点树格式设计（草案，待确认）

日期：2026-11-23。状态：设计稿，未实现。依据：`docs/interface-layout.md`（对 mod 作者的承诺）、
`src/ui/match_layout.rs`（现有读取器）、`src/ui/sidebar/veraui_default.ini`（现有内置文件，只有坐标）。

## 1. 目标

- `veraui.ini` 声明整棵对局界面树：每个部件是一个节点，写明**类型、位置、图片、文字样式、命令**。
  原版界面本身就是内置的 `veraui_default.ini`，代码里不再写死部件的图片名、帧含义、颜色、字体。
- 代码只保留**类型的行为**：雷达开合动画、电力条的推进、建造网格的排列与滚动、标签页的状态机、
  命令的执行。它们读节点属性，不认部件名字。
- 去掉侧边栏图集加载、绘制、视图里按部件逐个写的代码（估计净减 600–700 行 Rust）。

## 2. 不变的承诺

- `docs/interface-layout.md` 已写明的键含义不变，现有的 mod 文件照常读取。新格式只**增加**键。
- 坏文件不让游戏失败：拒绝的键记一条问题，保留原值（沿用 `LayoutProblem`）。
- 文件只影响本机显示，不进入模拟、不参与联机比对（不变）。
- 叠加顺序：`veraui_default.ini` → mod 的 `veraui.ini` → 玩家自己的布局。

## 3. 基本概念

| 概念 | 写法 | 说明 |
|---|---|---|
| 布局 | `[名字]` | 一组部件；`[Layouts]` 把阵营映射到布局（不变） |
| 继承 | `Base=GDI` | **新增**。布局从另一个布局开始，只写不同的键。mod 的布局不写 `Base=` 时，仍从它的阵营的原版开始（与现在一致） |
| 部件 | `[布局.部件]` | 一个节点；名字就是树里的节点名，外部模块和代码按名字找 |
| 子部件 | `Parts=A,B,C` | 容器的子节点及其绘制顺序（不变） |
| 类型 | `Type=` | 决定这个节点有哪些属性、由哪段代码驱动 |
| 阵营变体 | `键.阵营=值` | **新增**。只对某个阵营的图片主题生效，例如 `Art.Yuri=radary.shp`。阵营名：`Allied`、`Soviet`、`Yuri` |

阵营变体替代代码里的 `SidebarArtNames::for_yuri`：尤里的雷达、背景、调色板换名，写进数据。

## 4. 通用键（所有类型）

| 键 | 值 | 现状 |
|---|---|---|
| `X`、`Y` | 相对父节点 `Hold` 点的像素 | 已有 |
| `Hold` | `TopLeft`…`BottomRight` 九种 | 已有（部分部件固定） |
| `Width`、`Height` | 像素，或 `Rest` | 已有（部分部件） |
| `Visible` | `yes`/`no` | **新增**，等同把部件从 `Parts=` 拿掉，但保留节点 |
| `Art` | 文件名，`.shp`/`.pcx` | **新增可写**（文档目前写着“暂不接受”） |
| `Palette` | 调色板文件名；缺省为侧边栏通用的 `sidebar.pal` | **新增** |
| `Depth` | 绘制层次的序号，同父节点内比较；缺省按 `Parts=` 顺序 | 新增，可选，默认不写 |

`Art` 可以给图片的每一帧起名，供类型使用（见各类型的“帧”）：`Frames=Idle,Pressed,Disabled,Flash,FlashActive`
表示第 0 帧叫 Idle、第 1 帧叫 Pressed……，类型按名字取帧，缺的帧退回第 0 帧（与现在相同）。

## 5. 类型与各自的键

### Panel（面板图）
只画 `Art` 的第 0 帧。原版的 `Credits`、`Top`、`Middle` 都是它。

### Text（文字）
| 键 | 值 |
|---|---|
| `Value` | 显示什么：`Credits`（金钱；观战时自动变为对局时间）、`Time`、`Power` 等（已有 `Value=Time`） |
| `Font` | `GameFnt`（原版位图字体，默认）；以后可加别的 |
| `Color` | `SideText`（阵营的侧边栏文字色，即现在 `native_radar_outline_color`）、`SideHighlight`、`White`，或 `r,g,b` |
| `Align` | `Left`、`Center`、`Right`；`Center` 以部件宽度居中（金钱是在 168 宽内居中） |
| `Shadow` | `no`（原版无阴影，默认）/ `yes` |

原版的 `Money` 是 `Type=Text`、`Value=Credits`、`Color=SideText`、`Align=Center`、`Width=168`。

### Button（按钮）
| 键 | 值 |
|---|---|
| `Art`、`Frames` | 原版按钮：`Frames=Idle,Pressed`（顶部两钮），维修/卖出 `Idle,On` |
| `Command` | 命令名（已有，mod 按钮在用）。原版：`Diplomacy`、`Options`、`ToggleRepair`、`ToggleSell` |
| `Toggle` | `yes`：按下后保持（维修、卖出） |
| `Label`、`Color` | 无图片时的文字按钮（已有，mod 按钮在用） |

点击区就是图片的尺寸（与现在相同）。

### Tabs（标签页组）
| 键 | 值 |
|---|---|
| `Step` | 相邻标签的间距，可为负（已有） |
| `Tabs` | 各标签对应的建造类别，按顺序：`Buildings,Defenses,Infantry,Vehicles` |
| `Art` | 每个标签一张图：`Art=tab00.shp,tab01.shp,tab02.shp,tab03.shp` |
| `Frames` | `Idle,Active,Disabled,Flash,FlashActive` |

### CameoGrid（建造网格）
已有的键：`Columns`、`CellX`、`CellY`、`ColumnWidth`、`RowHeight`、`Footer`、`ScrollX`、`ScrollY`、`ScrollStep`、`CellWidth`、`CellHeight`、`Tab`、`Rows`。新增：

| 键 | 值 |
|---|---|
| `RowArt` | 每行的底图（`side2.shp`），`EmptyRowArt` 空行（`side2b.shp`） |
| `EndArt`、`FootArt` | 末行下方的 `side3.shp`、`addon.shp` |
| `ScrollUpArt`、`ScrollDownArt` | `r-up.shp`、`r-dn.shp`，`Frames=Idle,Pressed,Disabled` |
| `ClockArt` | 建造进度遮罩 `gclock2.shp` |
| `DarkenArt` | 不可用遮罩 `darken.shp` |
| `LabelColor` | “就绪”等状态文字颜色，缺省 `SideHighlight` |
| `LabelStrip` | 状态文字底条，`r,g,b,a`，缺省 `0,0,0,175`（即现在的 0xAF） |

### Radar（雷达）
已有：`MapX`、`MapY`、`MapWidth`、`MapHeight`。新增：`Art`（外壳，第 0 帧为关闭，开合动画用全部帧）、
`Palette`（`Palette.Yuri=radaryuri.pal`）。

### PowerBar（电力条）
| 键 | 值 |
|---|---|
| `Art` | `powerp.shp`，`Frames=Empty,Surplus,Spare,Drain,Blink` |
| `Step` | 每格的纵向步长，缺省 3 |
| `HitWidth` | 提示框的命中宽度，缺省 8 |

### Bar（命令栏）与 CommandButtons
`Bar` 已是容器部件。新增 `Rail`（`lspacer.shp`）、`LeftCap`（`lendcap.shp`，`Frames=Base,Pressed,Closed`）、
`RightCap`（`rendcap.shp`）、`Slot`（`bttnbkgd.shp`）。命令按钮沿用 UIMD.INI 的 `ButtonList`，图片 `button%02d.shp`
写成 `ButtonArt=button%02d.shp`。

### 没有图片的类型
`Row`、`Column`（已有 `Row`）：只排列子部件。`Battlefield`（`Field`）不变。

## 6. 仍留在代码里的（类型行为，不是部件）

- 雷达开合的帧序列、小地图的绘制（7.6k 行雷达渲染不迁）。
- 电力条的段数推进与闪烁（`PowerBarAnimState`）。
- 建造网格的条目顺序、滚动、就绪/暂停/队列徽章的显示条件（`CameoStrips`、`sidebar_view`）。
- 标签页的禁用与闪烁状态机（`gadget_flash`）。
- 命令的执行、点击顺序（`ui::gadget`）。
- Ares 按阵营改文件名：作为读取 `Art` 后的一层名字替换，Ares 配置优先（本机没有 Ares 模组，只能靠单元测试）。
- 侧边栏宽度变化时的平滑缩放（`frame.rs`）。

## 7. 默认文件草稿（`veraui_default.ini`）

```ini
; The built-in veraui.ini: the original interface as layouts. It is the
; bottom layer: a mod's veraui.ini and the player's own layout write their
; values over the layout its side starts from.

[Layouts]
GDI=GDI
Nod=Nod

; ---- GDI: the allied position table; every side's pictures ----
[GDI]
Version=2

[GDI.Side]
Width=168
Height=168
Parts=Credits,Money,Top,Diplomacy,Options,Radar,Middle,Repair,Sell,Tabs,Cameos,Power

[GDI.Credits]
Type=Panel
Art=credits.shp

[GDI.Money]
Type=Text
Value=Credits
Y=2
Width=168
Align=Center
Color=SideText

[GDI.Top]
Type=Panel
Y=16
Art=top.shp
Parts=Diplomacy,Options

[GDI.Diplomacy]
Type=Button
X=11
Y=4
Art=diplobtn.shp
Frames=Idle,Pressed
Command=Diplomacy

[GDI.Options]
Type=Button
X=83
Y=4
Art=optbtn.shp
Frames=Idle,Pressed
Command=Options

[GDI.Radar]
Type=Radar
Y=48
Art=radar.shp
Art.Yuri=radary.shp
Palette.Yuri=radaryuri.pal
MapX=16
MapY=1
MapWidth=140
MapHeight=108

[GDI.Middle]
Type=Panel
Y=158
Art=side1.shp
Parts=Repair,Sell,Tabs

[GDI.Repair]
Type=Button
X=20
Y=8
Art=repair.shp
Frames=Idle,On
Toggle=yes
Command=ToggleRepair

[GDI.Sell]
Type=Button
X=84
Y=8
Art=sell.shp
Frames=Idle,On
Toggle=yes
Command=ToggleSell

[GDI.Tabs]
Type=Tabs
X=26
Y=39
Step=29
Tabs=Buildings,Defenses,Infantry,Vehicles
Art=tab00.shp,tab01.shp,tab02.shp,tab03.shp
Frames=Idle,Active,Disabled,Flash,FlashActive

[GDI.Cameos]
Type=CameoGrid
Y=227
Columns=2
CellX=22
CellY=1
CellWidth=60
CellHeight=48
ColumnWidth=63
RowHeight=50
Footer=33
RowArt=side2.shp
EmptyRowArt=side2b.shp
EndArt=side3.shp
FootArt=addon.shp
ScrollX=39
ScrollY=7
ScrollStep=46
ScrollUpArt=r-up.shp
ScrollDownArt=r-dn.shp
ClockArt=gclock2.shp
DarkenArt=darken.shp
LabelColor=SideHighlight
LabelStrip=0,0,0,175

[GDI.Power]
Type=PowerBar
X=5
Y=227
Art=powerp.shp
Frames=Empty,Surplus,Spare,Drain,Blink
Step=3
HitWidth=8

[GDI.Bar]
Rail=lspacer.shp
LeftCap=lendcap.shp
RightCap=rendcap.shp
Slot=bttnbkgd.shp
ButtonArt=button%02d.shp

; Pause-menu and in-match dialog backgrounds, three sizes.
[GDI.Backgrounds]
Type=Panel
Art=bkgdlg.shp,bkgdmd.shp,bkgdsm.shp
Art.Yuri=bkgdlgy.shp,bkgdmdy.shp,bkgdsmy.shp
Palette=uibkgd.pal
Palette.Yuri=uibkgdy.pal

; ---- Nod: the Soviet and Yuri position table; only what differs ----
[Nod]
Version=2
Base=GDI

[Nod.Diplomacy]
X=14
Y=5

[Nod.Options]
X=86
Y=5

[Nod.Repair]
X=33
Y=7

[Nod.Sell]
X=85
Y=7

[Nod.Tabs]
X=20
Step=32

[Nod.Cameos]
ColumnWidth=64
Footer=25
ScrollStep=45

[Nod.Power]
X=0
```

`Base=` 让 Nod 只写与 GDI 不同的 12 个数，整份文件约 170 行（现有只含坐标的文件 149 行）。

## 8. 兼容与校验

- `Version=2` 的布局才接受新键；`Version=1` 照旧（新键记“未知键”问题，与现在一致）。
- 原版部件的类型不能改（`Repair` 的 `Type=Button` 是固定值），否则记问题并保留原值；mod 自加的部件可任选类型。
- 每个类型只接受它的键；拼错的键、越界的数、找不到的图片都记问题，保留默认值。
- `Art` 指向的文件不存在时，退回默认文件的图片（与现在“缺图跳过”不同：更宽容），并记问题。

## 9. 实现步骤与验收

1. 读取器：`Base=`、`键.阵营`、各类型新键；默认文件改成草稿的样子。验收：读出的布局表与现在逐字段相等。
2. 图集：按各部件的 `Art`（含变体）加载、打包；删掉逐部件加载代码。验收：图集像素与贴图坐标逐字节相同。
3. 绘制与视图：按类型通用处理；删掉逐部件分支和 `ChromeArt`。验收：三阵营 × 三套示例布局 × 几种窗口尺寸截图，
   与改动前逐像素对比；有差异逐处说明（目标是无差异）。
4. 文档：`docs/interface-layout.md` 补新键；示例文件补一个改图、改颜色的布局。

## 10. 待你确认的问题

1. 阵营变体的写法 `Art.Yuri=` 是否合适（另一种是 `[GDI.Radar.Yuri]` 子段）？
2. 颜色的命名值（`SideText`、`SideHighlight`）是否够用，还是一律写 `r,g,b`（阵营色则无法表达）？
3. `Art` 不存在时退回默认图片，还是像现在一样不画？
4. 命令栏的命令按钮仍沿用 UIMD.INI 的 `ButtonList`，还是也搬进 `veraui.ini`（`[GDI.Bar] Parts=Team01,...`）？
