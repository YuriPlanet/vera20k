# 浏览器联机网关

[English](README.md) | [简体中文](README.zh-CN.md)

这个可选服务将 `/game/primary` 和 `/game/alternate` 上的 WebSocket 二进制消息转发给浏览器所持票据指定的 UDP relay，前提是该 relay 在其列表中。每条消息对应一份未经修改的数据报，每个 WebSocket 都独占一个已连接的 UDP socket。网关不解释游戏报文、不签发凭据、不确认指令，也不实现重试。原大厅、relay 和桌面客户端均无需修改。

使用 Node.js 22 或更新版本。依赖仅安装在本目录内：

```sh
cd tools/wasm/gateway
npm ci
VERA_RELAYS=203.0.113.1:5000,198.51.100.2:5000 \
  VERA_WEB_ORIGIN=https://game.example.com npm start
```

`VERA_RELAYS` 必填：大厅下发的每个 relay 地址，以逗号分隔的 IPv4:端口 表示。每个游戏 WebSocket 在路径中写明其票据对应的 relay（`/game/primary/203.0.113.1:5000`），只有该 relay 在列表中时网关才转发到那里；其他路径一律以 403 拒绝，因此浏览器无法访问其他目标。大厅新增 relay 时须在同一次部署中把它加入列表，否则该 relay 上的浏览器玩家无法通过 relay 准入。早于 relay 路径的浏览器包只打开 `/game/primary` 和 `/game/alternate`。若要让升级前已打开的页面继续对局，把 `VERA_LEGACY_RELAY` 设为这些包所对应的那一个已列出的 relay，网关就把这两个路径转发到那里；不设置时返回 403。确定不会再有这类页面打开后删掉它。`VERA_LOBBY_URL` 默认为 `https://lobby.vera20k.org`，必须是 HTTPS origin。`/v1/*` 转发原有的请求方法、路径、Authorization、游戏登录时发送的请求头（`Vera-Version`、`Vera-Build`、`Vera-Quick-Match`、`Vera-Host-Handover`、`Vera-Watch-AI`、`Vera-Language` 和 `Vera-Changes`）以及请求体，不转换 JSON、不跟随重定向，也不重放请求，并保留上游的状态码和响应体。`VERA_WEB_ORIGIN` 必填：提供游戏页面的每个站点的 origin，以逗号分隔（`https://game.example.com,https://www.example.org`），须与浏览器发送的 Origin 完全一致，因此 `www.` 和端口都算在内。CORS 仅允许这些 origin（响应中回写请求自身的 origin）、GET/POST 方法以及上述请求头和 Content-Type。不会转发浏览器自动携带的 cookie。网关版本不得早于它所服务的浏览器包：较旧的网关会在 CORS 检查时拒绝较新浏览器包的登录请求头，导致登录失败。


大厅按请求来源地址实施按网络的限制，而代理请求的来源地址是网关自身。若要让每个浏览器玩家使用各自的限制，请把 `VERA_GATEWAY_KEY` 设为大厅 Worker 的 `GATEWAY_KEY` 密钥：一个至少 32 个字符的随机字符串（`openssl rand -hex 32`）。网关随后会用[大厅协议](../../../docs/lobby-protocol.md)定义的请求头注明每个玩家的地址（socket 对端地址，或 `VERA_CLIENT_IP_HEADER` 指定的请求头）。未设置时，所有浏览器玩家共享网关的地址及其限制。

浏览器的 `WS_PROXY_URL` 会编译进 `wasm-net` 构建，默认值为 `wss://wss.vera20k.com`。外部启动可以用 `spawn.ini` 的 `[Relay] GatewayUrl` 为这一局单独指定网关。无论哪种方式，relay 都来自这一局的票据，因此该网关必须列出它。请在该 origin 部署终止 TLS 的反向代理，将 `/game/*` 路由的 WebSocket Upgrade 连接以及 `/v1/*` HTTP 请求转发到本进程。把 `VERA_CLIENT_IP_HEADER` 设为该代理写入浏览器地址的请求头（例如 `cf-connecting-ip`；`x-forwarded-for` 取最后一项）；客户端能直连网关时不要设置该项。只要代理不是经回环地址连接网关（例如位于其他主机或容器中），就必须设置：否则所有玩家都按代理所在的网络计入下文的限制。保留浏览器的 Origin 请求头；WebSocket 空闲超时至少设为 70 秒。Node 服务默认绑定 `127.0.0.1:8788`（可通过 `HOST` 和 `PORT` 覆盖），不提供游戏静态文件。静态站点和网关可以分别托管。

