// Physical datagrams only. No credentials, ACKs, game retries or protocol headers.
(() => {
  const sockets = new Map();
  const MAX_PACKETS = 256;
  const MAX_QUEUED_BYTES = 256 * 1024;
  const MAX_SEND_BYTES = 256 * 1024;
  // The relay drops a hop silent for 1.5 s, so a match never waits longer than this to retry.
  const MAX_RETRY_DELAY = 2000;
  let next = 1;

  // A dedicated Worker owns the WebSockets where the browser allows one, so a
  // datagram's arrival is timed while a long frame blocks this thread
  // (docs/wasm-net.md). It moves bytes only; every rule below stays here.
  function workerMain() {
    const sockets = new Map();
    const report = (id, state) => {
      clearTimeout(state.drain);
      state.drain = undefined;
      postMessage({op: 'buffered', id, sent: state.sent, buffered: state.ws.bufferedAmount});
      if (state.ws.bufferedAmount > 0) state.drain = setTimeout(() => report(id, state), 50);
    };
    const retire = id => {
      const state = sockets.get(id);
      if (!state) return;
      sockets.delete(id);
      clearTimeout(state.drain);
      state.ws.onopen = state.ws.onmessage = state.ws.onclose = state.ws.onerror = null;
      state.ws.close();
    };
    onmessage = ({data: request}) => {
      const {op, id} = request;
      if (op === 'open') {
        let ws;
        try { ws = new WebSocket(request.url); }
        catch { postMessage({op: 'close', id}); return; }
        ws.binaryType = 'arraybuffer';
        sockets.set(id, {ws, sent: 0, drain: undefined});
        ws.onopen = () => postMessage({op: 'open', id});
        ws.onmessage = ({data}) => {
          const at = performance.timeOrigin + performance.now();
          postMessage({op: 'message', id, data, at}, data instanceof ArrayBuffer ? [data] : []);
        };
        ws.onclose = ws.onerror = () => { retire(id); postMessage({op: 'close', id}); };
      } else if (op === 'send') {
        const state = sockets.get(id);
        if (!state) return;
        try { state.ws.send(request.bytes); }
        catch { retire(id); postMessage({op: 'close', id}); return; }
        state.sent += request.bytes.byteLength;
        report(id, state);
      } else if (op === 'close') {
        retire(id);
      }
    };
  }

  // WebSocket's interface over the Worker. bufferedAmount adds the bytes
  // posted since the Worker's last report to the amount it reported.
  const remotes = new Map();
  let worker, nextRemote = 1;
  class RemoteSocket {
    constructor(url) {
      this.id = nextRemote++;
      this.readyState = 0;
      this.posted = this.reported = this.buffered = 0;
      remotes.set(this.id, this);
      worker.postMessage({op: 'open', id: this.id, url});
    }
    get bufferedAmount() { return this.buffered + this.posted - this.reported; }
    send(bytes) {
      this.posted += bytes.byteLength;
      worker.postMessage({op: 'send', id: this.id, bytes: bytes.buffer}, [bytes.buffer]);
    }
    close() {
      if (!remotes.delete(this.id)) return;
      this.readyState = 3;
      worker.postMessage({op: 'close', id: this.id});
    }
  }

  function startWorker() {
    if (worker !== undefined) return worker;
    worker = null;
    if (typeof Worker !== 'function') return worker;
    try {
      const source = URL.createObjectURL(new Blob([`(${workerMain})();`], {type: 'text/javascript'}));
      worker = new Worker(source);
    } catch { return worker; }
    worker.onmessage = ({data: event}) => {
      const socket = remotes.get(event.id);
      if (!socket) return;
      if (event.op === 'open') {
        socket.readyState = 1;
        socket.onopen?.();
      } else if (event.op === 'message') {
        socket.onmessage?.({data: event.data, at: event.at - performance.timeOrigin});
      } else if (event.op === 'buffered') {
        socket.reported = event.sent;
        socket.buffered = event.buffered;
      } else if (event.op === 'close') {
        remotes.delete(socket.id);
        socket.readyState = 3;
        socket.onclose?.();
      }
    };
    // A failed Worker ends its connections; their reconnects open directly.
    worker.onerror = () => {
      worker = null;
      for (const socket of [...remotes.values()]) {
        remotes.delete(socket.id);
        socket.readyState = 3;
        socket.onclose?.();
      }
    };
    return worker;
  }

  const openSocket = url => (startWorker() ? new RemoteSocket(url) : new WebSocket(url));

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
    state.retryDelay = Math.min(state.retryDelay * 2, MAX_RETRY_DELAY);
  }

  function connect(state) {
    let ws;
    try { ws = openSocket(state.url); }
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
    ws.onmessage = ({data, at}) => {
      if (!current()) return;
      if (!(data instanceof ArrayBuffer) || data.byteLength === 0
        || data.byteLength > state.maxDatagram || state.packets.length >= MAX_PACKETS
        || state.bytes + data.byteLength > MAX_QUEUED_BYTES) {
        reconnect(state);
        return;
      }
      state.retryDelay = 250;
      // Arrival time: Rust reads the queue only when the game loop next
      // services it. The Worker timed the message on its own thread.
      state.packets.push({bytes: new Uint8Array(data), at: at ?? performance.now()});
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
      throw new Error('Invalid WebSocket proxy origin');
    }
    url.pathname = `/game/${channel}`;
    const state = {
      url: url.href, maxDatagram, ws: null, packets: [], bytes: 0,
      retryTimer: undefined, connectTimer: undefined, retryDelay: 250,
      closed: false, routeChanged: false, receivedAgo: 0,
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
    // bytes views Wasm memory, and send() refuses a view of the threaded build's shared memory.
    try { ws.send(bytes.slice()); return 0; }
    catch { reconnect(state); return 1; }
  };

  globalThis.veraSocketReceive = id => {
    const state = sockets.get(id);
    if (!state || state.closed) return undefined;
    const packet = state.packets.shift();
    if (!packet) return null;
    state.bytes -= packet.bytes.byteLength;
    state.receivedAgo = Math.max(0, performance.now() - packet.at);
    return packet.bytes;
  };

  // Milliseconds since the datagram last returned by veraSocketReceive arrived.
  globalThis.veraSocketReceivedAgo = id => sockets.get(id)?.receivedAgo ?? 0;

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
