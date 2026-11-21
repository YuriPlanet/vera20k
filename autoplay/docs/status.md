# 现状

更新于 2026-11-21，分支 `feature/autoplay-v4`。
**这份文档只写当前代码的事实。** v2 时期的实测记录在 `history/status-v2.md`。

---

## 1. 跑的是什么

一个由策略驱动的房子：

- **默认大脑** `sim::strategy::assist::AssistBrain`（20 文件，约 14,900 行），通过
  `VERA20K_BRAIN` 选择；不设或 `assist` 都是它，`v2` 切引擎外的
  `autoplay::policy::Policy`。
- 它做的事，按模块：展开 MCV（`expand`）、花掉钱（`build` 建基地、`composition`
  填生产线）、使用已有的（`orders` 工程师/间谍/驻扎、`advance` 推%).
- 每一步都是玩家能发出的命令：部署、建造、放置、对自己单位的下令。**它不跑
  native 电脑 AI 的决策块** —— 人类房子在 native 里本来就不跑那些。

## 2. 已验证（2026-11-21）

| 项 | 结果 |
|---|---|
| `--profile fast` 构建 | 通过，`vera20k.exe` 约 91 MB |
| `cargo test -p vera20k-core --lib -- sim::strategy::assist` | **175 passed** |
| 改名 `retail` → `assist` | 无行为改动，构建与测试通过 |
| 会话重构（`AutoplaySession`） | 行为保持：同一 host 调用、同一 take_back、同一顺序 |
| 上游合并 `0e831f2c3` | 上游 377 提交并入，冲突四处，测试通过 |
| 驻扎上限 `GARRISON_MAX = 4` | 步兵流失停止，limbo 52 → 8 |
| 袭击支队（raid） | 三种子里出现第一场胜利（seed 24301，33977） |

## 3. 基线（对局强度）

判据工具：`python -m tools.autoplay_check`，Death.mmx、america、normal、36000 ticks。
每一格跑完写一行 JSON 到 `target/autoplay-results/`，`judge()` 只认四件事：
军队（fighters）、闲置（idle）、未认领（unclaimed）、**push 门翻转次数**（`push_flips`）。

| 基线 | 提交 | seed 24301 | seed 19 | seed 10 |
|---|---|---|---|---|
| baseline-v2（驻扎上限后） | `7934c2764` | timeout own=130 army=68 | timeout own=108 army=63 | timeout own=121 army=72 |
| raid（支队不再看军队大小） | `cbeb8ec34` | **won 33977** own=155 army=84 | timeout own=128 army=66 | timeout own=106 army=55 |
| 基地锚点不再被第二个指挥中心夺走 | `e75815a59` | **won 32491** own=158 army=84 | timeout own=128 army=66 | timeout own=106 army=55 |
| 每座精炼厂三辆采矿车 | `f4189fa6e` | **won 35276** own=158 army=86 | timeout own=126 army=66 | timeout own=147 army=82 enemy=10 |
| 防守上限 8 → 12 | `f9ea878bd` | **won 29484** own=154 army=79 | timeout own=158 army=90 idle=82 | **won 35820** own=157 army=93 |

读法：三局都打满 36000 ticks 之前，只有第一场胜。第一场胜利来自「屯兵 + 袭扰」
两条线分开：fist 留在家里等 1.2 倍优势，支队去打法娘的采矿与精炼。

### 防守上限 8 → 12（这条留下来了，第一次两局都赢）

`DEFEND_MAX`（`src/sim/strategy/assist/squads.rs`）是 Defend 支队的人数上限——「够挡下袭击」
的那个数。八个人在 Death.mmx seed 19 上不够，fist 出门以后袭击队就在家里拆房子。
（更正：`threat=[14,7,2,...]` 是**全图**敌方按兵种计数，见 `census.rs:840-851` 的 `counter(ty)`，
不是 rings 内的数量。ring 内的数量是 `enemies_near(base_cell, THREAT_RADIUS)`，逐帧都在变。）

实测（Death.mmx，america，normal，36000 帧，跑两遍逐字节一致）：

| seed | 8（基线） | 12 |
|---|---|---|
| 24301 | won 35276 own=158 army=86 lost=52:112 | **won 29484** own=154 army=79 lost=33:96 |
| 19 | timeout own=126 enemy=70 army=66 lost=52:43 | timeout own=158 enemy=75 army=90 idle=82 lost=47:46 |
| 10 | timeout own=147 enemy=10 army=82 lost=58:114 | **won 35820** own=157 army=93 lost=55:124 |

