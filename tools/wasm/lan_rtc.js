// Browser LAN datagrams over WebRTC. The gateway only groups peers by public
// address, assigns virtual IPv4s and forwards offers/answers/candidates; game
// bytes travel peer to peer. No ACKs, retries or game protocol state here.
(() => {
  const MAX_PACKETS = 256;
  const MAX_QUEUED_BYTES = 256 * 1024;
  const MAX_SEND_BYTES = 256 * 1024;
  const BROADCAST = 0xffffffff;
  // Peer frame: source port, destination port (big endian), then the datagram.
  const HEADER = 4;
  // Received packets: peer virtual IPv4 and source port, then the datagram.
  const PREFIX = 6;
  const sockets = new Map();
  let session = null;
  let next = 1;

  function signal(message) {
    const ws = session?.ws;
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(message));
  }

  function dropPeer(address) {
    const peer = session.peers.get(address);
    if (!peer) return;
    session.peers.delete(address);
    peer.channel.onmessage = peer.channel.onclose = null;
    peer.pc.onicecandidate = peer.pc.onconnectionstatechange = null;
    peer.pc.close();
  }

  function connected(address) {
    return session.peers.get(address)?.channel.readyState === 'open';
  }

  function deliver(address, data) {
    if (!(data instanceof ArrayBuffer) || data.byteLength <= HEADER) return;
    const frame = new Uint8Array(data);
    const source = (frame[0] << 8) | frame[1];
    const destination = (frame[2] << 8) | frame[3];
    const length = frame.length - HEADER;
    for (const state of sockets.values()) {
      if (state.port !== destination) continue;
      // A full receive queue loses the datagram, as a full UDP buffer would.
      if (length > state.maxDatagram || state.packets.length >= MAX_PACKETS
        || state.bytes + length > MAX_QUEUED_BYTES) return;
      const packet = new Uint8Array(PREFIX + length);
      packet[0] = address >>> 24; packet[1] = (address >>> 16) & 255;
      packet[2] = (address >>> 8) & 255; packet[3] = address & 255;
      packet[4] = source >>> 8; packet[5] = source & 255;
      packet.set(frame.subarray(HEADER), PREFIX);
      state.packets.push(packet);
      state.bytes += length;
      return;
    }
  }

  // The lower virtual address owns offers, so a pair never negotiates twice.
  function scheduleOffer(address, delay) {
    if (session.address === 0 || session.address >= address) return;
    clearTimeout(session.retries.get(address));
    session.retries.set(address, setTimeout(() => {
      session.retries.delete(address);
      if (session.members.has(address) && !session.peers.has(address)) {
        offer(address).catch(() => { if (session) dropPeer(address); });
      }
    }, delay));
  }

  function createPeer(address) {
    dropPeer(address);
    const pc = new RTCPeerConnection({iceServers: session.iceServers});
    // Negotiated on both sides: UDP-like, unordered and never retransmitted.
    const channel = pc.createDataChannel('vera', {
      negotiated: true, id: 0, ordered: false, maxRetransmits: 0,
    });
    channel.binaryType = 'arraybuffer';
    const peer = {pc, channel, candidates: []};
    const current = () => session?.peers.get(address) === peer;
    const lost = () => {
      if (!current()) return;
      dropPeer(address);
      scheduleOffer(address, 1000);
    };
    channel.onmessage = ({data}) => { if (current()) deliver(address, data); };
    channel.onclose = lost;
    pc.onicecandidate = ({candidate}) => {
      if (current() && candidate) signal({type: 'signal', to: address, data: {candidate: candidate.toJSON()}});
    };
    pc.onconnectionstatechange = () => {
      if (['failed', 'closed'].includes(pc.connectionState)) lost();
    };
    session.peers.set(address, peer);
    return peer;
  }

  async function offer(address) {
    const {pc} = createPeer(address);
    await pc.setLocalDescription(await pc.createOffer());
    signal({type: 'signal', to: address, data: {description: pc.localDescription.toJSON()}});
  }

  async function accept(peer, description) {
    await peer.pc.setRemoteDescription(description);
    for (const candidate of peer.candidates.splice(0)) await peer.pc.addIceCandidate(candidate);
  }

  async function negotiate(from, data) {
    const type = data.description?.type;
    if (type === 'offer') {
      const peer = createPeer(from);
      await accept(peer, data.description);
      await peer.pc.setLocalDescription(await peer.pc.createAnswer());
      signal({type: 'signal', to: from, data: {description: peer.pc.localDescription.toJSON()}});
      return;
    }
    const peer = session.peers.get(from);
    if (!peer) return;
    if (type === 'answer') await accept(peer, data.description);
    else if (data.candidate && !peer.pc.remoteDescription) peer.candidates.push(data.candidate);
    else if (data.candidate) await peer.pc.addIceCandidate(data.candidate);
  }

  function receiveSignal(text) {
    let message;
    try { message = JSON.parse(text); } catch { return; }
    if (message.type === 'welcome') {
      const address = message.address >>> 0;
      if (session.address !== 0 && session.address !== address) {
        // A new virtual address is a new LAN identity for every peer.
        for (const peer of [...session.peers.keys()]) dropPeer(peer);
      }
      session.address = address;
      session.iceServers = Array.isArray(message.iceServers) ? message.iceServers : [];
      session.members = new Set(message.peers.map(peer => peer >>> 0));
      for (const peer of [...session.peers.keys()]) {
        // Peers whose signaling is reconnecting keep an open data channel.
        if (!session.members.has(peer) && !connected(peer)) dropPeer(peer);
      }
      for (const peer of session.members) scheduleOffer(peer, 0);
    } else if (message.type === 'join') {
      session.members.add(message.peer >>> 0);
      scheduleOffer(message.peer >>> 0, 0);
    } else if (message.type === 'leave') {
      session.members.delete(message.peer >>> 0);
      if (!connected(message.peer >>> 0)) dropPeer(message.peer >>> 0);
    } else if (message.type === 'signal' && message.data) {
      // Serialized so candidates never race their pair's descriptions.
      const from = message.from >>> 0;
      session.negotiation = session.negotiation
        .then(() => negotiate(from, message.data))
        .catch(() => {}); // A broken pair ends; protocol timeouts decide.
    }
  }

  function reconnect() {
    if (!session || session.retryTimer !== undefined) return;
    const ws = session.ws;
    session.ws = null;
    clearTimeout(session.connectTimer);
    if (ws) { ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null; ws.close(); }
    // Established data channels outlive the signaling connection.
    session.retryTimer = setTimeout(() => {
      session.retryTimer = undefined;
      connect();
    }, session.retryDelay);
    session.retryDelay = Math.min(session.retryDelay * 2, 30000);
  }

  function connect() {
    const url = new URL(session.url);
    if (session.address) url.searchParams.set('prefer', String(session.address));
    let ws;
    try { ws = new WebSocket(url.href); } catch { reconnect(); return; }
    session.ws = ws;
    const current = () => session?.ws === ws;
    session.connectTimer = setTimeout(() => { if (current()) reconnect(); }, 10000);
    ws.onopen = () => { if (current()) clearTimeout(session.connectTimer); };
    ws.onmessage = ({data}) => {
      if (!current() || typeof data !== 'string') return;
      session.retryDelay = 250;
      receiveSignal(data);
    };
    ws.onclose = ws.onerror = () => { if (current()) reconnect(); };
  }

  globalThis.veraLanOpen = (origin, port, maxDatagram) => {
    const url = new URL(origin);
    const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
    if (!(url.protocol === 'wss:' || (url.protocol === 'ws:' && local))
      || url.username || url.password || url.pathname !== '/' || url.search || url.hash
      || !Number.isInteger(port) || port < 1 || port > 65535
      || [...sockets.values()].some(state => state.port === port)
      || !Number.isInteger(maxDatagram) || maxDatagram < 1 || maxDatagram > 2048) {
      throw new Error('Invalid compiled LAN signaling configuration');
    }
    url.pathname = '/lan/signal';
    if (!session) {
      session = {
        url: url.href, ws: null, address: 0, members: new Set(), peers: new Map(),
        retries: new Map(), iceServers: [], negotiation: Promise.resolve(),
        retryTimer: undefined, connectTimer: undefined, retryDelay: 250,
      };
      connect();
    }
    const id = next++;
    sockets.set(id, {port, maxDatagram, packets: [], bytes: 0});
    return id;
  };

  globalThis.veraLanSend = (id, address, port, bytes) => {
    const state = sockets.get(id);
    if (!state || !bytes.length || bytes.length > state.maxDatagram) return 2;
    const frame = new Uint8Array(HEADER + bytes.length);
    frame[0] = state.port >>> 8; frame[1] = state.port & 255;
    frame[2] = port >>> 8; frame[3] = port & 255;
    frame.set(bytes, HEADER);
    const targets = (address >>> 0) === BROADCAST
      ? [...session.peers.values()] : [session.peers.get(address >>> 0)].filter(Boolean);
    for (const {channel} of targets) {
      // An unconnected or unknown peer loses the datagram, like unrouted UDP.
      if (channel.readyState !== 'open') continue;
      if (channel.bufferedAmount + frame.length > MAX_SEND_BYTES) return 1;
      try { channel.send(frame); } catch { return 1; }
    }
    return 0;
  };

  globalThis.veraLanReceive = id => {
    const state = sockets.get(id);
    if (!state) return undefined;
    const packet = state.packets.shift();
    if (!packet) return null;
    state.bytes -= packet.length - PREFIX;
    return packet;
  };

  globalThis.veraLanLocalAddress = () => session?.address ?? 0;

  globalThis.veraLanClose = id => {
    if (!sockets.delete(id) || sockets.size || !session) return;
    for (const timer of session.retries.values()) clearTimeout(timer);
    clearTimeout(session.retryTimer);
    clearTimeout(session.connectTimer);
    for (const address of [...session.peers.keys()]) dropPeer(address);
    const ws = session.ws;
    if (ws) { ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null; ws.close(); }
    session = null;
  };
})();
