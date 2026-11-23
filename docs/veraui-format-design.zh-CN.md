# veraui.ini 节点树格式设计（第二稿，待确认）

日期：2026-11-23。状态：设计稿，未实现。用户 11-23 定：**不考虑旧 mod 的兼容**，格式可以重做。

## 1. 目标

- `veraui.ini` 就是对局界面的节点树：一个段是一个节点，键就是节点属性。原版界面是内置的
  `veraui_default.ini`，mod 和玩家的文件在它上面改。
- 位置由 gui 的布局算（`Hold`、`X`、`Y`、`Row`/`Column` 的排列和间距），不再由宿主按原版算式逐个计算。
- 代码只保留类型的行为（雷达开合、电力条推进、建造网格的条目与滚动、标签状态、命令执行、点击规则），
  不认部件名字，不写死图片名、帧含义、颜色。
- 预期删除：读取器里的部件表与数字绑定（`match_layout.rs` 2278 行中的大部分）、侧边栏图集的逐部件加载、
  绘制与视图的逐部件分支、`SidebarChromeLayoutSpec` 的大部分字段及其使用。

## 2. 去掉的东西（相对现在的 veraui.ini）

| 去掉 | 原因 |
|---|---|
| `Version=`、固定值的键、“这个键暂不接受”的规则 | 不再有旧版本要兼容；每种类型列出它接受的键 |
| 原版部件的特殊键（雷达的 `MapX`…、建造网格的 `ScrollX`…、标签的 `Step`） | 改成子节点：小地图、滚动按钮、每个标签都是自己的节点，用通用的 `X`、`Y` |
| 部件名和代码里数字字段的一一绑定（`Number` 枚举，约 40 个） | 节点属性直接给布局用 |
| `[Layouts]` 里按阵营选整套布局 | 保留，但布局之间可以 `Base=` 继承，只写差异 |

## 3. 文件结构

```ini
[Layouts]          ; 阵营用哪个布局（[Sides] 名，Default= 兜底）
Default=Original

[Original]         ; 一个布局：只放布局级的键
Base=              ; 从哪个布局开始，缺省从空树开始（内置文件除外，mod 的布局缺省 Base=它阵营的原版）
Root=Screen        ; 根节点

[Original.Screen]  ; 节点 = [布局.节点名]
Type=Row
Parts=Field,Side
...
```

- 节点名在一个布局内唯一，就是树里的 `Name`，外部模块按名字找。
- `Parts=` 列子节点，也是绘制顺序；不在任何 `Parts=` 里的节点不显示。
- 继承：`Base=` 布局的节点全部带过来；本布局写同名节点的哪个键，就只改那个键。新节点要被某个 `Parts=` 列出才显示。
- 阵营变体：`键@阵营=值`，只在该阵营的图片主题下生效，例如 `Art@Yuri=radary.shp`。阵营：`Allied`、`Soviet`、`Yuri`。

## 4. 通用键

| 键 | 值 |
|---|---|
| `Type` | 见第 5 节 |
| `Parts` | 子节点，按绘制顺序 |
| `Hold` | 相对父节点的哪个点：`TopLeft` … `BottomRight` 九种，缺省 `TopLeft` |
| `X`、`Y` | 离 `Hold` 点的像素；在 `Row`/`Column` 里则不写，由排列决定 |
| `Width`、`Height` | 像素；`Rest` 占满父节点剩下的；缺省为图片尺寸 |
| `Visible` | `yes`/`no` |
| `Art` | 图片文件（`.shp`/`.pcx`）；多张用逗号，节点类型按顺序取用 |
| `Frames` | 给帧起名：`Idle,Pressed,Disabled…`；类型按名字取帧，缺的退回第 0 帧 |
| `Palette` | 调色板，缺省 `sidebar.pal` |
| `Color` | `r,g,b`、`r,g,b,a` 或 `[Colors]` 里的名字 |

## 5. 类型