第一场胜利提前了 5792 帧，交换比从 52:112 变成 33:96；最弱的一局从 timeout 变成胜利。
seed 19 仍是 timeout，但它的物质力量比基线强得多（own 126→158、army 66→90），
卡住它的是 `push=outgunned`：军队 90 人却 idle=82，门在军队自己更强的时候把它按在家里。
那是下一件事。

### 防守队按突袭规模给（测过，三局都没有收益，留下了）

`squads.rs` 的 Defend 支队原来是固定 `.take(DEFEND_MAX)`：来 3 个敌人也是 12 个人去，
来 20 个也是 12 个人去。现在 `want = raiders.max(DEFEND_MAX).min((census.fighters / 2).max(DEFEND_MAX))`，
即「至少 12，最多突袭规模，且不超过军队的一半」，其中
`raiders = census.base_cell.map_or(0, |home| census.enemies_near(home, HOME_THREAT_CELLS))`。

实测（Death.mmx，america，normal，36000 帧）：与 defend12 **逐字节一致**——
24301 won 29484 own=154 army=79 inc=6270；19 timeout own=158 enemy=75 army=90 inc=6742；
10 won 35820 own=157 enemy=1 army=93 inc=5179。
原因：加了 `AUTOPLAY_TRACE_SQUADS` 的探针（已删）后看到，三局里 `raiders` 一直在 1..9，
从没超过 12，所以这条只在突袭大于 12 时才生效。留着是因为地板 12 只对小于 12 的突袭正确，
而回归测试 `a_raid_bigger_than_the_floor_calls_more_than_the_floor`（20 个敌人、30 个兵 → 15 个防守）
在去掉这条公式时失败（left: 12, right: 15）。
### 试过并且更差的（不要重试）

- **把 `Item::Refinery` 加进 `yard_reserve`**（想让第二个精炼厂也能存钱买到）：
  唯一一局胜利从 `won at tick 32491` 掉到 `won at tick 33964`，own 144 → 136，
  income 8613 → 5402，全局 `auto play: build` 行 785 → 242。加 `fighters>=` 条件
  也没用（那条件几乎恒真，两局数字完全一样）。步兵被钱荒饿死，
  `src/sim/strategy/assist/build.rs` 里那段注释早就写过这个价。
  另一条否证：seed 10 的收入在加与不加时都是 1943/min，可见它的贫穷不是
  「精炼厂不够」造成的，第二个 bay 不是那局的瓶颈。
- **`ARMY_TO_ATTACK` 10 → 20**（军队门槛抬高，想「屯更多兵再出门」）：三局
  全部 timeout，连原本 32491 胜的那局也丢了。原因不是门槛本身——`army at first
  push` 三局都还是 10，说明门不是它在把；抬它把 `fist_needs` 一起抬到 20，
  支队被饿死，袭扰线没了。
- **把 `PUSH_SUPERIORITY_PERMILLE` 从 1200 往下调**（想解 seed 19 的 `push=outgunned`）：
  两档都试过，两档都更差。降到 **1000**（同等兵力就出门）：seed 10 从
  `won at tick 35820` 掉到 timeout（own 157 → 116，army 93 → 50，lost 55:124 → 81:79）；
  seed 24301 从 `won at 29484` 掉到 `won at 30341`；seed 19 自己的 stuck 从 82 掉到 11
  （军队确实出门了），但仍 timeout，而且 own 158 → 151、lost 47:46 → 47:57。
  降到 **1100** 同样三局都差：24301 `won at 31756`，seed 10 也丢了胜利
  （timeout own=131 army=62 epoints=7336）。也就是说悬崖正好在 1.1 和 1.2 之间，
  1200 不是随手取的数，是这三局里刚好能站住的下限。
  结论：门不是无谓的谨慎。同等兵力出门，等于用军队换一块守不住的基地——
  这正是 `squads.rs:88-101` 那段注释写的，现在有数了。1200 回退后三局逐字节复原。
- **一帧多派几个命令**（`ADVANCE_ORDERS_PER_FRAME` 8 -> 32）：seed 24301 直接
  `lost at tick 9931`——20 分钟不到就丢了家。seed 10 反而提前到 `won 34220`，
  seed 19 依旧 timeout。所以 8 不是保守值，它是「军队成形」的速度：
  一次把命令全发出去，等于让先到的兵独自冲上去送死。已回退。
