// Physical datagrams only. No credentials, ACKs, game retries or protocol headers.
(() => {
  const sockets = new Map();
  const MAX_PACKETS = 256;
  const MAX_QUEUED_BYTES = 256 * 1024;
  const MAX_SEND_BYTES = 256 * 1024;
  let next = 1;

  function detach(state) {
    clearTimeout(state.connectTimer);
    state.connectTimer = undefined;
    const ws = state.ws;
    state.ws = null;
    if (ws) {
      ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
      ws.close();
    }
    state.packets.length = 0;
    state.bytes = 0;
  }

  function reconnect(state) {
    if (state.closed || state.retryTimer !== undefined) return;
    detach(state);
    state.routeChanged = true;
    state.retryTimer = setTimeout(() => {
      state.retryTimer = undefined;
      if (!state.closed) connect(state);
    }, state.retryDelay);
    state.retryDelay = Math.min(state.retryDelay * 2, 30000);
  }

  function connect(state) {
    let ws;
    try { ws = new WebSocket(state.url); }
    catch { reconnect(state); return; }
    state.ws = ws;
    ws.binaryType = 'arraybuffer';
    const current = () => !state.closed && state.ws === ws;
    state.connectTimer = setTimeout(() => {
      if (current()) reconnect(state);
    }, 10000);
    ws.onopen = () => {
      if (!current()) return;
      clearTimeout(state.connectTimer);
      state.connectTimer = undefined;
      state.routeChanged = true;
    };
    ws.onmessage = ({data}) => {
      if (!current()) return;
      if (!(data instanceof ArrayBuffer) || data.byteLength === 0
        || data.byteLength > state.maxDatagram || state.packets.length >= MAX_PACKETS
        || state.bytes + data.byteLength > MAX_QUEUED_BYTES) {
        reconnect(state);
        return;
      }
      state.retryDelay = 250;
      state.packets.push(new Uint8Array(data));
      state.bytes += data.byteLength;
    };
    ws.onclose = ws.onerror = () => {
      if (current()) reconnect(state);
    };
  }

  globalThis.veraSocketOpen = (origin, channel, maxDatagram) => {
    const url = new URL(origin);
    const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
    if (!['primary', 'alternate'].includes(channel)
      || !(url.protocol === 'wss:' || (url.protocol === 'ws:' && local))
      || url.username || url.password || url.pathname !== '/' || url.search || url.hash
      || !Number.isInteger(maxDatagram) || maxDatagram < 1 || maxDatagram > 2048) {
      throw new Error('Invalid compiled WebSocket proxy configuration');
    }
    url.pathname = `/game/${channel}`;
    const state = {
      url: url.href, maxDatagram, ws: null, packets: [], bytes: 0,
      retryTimer: undefined, connectTimer: undefined, retryDelay: 250,
      closed: false, routeChanged: false,
    };
    const id = next++;
    sockets.set(id, state);
    connect(state);
    return id;
  };

  globalThis.veraSocketSend = (id, bytes) => {
    const state = sockets.get(id);
    if (!state || state.closed || !bytes.length || bytes.length > state.maxDatagram) return 2;
    const ws = state.ws;
    if (!ws || ws.readyState !== WebSocket.OPEN
      || ws.bufferedAmount + bytes.length > MAX_SEND_BYTES) return 1;
    try { ws.send(bytes); return 0; }
    catch { reconnect(state); return 1; }
  };

  globalThis.veraSocketReceive = id => {
    const state = sockets.get(id);
    if (!state || state.closed) return undefined;
    const packet = state.packets.shift();
    if (!packet) return null;
    state.bytes -= packet.byteLength;
    return packet;
  };

  globalThis.veraSocketTakeRouteChange = id => {
    const state = sockets.get(id);
    if (!state) return false;
    const changed = state.routeChanged;
    state.routeChanged = false;
    return changed;
  };

  globalThis.veraSocketClose = id => {
    const state = sockets.get(id);
    if (!state) return;
    sockets.delete(id);
    state.closed = true;
    clearTimeout(state.retryTimer);
    detach(state);
  };
})();