| 类型 | 画什么 | 自己的键 |
|---|---|---|
| `Row`、`Column` | 不画，排列子节点 | `Gap`（可为负）、`Reverse=yes`（从右往左 / 从下往上） |
| `Image` | `Art` 的第 0 帧 | — |
| `Text` | 一行字 | `Value`（`Credits`、`Time`、`Power`…；或 `Text=` 固定文字）、`Font`、`Align`、`Shadow` |
| `Button` | 按状态取帧 | `Command`、`Toggle=yes`、`Label`（无图时的文字） |
| `Tab` | 标签按钮 | `Tab=Buildings/Defenses/Infantry/Vehicles`；帧 `Idle,Active,Disabled,Flash,FlashActive` |
| `Radar` | 雷达外壳与开合动画 | 子节点 `Minimap` |
| `Minimap` | 小地图（宿主区域） | — |
| `CameoGrid` | 建造图标网格，按剩余高度决定行数 | `Columns`、`Cell=60,48`、`Pitch=63,50`、`RowArt`、`EmptyRowArt`、`Clock`、`Darken`、`LabelColor`、`LabelStrip`、`Tab`（固定显示某一类） |
| `Repeat` | 把一张图沿纵向或横向铺到指定长度 | `Along=Down/Right` |
| `PowerBar` | 电力条 | `Step`、`HitWidth`；帧 `Empty,Surplus,Spare,Drain,Blink` |
| `CommandBar` | 命令栏底轨、两端、底座和命令按钮 | `Rail`、`LeftCap`、`RightCap`、`Slot`、`Buttons`（命令名到图片）；**哪些按钮、顺序、空格默认来自 `UIMD.INI` 的 `ButtonList`**（单人 `[AdvancedCommandBar]`，联机 `[MultiplayerAdvancedCommandBar]`，`x` 为空格）；`veraui.ini` 写 `Parts=`（单人与遭遇战）或 `MultiplayerParts=`（联机）就覆盖它 |
| `Battlefield` | 战场 | 子节点：`Timers`、`Messages`、`Banner`、`Tooltip` |

`Text` 的 `Value=Credits` 在观战时自动显示对局时间（现在的行为）。

## 6. 颜色表

```ini
[Colors]
SideText=164,210,255
SideText@Soviet=255,255,0
SideText@Yuri=255,255,0
Black=0,0,0
LabelStrip=0,0,0,175
```

阵营色（现在写死在 `sidebar_text.rs` 的 `HIGHLIGHT_*` 和 RGB565 换算）由颜色表给出；mod 改一处，所有用到它的部件一起变。

## 7. 默认文件草稿（侧边栏与命令栏部分）