- **把「原地待命」的 Move 重新接下来**（advance.rs 的 push 循环原来对 `mission == Move`
  无条件 `continue`，改成只有 `obs.destination()` 还在时才跳过）：三局全 timeout。
  seed 24301 `won 31893` -> timeout own=136 enemy=52 army=61；
  seed 19 timeout own=162 enemy=52 -> own=137 enemy=61 army=72；
  seed 10 `won 35437` -> timeout own=160 enemy=61 army=90。
  推论本身成立（到达之后任务号不会自己变回 Sleep，这些兵会被永远跳过），
  但把它们接回来等于把已经到位的兵一次次重新编队，军队的集中度反而更差：
  三局的 army 一个降、一个降、一个升，全都输了。已回退。
- **把 limbo 里的兵从 `able` 里去掉**（src/sim/strategy/assist/census.rs，一个兵在
  `limbo` 里既走不动也不能开火，`fighters` 早就不数它了，`able` 却还在数）：
  这是对的方向，但只换来「有变化」，没换来胜利。
  seed 24301 `won 29484` -> `won 31893`（own 154 -> 138，lost 33:96 -> 48:111）；
  seed 19 enemy 75 -> 52、stuck 82 -> 46、push=push 的采样从「几乎不出现」变成 21/60，
  也就是说这场原本死锁的和局第一次真的开打了，但 36000 帧内还没打完；
  seed 10 `won 35820` -> `won 35437`（income 5179 -> 7613）。
  两胜保住，但参考局的胜利慢了 2400 帧，暂时算「换了打法」而不是「变强了」。
- **把 push 的优势门从 1.2 降到 1.15**（`PUSH_SUPERIORITY_PERMILLE` 1200 -> 1150，
  想正好覆盖 seed 19 的 1.164）：净亏一胜，seed 19 也没被救回来。
  seed 24301 `won 29484` -> `won 35529`（army 79 -> 117，own 154 -> 187，stuck 17 -> 106）；
  seed 19 依旧 timeout（enemy 75 -> 81）；
  seed 10 `won 35820` -> timeout（own 157 -> 129，lost 55:124 -> 73:119）。
  和 1.0、1.1 两档合起来看，seed 10 的门槛在 1.15 之上，seed 19 的 1.164 并不是
  唯一卡点——门开一条缝只会让军队提前出发、在大地图上磨得更久。已回退。
- **把袭扰队从 4 个加到 8 个**（`HARASS_MAX` 4 -> 8，想去打对方的经济）：三局全 timeout。
  seed 24301 `won 29484` -> timeout own=179 enemy=1 army=107 lost=35:89 inc=7255
  （自己的分数和军队规模都是历次最大，还是差最后一个建筑）；
  seed 19 enemy 75 -> 63 但 128 -> 159 own 也没赢；
  seed 10 `won 35820` -> timeout own=136 enemy=44。
  袭扰队变大会把 Attack 那一堆摊薄，而 punch 的门并没有因此更容易过。已回退。
- **让拳头优先打「打得过」的目标而不是最近的目标**（`reachable_target` 加一个 `worth`
  谓词，在最近的 12 个目标里先挑「走得到且打得过」的）：净亏一胜。
  seed 10 从 `won at 35820` 提前到 `won at 33581`（income 5179 → 7773），
  但 seed 24301 从 `won at 29484` 掉到 timeout，而且终局 `enemy=1`——
  就差最后一个建筑：lost 33:96 → 56:116，income 6270 → 4956，own 154 → 161。
  也就是说它把一场快胜换成了一场磨到读秒还差一点的地推。
  两种写法（先在全图筛可胜目标、再在 12 个候选里筛）结果逐字节相同，
  说明问题不在候选范围，而在「舍近求远」本身：那 12 个里已经有一个可胜的了。
  已回退，三局逐字节复原。
- **把 fist 的门单独抬到 20**（新增 `PUSH_ARMY`，`mop_up` 不再能把它降回 10）：
  机制确实生效了——`army at first push` 从三局都停在 10 抬到 31 / 39 / 35，
  但三局全部 timeout：seed 24301 从 `won at tick 32491` 掉到 own=134 enemy=34
  army=71 income=6956（基线 own=144 enemy=1 army=84 income=8613），seed 19
  own=127 enemy=59，seed 10 own=78 enemy=41。也就是说早出门的那十个兵不是
  「散着送」，而是维持压力和 income 的一部分：等凑够 20 个再走，攒兵的速度
  反而跟不上消耗，peak army 从 84 掉到 71。
