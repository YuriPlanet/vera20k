/* Byte transport for the existing Rust lobby owner. Loaded only by wasm-net. */
(() => {
    const visits = new Map();
    let next = 1;

    // serviceOrigin: an external launch's match API (docs/spawn.md); the
    // built-in lobby shares the gateway's origin.
    globalThis.veraLobbyOpen = function(proxyOrigin, serviceOrigin) {
        let url;
        if (serviceOrigin == null) {
            url = new URL(proxyOrigin);
            const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
            if (!(url.protocol === 'wss:' || (url.protocol === 'ws:' && local)) || url.username || url.password
                || url.pathname !== '/' || url.search || url.hash) throw new Error('Invalid proxy origin');
            url.protocol = url.protocol === 'wss:' ? 'https:' : 'http:';
        } else {
            url = new URL(serviceOrigin);
            const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
            if (!(url.protocol === 'https:' || (url.protocol === 'http:' && local)) || url.username
                || url.password || url.pathname !== '/' || url.search || url.hash) {
                throw new Error('Invalid service origin');
            }
        }
        const id = next++;
        visits.set(id, {origin: url.origin, requests: new Set()});
        return id;
    };

    globalThis.veraLobbyCancel = function(id) {
        const visit = visits.get(id);
        if (!visit) return;
        visits.delete(id);
        for (const abort of visit.requests) abort.abort();
        visit.requests.clear();
    };

    // headers: a sign-in's JSON [[name, value], ...] from the Rust lobby owner
    // (sign_in_headers), which the gateway forwards by name.
    globalThis.veraLobbyRequest = async function(id, path, token, body, limit, headers) {
        const visit = visits.get(id);
        if (!visit || !path.startsWith('/v1/') || path.includes('#')
            || !Number.isSafeInteger(limit) || limit < 1 || limit > 1024 * 1024) {
            throw new Error('Invalid lobby request');
        }
        const url = new URL(path, visit.origin);
        if (url.origin !== visit.origin || !url.pathname.startsWith('/v1/')) throw new Error('Invalid lobby path');
        const abort = new AbortController();
        visit.requests.add(abort);
        const timer = setTimeout(() => abort.abort(), 12000);
        let reader;
        try {
            const sent = {'Accept': 'application/json'};
            if (token) sent.Authorization = 'Bearer ' + token;
            if (body !== undefined) sent['Content-Type'] = 'application/json';
            for (const [name, value] of headers === undefined ? [] : JSON.parse(headers)) sent[name] = value;
            const response = await fetch(url.href, {
                method: body === undefined ? 'GET' : 'POST', headers: sent, body,
                credentials: 'omit', redirect: 'error', cache: 'no-store', signal: abort.signal,
            });
            reader = response.body.getReader();
            const chunks = [];
            let length = 0;
            for (;;) {
                const {done, value} = await reader.read();
                if (done) break;
                length += value.length;
                if (length > limit) throw new Error('Lobby response too large');
                chunks.push(value);
            }
            if (visits.get(id) !== visit || abort.signal.aborted) throw new Error('Lobby visit ended');
            const bytes = new Uint8Array(length);
            let offset = 0;
            for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
            const text = new TextDecoder('utf-8', {fatal: true}).decode(bytes);
            return JSON.stringify([response.status, text]);
        } catch (error) {
            abort.abort();
            if (reader) await reader.cancel().catch(() => {});
            throw error;
        } finally {
            clearTimeout(timer);
            visit.requests.delete(abort);
        }
    };

    globalThis.veraLobbyDelay = ms => new Promise(resolve => setTimeout(resolve, ms));
})();
