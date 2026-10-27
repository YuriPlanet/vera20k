# WASM 联机技术方案

目标是在现有浏览器单机版本上接入现有在线对战，保留原始网络协议，只增加 WS 与 UDP 的透明转换及其必要的浏览器平台适配。默认构建、桌面联网和仅启用 `wasm` 的单机版本保持原行为。

方案采用独立网关：浏览器继续运行现有大厅、认证、可靠消息、指令排序客户端和模拟代码；网关把完整二进制数据报原样转发给现有 UDP relay。原大厅和 relay 无需升级。浏览器与桌面互联是验收目标，需要实际验证，不能由协议相同直接推定模拟一致。

## 范围和约束

- 分支为 `wasm-net`，方案基于 `origin/main` 的 `02c2fc7b5`。本阶段只提交方案，不实施联网功能或部署服务。
- 不修改大厅协议 v4、relay wire v2、票据、证书、加密、ACK、重试、指令编码、排序、帧时钟及对局日志语义。不增加消息类型、字段或协议版本。
- WS 消息体就是原始 UDP 数据报；不加 JSON、base64、目的地址头或自定义握手包，不拆分或拼接应用数据报。WebSocket 自身的标准帧和控制帧不属于游戏协议。
- 浏览器专用实现必须同时满足 `feature = "wasm-net"`、`target_arch = "wasm32"`、`target_os = "unknown"`。不能只用运行时判断，也不能只检查 `target_arch`。
- 不为浏览器复制 `LanConnections`、`GlobalChannel`、`ClientSession`、`SequencedSession` 或模拟状态。共享协议逻辑保持单一实现。
- `/Users/libi/Documents/workspace/other/vera20k-web` 只作为只读设计参考，不整体移植补丁、构建副本或旧的单机兼容层。
- 第一条交付链是现有在线大厅中的普通已安装地图：进大厅、建房或加入、准备、加载、对战、短暂断线恢复、正常退出和再开一局。地图下载沿用现有机制。浏览器 LAN 广播发现、任意 IP 直连、CnCNet、WebRTC、WebTransport、随机地图生成和刷新页面后恢复整个对局均不在本轮新增范围内。

## 当前代码和接入点

| 当前所有者 | 已有能力 | 浏览器缺口 |
| --- | --- | --- |
| [`src/net/udp.rs`](../src/net/udp.rs) 的 `LanUdpTransport` / `UdpIo` | 双 socket、可靠消息、全局消息、relay 认证及续期、排序请求、退出确认 | WASM `bind` 返回 Unsupported；物理 I/O pump 使用线程 |
| [`http.rs`](../src/app/shell_wol/live_service/http.rs) 的 `Client` / `run` | bearer、事件 cursor、请求映射、重试、地图传输、match resume、退出时事件排空 | 使用阻塞 ureq、线程及 `recv_timeout` |
| [`live_service.rs`](../src/app/shell_wol/live_service.rs) 的 `LiveService` | 房间状态、版本和规则校验、relay admission、延迟探测、续期、清理 | WASM 构造直接报不支持 |
| [`lan_service/loading.rs`](../src/app/lan_service/loading.rs) 的 `LoadingNetworkService::milestone` | 加载进度及 ACK 服务，保留原 transport 和 session | 网络队列超过阈值时同步循环并 sleep 20 ms，不能在浏览器照搬 |
| [`loading/pump.rs`](../src/app/loading/pump.rs)、[`fresh_map_job.rs`](../src/app/loading/fresh_map_job.rs) | 加载阶段、可续执行的地图加载工作、显示和安装屏障 | `LoadingStartup::Network` 当前直接调用同步 `load_map_from_initial`，只有另一分支走分段 job |
| [`live_commands/sequenced.rs`](../src/net/live_commands/sequenced.rs) | 已提交帧消费、batch 去重、独立日志 cursor、断线后的重试和追赶 | 保留整个协议实现，传输恢复后继续使用 |
| [`services/relay`](../services/relay/README.md) | 原 UDP 接口、认证、固定时钟排序和磁盘日志 | 不修改，通过转换层接入 |
| [`services/lobby`](../services/lobby/protocol.md) | 原 HTTPS JSON 大厅 | 当前入口仅接受 GET/POST；浏览器跨源预检由网关解决，不修改大厅 |

现有在线模式不仅是两端互等的 LAN 锁步：relay 已拥有认证后的固定帧时钟和指令日志。转换层必须保持其现有所有权，不能重新实现房主排序或浏览器专用帧同步。