- **推进目标的三种改法**（按角色权重分配目标 / 从军队中心瞄准拳头 / 一个目标
  打到最后）都输掉原本 tick 25636 赢的那局，见 `src/sim/strategy/assist/mod.rs`
  的模块头注。
- **把撤退线从三成抬到六成**（`RETREAT_HEALTH_PERMILLE` 300 → 600，想让部队早点
  回家保命）：唯一一局胜利丢了——seed 24301 `won at tick 32491` → `timeout`，
  enemy 1 → 36，而且那一局的交换比更差（lost 41:108 → 52:89）。原因是提前撤退的
  单位走在路上同样是废的，还会被追着打：60% 血就脱离，等于把还能打的输出和
  一条命一起交出去。seed 19 的交换比确实变好（47:58 → 42:76）、army 66 → 89，
  但仍然 timeout。已还原为 300。

### 已经排除的嫌疑（别再查）

- **步兵占比失控（编制层）**：一场看起来要输的局里 `infantry=59 / fighters=74`，
  55% 的上限像是没生效。加了 `AUTOPLAY_TRACE_COMPOSITION=1`（见 `31b7d9348`）
  实测：钱袋在大部分采样点是负的（-7492..3273），`vehicle_floor` 从 2000 落到
  600，`rich` 全程 false，20 个采样点里 15 个 barracks 是 held 的。步兵仍然
  从 7 涨到 56，是因为钱袋长期停在「车价 600」和「一兵 200」之间，而那一档
  正是设计好的 `a_soldier_would_reset_the_saving` 要放行步兵的档位。
  编制层不是漏洞，步兵过多的军队要从别处解释。
- **拳头瞄准打不动的目标（已排除，但留下一处未解释的矛盾）**：在 `advance` 里加过
  临时诊断（`AUTOPLAY_TRACE_CANNOT`）实测 seed 24301 一局：179 次 `Engage::Cannot`，
  其中 172 次的 `obs.health(target)` 是 `None`、`obs.enemy_entities()` 也说没有，
  但同一个 obs 同一帧里 `brain.census.enemies` 里确实有这个 id
  （`in_census=true / live=false / hp=None`），目标类型集中在
  GAROBO / GAPILL / NASAM / ATESLA。`census.refresh` 每次调用都从
  `obs.enemy_entities()` 重建（`src/sim/strategy/assist/census.rs:690` 清空、
  `:840` 重填），同一帧两个答案相反，这一点还没有解释。另有一次 `target:` 采样显示
  26 个采样点里 24 个 `structure=0`（拳头当时在搜索分支，目标列表里只有占位 id）。
  两种改法都试了：(a) 建 `structures` 时按 `obs.health` 过滤——53 个 `hp=None`
  一个都没少；(b) 记下打不动的目标，达到 `ADVANCE_ORDERS_PER_FRAME` 次拒绝且
  `push_stalled` 为真时丢掉它两分钟的 `hopeless` 表——不加 stall 门槛时 seed 24301
  从 `won at 35276` 掉成 timeout（army 86 → 77），加上门槛后三局与基线逐字节相同，
  说明这个规则在这三局里从不触发。两种都未保留，代码已还原，工作区干净。
  留下的结论：seed 24301 尾段 `idle=71 / fighters=86` 不是这些 `Cannot` 造成的。
- **推进目标来回跳**：`target=` 在 seed 10 的尾段里 27 个采样点盯着 3105、
  14 个盯着 7017，整局只出现过 4 个不同目标；seed 24301 也只有 9 个，
  且单个目标能连盯 19 个采样点。目标承诺是有效的，拳头没有被拖着跑。

### 真正的差距是经济，不是交换比（修正上面的一条）

先前把三局的差别归到「损失更多」上（41:108 对 64:89、47:58），那用的是
`src/sim/strategy/assist` 里 census 行的 `pts=` 和 `lost=`。用同一份基线重新量
一遍（`target/autoplay-results/base-858a8ec.jsonl`，三局同 commit、同参数）后，
真正拉开差距的是收入曲线：

| seed | 开局 | 中期 | 尾段 | own | army | income 尾值 |
| 24301 | 2360 | 5165 | 4516 | 144 | 84 | **8613/min** |
| 19 | 1180 | 1860 | 7040 | 128 | 66 | 4925/min |
| 10 | 1360 | 1540 | 2560 | 106 | 55 | **1943/min** |

