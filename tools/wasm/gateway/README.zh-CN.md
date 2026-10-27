# 浏览器联机网关

[English](README.md) | [简体中文](README.zh-CN.md)

这个可选服务将 `/game/primary` 和 `/game/alternate` 上的 WebSocket 二进制消息转发给固定的 UDP relay。每条消息对应一份未经修改的数据报，每个 WebSocket 都独占一个已连接的 UDP socket。网关不解释游戏报文、不签发凭据、不确认指令，也不实现重试。原大厅、relay 和桌面客户端均无需修改。

使用 Node.js 22 或更新版本。依赖仅安装在本目录内：

```sh
cd tools/wasm/gateway
npm ci
VERA_RELAY_HOST=relay.example.com VERA_RELAY_PORT=5000 \
  VERA_WEB_ORIGIN=https://game.example.com npm start
```

`VERA_RELAY_HOST` 必填，`VERA_RELAY_PORT` 默认为 5000。请配置与大厅下发的 `RelayGrant.address` 对应的 relay；网关不能使用另一台 relay 的凭据进行替代。浏览器不能选择上游目标。`VERA_LOBBY_URL` 默认为 `https://lobby.vera20k.org`，必须是 HTTPS origin。`/v1/*` 转发原有的请求方法、路径、Authorization 和请求体，不转换 JSON、不跟随重定向，也不重放请求，并保留上游的状态码和响应体。CORS 仅允许 `VERA_WEB_ORIGIN`、GET/POST 方法以及 Authorization/Content-Type 请求头。不会转发浏览器自动携带的 cookie。

浏览器的 `WS_PROXY_URL` 会编译进 `wasm-net` 构建，默认值为 `wss://wss.vera20k.com`。这是客户端配置，与服务端的 UDP 目标分别设置。请在该 origin 部署终止 TLS 的反向代理，将 `/game/*` 路由的 WebSocket Upgrade 连接以及 `/v1/*` HTTP 请求转发到本进程。保留浏览器的 Origin 请求头；WebSocket 空闲超时至少设为 70 秒。Node 服务默认绑定 `127.0.0.1:8788`（可通过 `HOST` 和 `PORT` 覆盖），不提供游戏静态文件。静态站点和网关可以分别托管。

本地开发可以使用 `VERA_WEB_ORIGIN=http://localhost:8000`。若大厅使用 HTTP，必须同时设置 `VERA_ALLOW_LOCAL_HTTP=1`，并使用回环地址的大厅 origin，例如 `VERA_LOBBY_URL=http://127.0.0.1:8790`。远程 HTTP 地址会被拒绝。此时浏览器也必须使用匹配的本地代理 origin `ws://localhost:8788` 进行编译。

所有游戏载荷仍保持加密，但 HTTP 代理会处理大厅 bearer 和包含密钥的响应。因此，网关属于受信任的大厅基础设施；不要记录请求头、请求体、响应体、票据或载荷密钥。网关不需要 relay 签名密钥，也不需要零售资源。WS Origin 检查只是现有 relay 身份认证的补充，不能认证任意非浏览器客户端。

传输限制集中定义在 `server.mjs` 中：最多 256 个 WebSocket、128 个待完成 HTTP 请求，HTTP 请求体上限 64 KiB、响应体上限 1 MiB，单条消息上限 2048 字节，UDP 发送队列上限 256 包 / 256 KiB，WS 待发送数据上限 1 MiB。每条连接每秒最多收发合计 2048 份数据报，同时受每秒合计 16384 份的全局限制。消息的分片数量和缓冲也有限制。溢出会关闭对应传输连接，恢复方式仍只由原游戏协议决定。HTTP 响应使用流背压，请求的绝对超时为 12 秒。关闭 socket 时会释放队列和 UDP 端口。每 30 秒一次的 WS Ping/Pong 心跳不会改变游戏准备状态、票据或 Leave 语义。

锁定的 `ws` 版本包含上游针对[分片内存耗尽](https://github.com/websockets/ws/security/advisories/GHSA-96hv-2xvq-fx4p)和[关闭原因内存泄露](https://github.com/websockets/ws/security/advisories/GHSA-58qx-3vcg-4xpx)的修复。实现依据 [ws 8.22.0 限制说明](https://github.com/websockets/ws/blob/8.22.0/doc/ws.md)和[已连接 UDP 的语义](https://nodejs.org/api/dgram.html#socketconnectport-address-callback)。

执行 `npm test` 运行真实 HTTP 和 WS/UDP 集成测试。测试覆盖字节和报文边界保持、独立且稳定的源端口及其释放、代理限制和准入失败路径。这些测试不能证明认证后的真实对局、浏览器与原生客户端的模拟状态收敛，或 30 人 / 20,000 单位的性能目标。