```ini
; The built-in veraui.ini: the original interface. Every number here is its
; original value; a mod's veraui.ini and the player's own layout write over
; the layout its side starts from.

[Layouts]
; [Sides] names: the Allies use the GDI position table, the Soviets and
; Yuri the Nod one.
GDI=GDI
Nod=Nod
ThirdSide=Nod

[Colors]
SideText=164,210,255
SideText@Soviet=255,255,0
SideText@Yuri=255,255,0
LabelStrip=0,0,0,175

[GDI]
Root=Screen

[GDI.Screen]
Type=Row
Parts=Field,Side

[GDI.Field]
Type=Battlefield
Width=Rest
Parts=Timers,Messages,Banner,Tooltip,Bar

[GDI.Side]
Type=Image
Width=168
Height=Rest
Parts=Credits,Top,Radar,Middle,Cameos,Power

[GDI.Credits]
Type=Image
Art=credits.shp
Parts=Money

[GDI.Money]
Type=Text
Value=Credits
Y=2
Width=Rest
Align=Center
Color=SideText

[GDI.Top]
Type=Image
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
Art@Yuri=radary.shp
Palette@Yuri=radaryuri.pal
Parts=Minimap

[GDI.Minimap]
Type=Minimap
X=16
Y=1
Width=140
Height=108

[GDI.Middle]
Type=Image
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
Type=Row
X=26
Y=39
Gap=1
Parts=Buildings,Defenses,Infantry,Vehicles

[GDI.Buildings]
Type=Tab
Tab=Buildings
Art=tab00.shp
Frames=Idle,Active,Disabled,Flash,FlashActive

[GDI.Defenses]
Base=Buildings          ; a node may start from another node of the layout
Tab=Defenses
Art=tab01.shp

[GDI.Infantry]
Base=Buildings
Tab=Infantry
Art=tab02.shp

[GDI.Vehicles]
Base=Buildings
Tab=Vehicles
Art=tab03.shp

[GDI.Cameos]
Type=Column
Y=227
Height=Rest
Parts=Grid,End,Foot

[GDI.Grid]
Type=CameoGrid
Height=Rest
Columns=2
Cell=60,48
CellAt=22,1
Pitch=63,50
Footer=33
RowArt=side2.shp
EmptyRowArt=side2b.shp
Clock=gclock2.shp
Darken=darken.shp
LabelColor=SideText
LabelStrip=LabelStrip

[GDI.End]
Type=Image
Art=side3.shp
Parts=ScrollDown,ScrollUp

[GDI.ScrollDown]
Type=Button
X=39
Y=7
Art=r-dn.shp
Frames=Idle,Pressed,Disabled
Command=ScrollDown

[GDI.ScrollUp]
Base=ScrollDown
X=85
Art=r-up.shp
Command=ScrollUp

[GDI.Foot]
Type=Image
Art=addon.shp

[GDI.Power]
Type=PowerBar
X=5
Y=227
Height=Rest
Art=powerp.shp
Frames=Empty,Surplus,Spare,Drain,Blink
Step=3
HitWidth=8

[GDI.Bar]
Type=CommandBar
Hold=Bottom
Height=32
Width=Rest
Rail=lspacer.shp
LeftCap=lendcap.shp
RightCap=rendcap.shp
Slot=bttnbkgd.shp
; Which buttons, in which order, with which gaps: UIMD.INI's ButtonList,
; unless a layout writes Parts= (solo and skirmish) or MultiplayerParts=
; (online), x for a gap. Here only each command's picture (Idle,Pressed).
Buttons=Team01:button00.shp,Team02:button01.shp,Team03:button02.shp,TypeSelect:button03.shp,Deploy:button04.shp,AttackMove:button05.shp,Guard:button06.shp,Beacon:button07.shp,Stop:button08.shp,PlanningMode:button09.shp,Cheer:button10.shp

; ---- Nod: the Soviet and Yuri table; only what differs ----
[Nod]
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
Gap=0

[Nod.Grid]
Pitch=64,50
Footer=25

[Nod.ScrollUp]
X=84

[Nod.Power]
X=0
```

计时器、消息、横幅、提示框和暂停菜单的背景按同样方式写在 `Field` 下（略）。

## 8. 与现在行为的差异（需要截图核对）

`UIMD.INI` 保留（用户 11-23 定）：命令栏的按钮、顺序、空格默认仍由它的 `ButtonList` 决定，`veraui.ini` 的
`Parts=` / `MultiplayerParts=` 可以覆盖；Phobos 的 `[Sidebar]`（左侧超武栏、`CenterPauseMenuBackground`）仍从它读。

- 建造网格的 3–5 列（VERA 自加功能）现在按浮点间距排，改成整数像素后，列的位置可能差 1 像素。
- `Tabs` 的间距现在是“标签起点相距 29”，改成 `Row` + `Gap` 后依赖标签图的宽度；换了宽度不同的标签图，间距会跟着变（这正是 Row 的意义）。

## 9. 实现步骤与验收

1. 新读取器：段 → 节点，键 → 属性，`Base=`（布局与节点两级）、`@阵营`、颜色表；按类型校验。删除旧读取器的部件表与数字绑定。
2. 侧边栏和命令栏：从树建 HUD 节点，位置交给 gui 布局；绘制和视图按类型；图集按 `Art` 加载。删除逐部件代码。
3. 战场部件（计时器、消息、横幅、提示框）同样从树来。
4. 布局编辑模式改为写节点属性；文档与示例按新格式重写。
5. 验收：三阵营 × 示例布局 × 800×600、1024×768、1920×1080、缩放 1.5/2 截图，与改动前对比；只接受第 8 节列出的差异。

## 10. 待你确认

1. 阵营变体 `键@阵营=`、颜色表 `[Colors]`、节点继承 `Base=` 这三个写法是否可以。
2. 第 8 节的两处可能差异是否接受。