三局的 `harv=6..7 ref=3 yard=1` 结构相同，收入却差 4.4 倍，所以差别不在
「精炼厂够不够」（那条已经写在上面否掉了），而在每车矿石的往返：seed 10 的
基地在 (157,177)，`crates=1[(57, 86)]`，尾段 `threat=[10,2,2]`——它一直在被
袭扰，而 24301 的 `threat=[0,1,0]` 全程安全。结论：这是防御/袭扰问题，不是
交换比问题。下一轮该查的是「为什么袭扰能一直打断矿线」。


### 每座精炼厂三辆采矿车（这条留下来了）

三局的结构其实一样：`harv=6..7 ref=3`。差别在每辆车跑一趟带回多少。把
`HARVESTERS_PER_REFINERY`（`src/sim/strategy/assist/build.rs`）从 2 抬到 3 之后，
三局都造到九辆车三座厂，而原本 1943/min、只剩 52 个敌人的那一局变成
own 147 / enemy 10 / army 82；赢的那局仍然赢（35276，比 32491 晚 2785 帧）。
代价是第三局从 128/61 变成 126/70。

### 精炼厂选址不是经济杠杆（已排除）

加了 `AUTOPLAY_TRACE_SITE=1`（`src/sim/control/placement.rs` 的 `cell_for` 里，
env-gated）量「所选的格子离最近一块矿多远」：seed 10 三座 `GAREFN` 落在
离矿 10 / 8 / 9 格，seed 24301 落在 9–14 格。**两边一样远**，所以
「把精炼厂建到矿边」这条路是死的，ore-aware 选址不要再试。

### 把清点门槛从 8 抬到 12（试过，更差）

`MOP_UP_OBJECTS`（`src/sim/strategy/assist/advance.rs`）同时管两件事：`mop_up()` 读
基地 rings 里能看到的敌人机动量，`mopping_up` 读全局敌人对象数。抬到 12 的动机是
seed 10 打到还剩 10 个敌人就不再清场（8 差两个）。

结果（Death.mmx，america，normal，36000 帧）：seed 24301 从 won 35276 变成 timeout
（enemy=15、army=83、lost 63:106）；seed 19 明显变强（army 66→92、own 126→150、
lost 52:43→43:52）但仍 timeout（enemy=67）；seed 10 更差（enemy 10→34）。

按基线判定已还原（复测 seed 24301 = won 35276，与基线一致）。两个判据本该是两个常量：
「基地门口的机动量」和「全局还剩几个目标」不是一回事，下次要试只动 `mopping_up` 那个。

### 把拳头锁在一个目标上 1800 帧（试过，更差）

`PUSH_LOCK_FRAMES` 一直是"有定义、没人引用"的常量。旧记录说锁一个任期在 1v1 上
把胜利拖成 timeout，原因是锁到了一栋走不到的建筑；`reachable_target`（8db1c08a8）
正好补上那个守卫——锁只在还有人走得到时保持。所以这轮试的是"锁 + 可达"这个新组合，
不是旧实验的重复。

实测（Death.mmx，america，normal，36000 帧）：seed 24301 与 seed 19 和基线逐字节一致
（won 35276 / timeout own=126 enemy=70 army=66）；seed 10 从
`enemy=10 lost=58:114 income=8415` 掉到 `enemy=60 lost=49:77 income=4163`
（own 147→161、army 82→93），两次运行完全一致。

结论：seed 10 恰恰是最依赖"每帧重选目标"的一局。锁住一栋建筑 30 秒，敌人就在这 30 秒里
把场子修回来，拳头最后绕着一个打不完的基地转——自己的单位越攒越多，敌人的建筑一栏反而
从 10 涨回 60。已还原（复测 seed 10 = own 147 / enemy 10 / army 82，与基线一致），
`PUSH_LOCK_FRAMES` 重新变成死常量；要再用它，先得解释 seed 10 为什么反过来变差。

### 打不到的目标会让全军站在家里（已定位，未修）

终局读数（seed 24301）：`push=push target=3844 at Some((62, 173))`、
`squads=[1, 4, 87]`、`idle=85 fighters=93`、`pts=12241/0`，而我们的基地在 (174,96)。
`AUTOPLAY_TRACE_REFUSE=1` 量到 187 次拒绝，**全部**是同一条
`goal=AttackMove { cell: (62, 173) }`，reason=unreachable。