## 网络结构

```text
浏览器现有大厅逻辑 → Fetch → 网关 /v1/* → 原 HTTPS 大厅

浏览器现有网络协议 → 浏览器 socket 适配 → WSS /game/primary
                                           ↓ 原样转换
                                     独立 UDP socket → 原 UDP relay

浏览器 alternate 通道 → WSS /game/alternate → 另一独立 UDP socket

桌面客户端 → 原 HTTPS 大厅 / 原 UDP relay（路径保持不变）
```

两条 WS 路由各绑定一个 UDP 源端口，同一连接寿命内保持稳定。不同浏览器、标签页、通道不得共用源端口，否则会混淆 relay 的地址挑战和路由身份。所有连接转发到管理员配置的固定 relay，不接受浏览器指定任意 UDP 目的地。

浏览器 WebSocket API 不提供原始 UDP socket，所以需要服务器转换；连接是否 OPEN 只表示 WS 建立，不能代替原协议的 relay challenge/response。参见 [WebSockets Standard](https://websockets.spec.whatwg.org/)。

### 数据报转换

浏览器调用原 `send_to` 时，先由现有 Rust 所有者完成原协议编码、校验和及加密，适配器只把得到的字节交给 WS。网关收到一条完整 binary message 后只进行一次 UDP send。UDP 回包对应一次 WS binary send，Rust 收到后继续走现有解码和认证。

不在网关解析或重写票据、签发凭证、生成 ACK、重发游戏消息或保存指令日志。WS 的 Ping/Pong 仅维护连接；它不能更新游戏层 relay readiness，也不能延长原票据有效期。

使用现有 `vera20k_relay_protocol::MAX_DATAGRAM` 验证 Rust 边界。网关可采用明确的传输上限，但必须覆盖当前所有原协议数据报；上限不是新的协议字段。若使用参考实现的 2048 字节上限，须以原 codec 的最大报文回归测试证明其覆盖范围，不能把 UDP 内层的 640 字节容量误用为加密外层容量。

### 独立网关

建议新增 `tools/wasm/gateway/`，使用 Node.js 的 `node:dgram` 和 `ws`，依赖仅存在于该目录的 package manifest 和 lockfile。它不进入 Rust workspace，也不进入桌面打包、默认 CI 或单机运行依赖。实现时锁定兼容版本，不升级现有 Rust 依赖。

网关负责固定上游 UDP 转发，以及可选的固定上游 HTTPS 反向代理。静态游戏文件可继续独立部署；网关不需要读取零售资源。公网由 HTTPS 反向代理终止 TLS，WS Upgrade 路由到网关；localhost 开发可使用 HTTP/WS。

网关使用 connected UDP socket，限制回包源为配置的 relay；关闭 WS 时释放对应 UDP socket。Node 的 connected datagram 接口见 [Node.js dgram 文档](https://nodejs.org/api/dgram.html#socketconnectport-address-callback)，WS 服务端选项见 [ws 官方文档](https://github.com/websockets/ws/blob/master/doc/ws.md)。

### 大厅 HTTPS 的兼容边界

Fetch 仍发送现有路径、HTTP 方法、Authorization、JSON 请求和 request_id，接收原状态码和响应体。`/v1/*` 代理不反序列化和改写游戏 JSON，不修改 `RelayGrant.address`，不增加 Web 客户端能力字段。

网关入口由编译期 `WS_PROXY_URL` 固定，不从静态页面域名推导。现有方案的 `/v1/*` HTTPS 代理使用同一网关 authority，将 `wss:` 映射为 `https:`（本地开发 `ws:` 映射为 `http:`）。跨源部署时，仅网关处理指定 Origin 的 OPTIONS，允许现有 GET/POST 及 Authorization/Content-Type；原大厅代码不变。HTTP 代理不得跟随到未知上游的重定向，保留请求和响应大小限制。Fetch 配置 `credentials: omit`、`redirect: error`、`cache: no-store`，沿用现有请求超时和错误映射。依据 [Fetch CORS 规范](https://fetch.spec.whatwg.org/#http-cors-protocol)。

WS 侧只接触原加密报文，不需要任何密钥。但同一网关若终止或转发大厅 HTTPS，会经手 bearer 和包含 payload key 的响应，属于大厅凭据的信任边界；不能宣称整台网关看不到这些秘密。不得记录 Authorization、响应体、票据或 payload key。

## 编译隔离

拟新增特性：

```toml
# 根 Cargo.toml
wasm-net = ["wasm", "vera20k-core/wasm-net"]

# src/Cargo.toml
wasm-net = ["wasm", "dep:wasm-bindgen"]
```

core 的 `wasm-bindgen` 依赖放入已有的 `wasm32-unknown-unknown` target dependency 表并设为 optional，使用现有 lockfile 的版本。浏览器 HTTP 继续使用根 crate 已有的 `wasm-bindgen-futures` 等能力；若需要额外绑定，只在浏览器 target 和可选 feature 下加入。

所有新平台模块、构造分支和调度差异使用：

```rust
#[cfg(all(feature = "wasm-net", target_arch = "wasm32", target_os = "unknown"))]
```

| 构建 | 预期行为 |
| --- | --- |
| 桌面默认 / shipping | 原生 UDP、ureq 和原线程调度 |
| 桌面启用 `wasm` | 仍使用原生实现 |
| 桌面启用 `wasm-net` | 仍使用原生实现，不引用 JS 函数 |
| WASM 仅启用 `wasm` | 保持现有单机版，联网仍明确不支持 |
| WASM 启用 `wasm-net` | 启用 WS 数据报适配和 Fetch 大厅访问 |
| WASM 未启用 `wasm` | 保留现有编译错误 |

`tools/wasm/build.py` 拟增加 `--network`，默认命令保持单机；只有显式选项才使用 `--features wasm-net` 并打包联网 JS。以下为实施后的预期命令，目前尚未提供该参数：

```sh
python3 tools/wasm/build.py
python3 tools/wasm/build.py --network
```

### 编译期代理地址

新增构建环境变量 `WS_PROXY_URL`，默认域名按用户指定为 `wss.vera20k.com`，完整默认值为 `wss://wss.vera20k.com`。它表示代理 origin，不含通道路由。默认连接地址为：

- 主通道：`wss://wss.vera20k.com/game/primary`。
- 辅通道：`wss://wss.vera20k.com/game/alternate`。

只有编译 `wasm-net` 浏览器版本时读取并嵌入该值。浏览器模块使用 Rust 的 `option_env!("WS_PROXY_URL")` 获取编译环境，未设置时采用完整默认值；由同一配置 owner 将该 origin 传给 JS 桥，JS 不另设默认地址。构建入口验证自定义值为完整 `wss://` origin，拒绝空值、用户信息、路径、查询串和片段；本地调试允许回环地址的 `ws://`。变量变化必须触发重新编译并更新产物，不能复用旧地址的 WASM。

实施后的构建示例：

```sh
# 未设置 WS_PROXY_URL 时使用 wss://wss.vera20k.com
python3 tools/wasm/build.py --network

# 自定义代理，需要重新编译
WS_PROXY_URL=wss://gateway.example.com python3 tools/wasm/build.py --network

# 本地开发
WS_PROXY_URL=ws://localhost:8788 python3 tools/wasm/build.py --network
```

不增加运行时网关设置、URL 参数、localStorage 或 `location.host` 回退。`WS_PROXY_URL` 只决定浏览器连接哪台代理，不修改大厅下发的 `RelayGrant.address` 或 UDP 协议。默认域名是构建约定，不表示本任务已部署该服务。桌面构建以及仅启用 `wasm` 的单机包不读取或使用此配置。

打包必须按模式选择文件，避免在同一输出目录中把上一次联网构建的 JS 或配置留给单机构建。Cargo feature 会合并，因此必须同时检查 target，参见 [Cargo Features](https://doc.rust-lang.org/cargo/reference/features.html)。

## 客户端最小改动

### socket 边界

在 `src/net/udp.rs` 的私有 socket 导入处选择原 `std::net::UdpSocket` 或新增的浏览器 adapter，保留 `LanUdpTransport` 对上层的接口及全部协议所有者。新增文件建议为 `src/net/web_socket.rs`，只实现当前调用需要的有限 socket 接口，不创建全项目通用 Transport trait，也不改名迁移整个 net 模块。

- `bind` 创建对应 WS 通道，允许处于连接中状态；第二通道创建失败时清理第一通道。
- `send_to` 在连接未建立或发送缓冲达到限制时返回 WouldBlock；成功只表示浏览器已接受发送，不能伪装成协议 ACK。
- `recv_from` 只消费完整消息；无数据为 WouldBlock，连接断开与畸形报文使用明确的现有错误通道。
- 收包端点固定对应当前 relay 路由。不得像参考实现一样无条件把“最后一次 send_to 的目标”当成可信回包来源；限制适配器为已配置的 relay 通道，任意 direct-LAN 目标应明确拒绝。
- 逻辑 `local_addr` 只满足现有调用约定，不能把它当浏览器真实可达 IP，也不用于向大厅宣告新身份。
- 浏览器不启动原生 I/O 线程。由现有 app 轮询入口分次调用同一个 `service`，网络 callback 仅投递字节，不直接进入模拟或房间 reducer。
- `WouldBlock` 的浏览器连接中语义只在浏览器分支接入。不得照抄参考补丁中对 `record_io_error` 的无条件全平台改动。

### 缓冲和恢复

转换层只保存有限的未消费原始报文，没有自己的 ACK、重传时间线或游戏 batch。分别限制接收条数、接收总字节、WS bufferedAmount 和网关 UDP 发送积压，超过限制就产生可诊断的传输失败或明确的丢包策略，不能无限增长。

初始建议沿用参考实现的量级作为待测配置：浏览器接收 256 条且加总字节上限，发送积压约 256 KiB，网关 WS 发送积压约 1 MiB。这些只是传输资源限额，需用真实 2 人和多人流量验证，不能借限额改变游戏协议。浏览器 send 的 bufferedAmount 语义见 [WebSocket 标准](https://websockets.spec.whatwg.org/#dom-websocket-bufferedamount)。

WS 临时断开时，浏览器 adapter 可以有界退避重连物理连接；重连不复用旧 callback 的队列，不把陈旧 WS 数据报重新发送。原所有者保留对局身份、可靠队列、未确认 batch 和日志 cursor，并通过原 probe/challenge 重新确认路径。地址变化后如何及时取消 readiness，必须通过既有 `ClientSession` 接口触发，不修改认证算法。票据续期继续由原大厅 owner 负责。

WS close、后台挂起和网络丢失都不能制造游戏 Leave。玩家明确退出仍由原 owner 发送 Leave，并在确认后释放 transport。关闭整个页面不能保证异步退出完成，应按原协议网络失联处理；刷新恢复整个模拟状态不在本轮承诺中。

### 大厅请求执行器

在 `LiveService` 构造点选择原生 worker 或浏览器异步 runner。继续使用现有 `Request` / `Response` 和 `apply_response`、`apply_event`，不复制整套大厅逻辑到 JS。

`http.rs::Client` 不只是 HTTP 包装，它还拥有 token、cursor、active_match、批次去重、丢 cursor 恢复和 finish/leave 的排空规则。实施时只对这些既有规则做最小的共享提取：同步的请求准备、响应处理和下一步动作仍由一个 owner 实现；原生 runner 保持阻塞 ureq 和串行执行，浏览器 runner 用 Fetch 驱动同一动作。新增异步调度只在 `wasm-net` 浏览器分支编译。不把整个原生 worker 改为 async/block_on，也不复制一个长期分叉的 `Client`。

主控制通道最多一个请求在途，保持现有顺序、request_id 重试和轮询频率；翻译沿用独立执行安排，不阻塞房间命令。地图仍走现有分块、摘要验证和文件系统 owner。离开大厅时取消在途 Fetch，丢弃旧 visit 的回调，释放 response sender，保留原清理顺序。网关不可替客户端重试 POST，以免引入第二个重试所有者。

这是最需要控制 diff 的共享接点。先用现有回归测试固定行为，再提取必要函数；若实现需要大范围改写，则收窄拆分方案，不以“浏览器兼容”为由放宽原生语义。

### 在线加载让出事件循环

现有 `LoadingNetworkService::milestone` 在队列超过 5 时最多等待 240 wall buckets，并调用 `std::thread::sleep(20 ms)`。这条路径在单机下不触发，联网后会触达。只把 socket 换成 WS 无法解决：JS 回调不能在同一线程同步等待期间交付 ACK。

`loading/pump.rs` 目前显式将 `LoadingStartup::Network` 送入同步 `load_map_from_initial`，其他启动才使用 `FreshMapLoadJob`。因此需要在浏览器联网编译分支选择已有分段 job，并保留待完成的 milestone，在安全的加载工作边界返回事件循环，然后用同一个 session、transport 和进度 owner 继续 pump。在线必需的初始状态、阶段顺序和终态 100% 屏障仍须完整经过。不能用“直接返回 Ok”删掉等待，也不能先安装模拟后补做启动认证。

浏览器分支需要补上加载阶段的持续大厅轮询和票据续期，保证每个必要网络等待都有未来唤醒点。发现单个同步阶段长到错过网络服务时，只在安全的原有操作边界增加 continuation，不改变模拟的初始化顺序和 RNG 消费。桌面 sleep、线程 pump 和加载路径保持原样。

### 后台页面和实际性能

浏览器可能节流后台计时器及暂停 requestAnimationFrame，不能假设 6 ms 的原生 I/O 频率在浏览器成立。恢复可见时继续原日志 cursor，分预算追赶，避免一次补帧长期占满主线程。调度不得跳过已提交帧或注入空命令来“赶时间”。参见 [Page Visibility API](https://developer.mozilla.org/en-US/docs/Web/API/Page_Visibility_API#policies_in_place_to_aid_background_page_performance)。

WS 的可靠有序传输可能增加丢包时的队头阻塞，这是转换方案的性能取舍。保留原 ACK 和可靠消息层，即使它与 TCP 的可靠性有重叠，也不能为提速删除。首轮不引入 Worker、SharedArrayBuffer 或更换网络协议；是否需要进一步隔离物理 I/O，只由实测卡顿决定。

## 参考实现取舍

| 参考内容 | 处理方式 |
| --- | --- |
| `gateway/server.mjs` 的双路 WS / 独立 UDP socket | 借鉴传输结构，重新对照当前协议边界与资源限制 |
| `templates/network.js`、`overlays/src/net/web_socket.rs` | 借鉴 binary message 和同步非阻塞接口，不直接照搬目的地址追踪及队列溢出策略 |
| `templates/lobby.js`、HTTP browser adapter | 借鉴有界 Fetch、取消、无 cookie、无重定向；协议状态继续留在 Rust 原 owner |
| 参考项目直接把网络加入 `wasm` feature | 不采用；本项目增加独立默认关闭的 `wasm-net` |
| `network-compat.patch` 中房间行号匹配、ready/map_status 字段构造修改 | 不纳入转换层；若在当前基线复现，单独记录，不能偷偷移植 |
| 原生 HTTP 全改 async 和 `record_io_error` 全平台行为变化 | 不采用；只允许必要共享提取和浏览器受控分支 |
| 旧 `web-compat.patch`、渲染、资源加载和存储 overlays | 不导入；当前仓库已有单机 WASM owner |

## 文件变更预算

下面是实施范围，不表示本方案阶段已修改这些文件。

| 文件或目录 | 允许的变更 |
| --- | --- |
| 根及 core `Cargo.toml`、`Cargo.lock` | 新 feature、可选浏览器依赖与锁文件必要记录，保持默认项和版本 |
| `src/net/udp.rs` | 私有 socket 选择、浏览器 bind/pump/物理恢复接口，原协议主体保持不变 |
| 新 `src/net/web_socket.rs` | 浏览器原始数据报 adapter |
| `src/app/shell_wol/live_service.rs` | 构造与调度边界，不改房间规则、消息内容、启动和清理规则 |
| `src/app/shell_wol/live_service/http.rs` 及其新 browser 子模块 | 共享规则最小提取和 Fetch 执行器，保持原生执行方式 |
| `src/app/lan_service/loading.rs`、`src/app/loading/pump.rs`，必要时已有加载 job | 浏览器在线分段与 pending 网络等待，不复制 loader |
| `src/web.rs`、`tools/wasm/host.js` | 仅联网构建的编译期代理地址和 JS 接线，不增加运行时地址设置或新大厅 UI |
| `tools/wasm/build.py`、WASM workflow、相邻测试与文档 | 可选联网打包、`WS_PROXY_URL` 构建输入和定向检查；默认桌面 workflow 不变 |
| 新 `tools/wasm/network.js`、`lobby.js`、`gateway/` | 传输桥、HTTP 字节访问和独立部署服务 |

默认零改动区域：`services/relay-protocol/`、`services/relay/`、`services/lobby/`、模拟算法、规则解析、指令 codec、可靠队列和排序状态机。发现其中既有缺陷先记录和报告，不能借本任务扩大修改范围或通过网关绕过。

## 实施顺序和验收

各步骤在同一分支形成可检查增量，不并行堆叠未合并实现分支。

1. **确认透明转换。** 增加独立网关和传输测试，用 UDP echo 验证二进制数据报的边界、长度和逐字节相等；验证两个浏览器、双通道均隔离源端口。原服务不参与修改。
2. **接入浏览器 socket。** 增加 feature、私有 adapter 和 JS 桥；跑原 relay codec 的真实 challenge/response、续期与 ordering 收发，验证 WS OPEN 不能越过原 admission。
3. **接通大厅。** 以原请求/响应类型驱动 Fetch，实现登录、建房/加入、选图、准备、版本/地图/规则一致性检查以及地图下载。验证旧 visit 回调不能污染新房间。
4. **打通在线加载。** 先复现同步等待问题，再增加浏览器 continuation；用一快一慢两个客户端验证加载进度、原启动屏障和票据续期。
5. **真实比赛与恢复。** 浏览器对浏览器、浏览器对未修改的桌面客户端，各走建房→准备→加载→生产/移动/攻击→断线恢复→退出→再开局。比较相同已提交帧的状态摘要和必要 RNG 计数，不比较两个客户端不同墙钟时刻的状态。
6. **隔离回归和交付。** 执行下面构建矩阵及受影响模块测试，检查禁改区域零 diff、双门控、残留文件和原生行为。测试通过后才进入实现 PR；本次方案不创建 PR 或发布服务。

### 定向测试

- 编译配置：未设置变量时使用默认域名，自定义地址进入产物，非法值构建失败；连续使用两个地址构建时不能残留旧值；桌面和 WASM 单机包均不启用该地址或联网 JS。
- 网关：真实 WS↔UDP round trip 保持任意二进制字节及报文边界；多连接源端口隔离；关闭释放端口；拒绝错误 Origin、文本、超长消息和任意转发目的地；缓冲压力可控。测试使用实际生产入口，不仅 mock。
- socket：Connecting/OPEN/Closed、WouldBlock、缓冲溢出、断线重连后的旧消息隔离；原认证通过前 readiness 不变。使用少量边界用例，避免维护另一套协议实现。
- HTTP：现有 batch/cursor、resume、命令幂等与 finish/leave/logout 测试继续通过；新增浏览器取消、响应超限和回调生命周期测试。请求参数与状态语义由同一生产逻辑生成。
- 加载：ACK 延迟时事件循环仍有进展；终态加载和启动认证未完成时不得进入第一模拟帧；对局退出后网络所有者完整清理。
- 确定性：相同构建能力、零售数据、地图、seed 和指令流，比较 native 与 WASM 的实际开局及运行摘要。不能仅用 headless `parity-digest` 取代实际多人开局，因为该工具自身注明没有完整 skirmish session。若出现差异，定位并报告，不修改协议、放宽摘要检查或静默调整模拟来掩盖。

Rust 定向测试通过 `python3 -m tools.cargo_run -- test -p <crate> --lib <filter>` 执行，按实际变更选择 net、HTTP、加载及直接消费者；零售相关验证设置 `VERA20K_REQUIRE_RETAIL_INI=1`。加载改动还需 `--profile fast` 的真实零售地图加载。每个具体修复保留 1–3 个有意义的回归用例，不跑无关全套 Rust 测试。修改 Python 打包工具后按项目要求执行 `python -m tools.run_tests`；JS 测试使用 Node 原生测试入口。

构建矩阵必须覆盖 native 默认、native `wasm`、native `wasm-net`、WASM `wasm` 和 WASM `wasm-net`。有计划的测试已经覆盖编译时，不重复 cargo check。平台 workflow 不自动派发；各平台验证范围如实列出。

### 交付条件和明确限制

交付需要同时满足：原服务与协议零变更、原生和单机入口保持行为、联网构建显式可选、双客户端完整流程通过、临时断线恢复与正常退局通过、必要的 native/WASM 确定性验证通过。只通过构建、mock 或 UDP echo 不算完成。

协议支持的 30 个 source 不等于浏览器已经达到 30 人和 20,000 单位性能目标。本轮先以真实双客户端证明功能与兼容性，再做代表性多人及高单位数压力测试，报告主线程阻塞、帧耗时、延迟和网关内存。不能用改变帧时钟、指令内容或游戏规则达成性能指标。

保留原 relay 的部署限制，包括现有日志故障处理及不支持进程重启恢复的边界。WS 网关重连不是 relay 重启恢复，也不是页面刷新后的模拟恢复。若既有平台或游戏行为问题阻碍本链路，给出复现、影响及需要的独立工作，不将其暗中并入转换层。