局域网信令默认关闭。设置 `VERA_LAN_SIGNAL=1` 后会为 `wasm-lan` 构建启用 `/lan/signal`：按公网地址分组、分配虚拟 IPv4，并在组内转发 WebRTC 描述和候选地址；游戏数据报不经过网关。部署在反向代理后时，把 `/lan/*` 的 WebSocket Upgrade 转发到本进程；分组使用 `VERA_CLIENT_IP_HEADER` 给出的地址。`VERA_LAN_STUN` 可选，填写逗号分隔的 `stun:` 地址，用于隐藏 host candidate 的网络；TURN 会被拒绝。限制定义在 `signal.mjs`：256 个连接、每组 16 人、单条消息 16 KiB、每连接每秒最多接收 64 条消息或 Ping。JSON、Ping 和 Pong 写入都会检查待发送队列（含帧头预留）：每接收端最多 256 KiB，所有信令连接合计最多 1 MiB。写入会超过任一上限时，按正常成员清理流程移除对应接收端；欢迎消息被拒绝的连接不会发布加入通知。离开的浏览器的虚拟地址会保留两分钟，直到其他成员已不再使用它；只有请求取回该地址的信令重连才能获得它。

本地开发可以使用 `VERA_WEB_ORIGIN=http://localhost:8000`。若大厅使用 HTTP，必须同时设置 `VERA_ALLOW_LOCAL_HTTP=1`，并使用回环地址的大厅 origin，例如 `VERA_LOBBY_URL=http://127.0.0.1:8790`。远程 HTTP 地址会被拒绝。此时浏览器也必须使用匹配的本地代理 origin `ws://localhost:8788` 进行编译。

游戏数据报按每一跳认证，但不加密：网关以明文转发玩家的指令，其 HTTP 代理还会处理大厅 bearer 以及包含票据和 packet key 的响应。因此，网关属于受信任的大厅基础设施；不要记录请求头、请求体、响应体、票据、密钥或转发的数据报。网关不需要 relay 签名密钥，也不需要零售资源。WS Origin 检查只是现有 relay 身份认证的补充，不能认证任意非浏览器客户端。

传输限制集中定义在 `server.mjs` 中：最多 256 个 WebSocket、128 个待完成 HTTP 请求，HTTP 请求体上限 64 KiB、响应体上限 1 MiB，单条消息上限 2048 字节，UDP 发送队列上限 256 包 / 256 KiB，WS 待发送数据上限 1 MiB。一个网络（一个 IPv4 地址，或某 IPv6 地址所在的 /48，与大厅划分份额的方式相同）最多占用 WebSocket 和 HTTP 请求各约十分之一：24 个 WebSocket（每个玩家打开两个，即 12 名玩家）和 12 个 HTTP 请求；超出时 WebSocket 升级返回 403，HTTP 请求返回 503。设置了 `VERA_CLIENT_IP_HEADER` 时，缺少该请求头的请求会被拒绝（403）；未设置时，回环地址的对端（经回环地址连接的代理或本地测试）不计入。每条连接每秒最多收发合计 2048 份数据报，所有连接按最大最小公平原则共享每秒 16384 份的全局预算：每个一秒窗口为每条连接预留其上一窗口尝试发送的数量（不超过均分份额），超出预留的流量只能使用未被预留的预算。超出预算或遇到已满队列的数据报会像 UDP 丢包一样被丢弃，连接保持打开，恢复方式仍只由原游戏协议决定。文本、空消息或超长消息会关闭对应传输连接。消息的分片数量和缓冲也有限制。HTTP 响应使用流背压，请求的绝对超时为 12 秒。关闭 socket 时会释放队列和 UDP 端口。

对客户端 Ping 的 Pong 回复使用单独的公平预算：每连接每秒最多 64 帧，所有游戏连接共享每秒 1024 帧；超出时按正常清理流程关闭对应传输连接。Pong 和网关自身的心跳 Ping 都必须满足含帧头预留的 1 MiB WS 待发送队列上限，否则关闭该慢速连接。心跳不计入共享预算，因此其他连接无法耗尽安静玩家的心跳额度；控制帧也不占用游戏数据报额度。每 30 秒一次的 WS Ping/Pong 心跳不会改变游戏准备状态、票据或 Leave 语义。

锁定的 `ws` 版本包含上游针对[分片内存耗尽](https://github.com/websockets/ws/security/advisories/GHSA-96hv-2xvq-fx4p)和[关闭原因内存泄露](https://github.com/websockets/ws/security/advisories/GHSA-58qx-3vcg-4xpx)的修复。实现依据 [ws 8.22.0 限制说明](https://github.com/websockets/ws/blob/8.22.0/doc/ws.md)和[已连接 UDP 的语义](https://nodejs.org/api/dgram.html#socketconnectport-address-callback)。

执行 `npm test` 运行真实 HTTP 和 WS/UDP 集成测试。测试覆盖字节和报文边界保持、独立且稳定的源端口及其释放、代理限制和准入失败路径。这些测试不能证明认证后的真实对局、浏览器与原生客户端的模拟状态收敛，或 30 人 / 20,000 单位的性能目标。