机制：`orders/common.rs` 的 `reachable()` 是命令唯一的失败出口——目标在图上、能停、
但 zone map 不通（墙、悬崖、岛）。命令不下达，`out.blocked` 记一笔；下一帧
`avoidance::answer_blocked`（`src/sim/strategy/assist/avoidance.rs:764`）只把单位往旁边
挪一格，不会换一个目标。于是最后一个敌人站在我方地面部队走不到的地方时，87 人的
拳头每帧都被同一条理由拒绝，`idle` 一路涨到 85，直到 36000 帧超时。

试过并还原的错误修法：把 census 里已经死掉的目标滤掉
（`obs.health(..).is_some()`，两处：`structures` 与 `mopping_up` 的追加）。方向是对的
（179 次拒绝降到 7 次），但单独上这一条会让 seed 24301 从 won 35276 变成 timeout
（enemy 只剩 1，army 93、income 6379 反而都是三局最高），seed 19 从 126/70/66 涨到
154/52/87。所以尸体不是主因，可达性才是；按基线判定已还原。

下一轮该做的：选目标时先问「有没有人能走到」，而不是先选最近的再让引擎拒绝。

### 兵营/基地车那一对：第二基地确实从来没站起来过

`pack_captured_yard`（`src/sim/strategy/assist/orders.rs:536`）用「不在基地环里」
来判断哪个是战利品，但第二基地按定义就离主基地很远，于是它每次都被当成捕获的
庭院打包回主基地：日志里 `maker ... far=26 admits=true` 之后一个采样点就是
`yard GACNST at (129, 199) packs a`，三局基线全程 `yards=1`。把它改成「脚下的地面
不可建造（即敌方/非我方可建）才打包」之后，`yards=2` 确实出现了（seed 24301 与
seed 10），机制是通的。

但强度没变好：seed 24301 仍然赢（32537，几乎与 32491 同时），army 84 → 79、
income 8613 → 6165；seed 10 own 106 → 98、army 55 → 48、lost 64:89 → 71:89；
seed 19 完全不变（那局本来就没造出第二个 maker）。第二个基地要花 2500 起，而
36000 帧内没有回本。按基线判定：还原。要保留的话得先回答「什么时候第二个基地
能在本局内回本」，那不是这一轮能定的。

### 为什么基线必须是 36000 ticks

`PUSH_REST_FRAMES = 750`、census 采样每 600 帧一次，`autoplay_check` 的
`PROGRESS_FRAMES = 150`。更粗的采样会把两次门翻转混成一次，翻转次数就读不出来。

## 4. 已知坏的 / 停用的

| 项 | 状态 | 原因 |
|---|---|---|
| `WALL_BREAKER_PRIORITY` | 未接线，`#[allow(dead_code)]` | 拆墙规则没有调用点 |
| `MAIN_BATTLE_MAX_ENEMIES` | 未接线，`#[allow(dead_code)]` | 步兵下限规则没有调用点 |
| `SCOUTS_OUT` | 未接线，`#[allow(dead_code)]` | 侦察规则没有调用点 |

## 5. 结构（改代码前必读）

```
sim/behavior  L1 执行：目标词表、单一命令漏斗、托管微操、ready 门
sim/control   L2 控制：ControlBoard、Brain port、预算、Observation 投影、
                   感知、选址、外部契约、托管表
sim/strategy  L3 决策：assist 大脑 —— 默认
autoplay/     引擎外的 v2 策略 crate
```

依赖方向 L3 → L2 → L1，`src/architecture_guards.rs` 守着。L2 不指名具体大脑。

## 6. 残留（从代码头注摘录）

- **推进**只有一条规则：军队规模 → 最近敌方建筑。没有撤退、重整、波次时机。
- **绕行**（`avoidance`）是两段几何，不是寻路：不认识墙，两面火力之间没缝时会停在近的那把枪边缘。
- **没有 native `TeamClass`**：165 个 `AITriggerTypes` 到 tick 6000 没有一个产出完整队伍。
- **军队看整张图**：雾的诚实性仍是公开问题（`docs/autoplay.md`）。
- **目标选择**是一条谓词，不是 native 那条：交战国、最近优先、基地半径内。
- **没有基地时** `advance` 读不出威胁，于是推进；但它不会重建（`CanBuild` 可能拒绝 maker）。
