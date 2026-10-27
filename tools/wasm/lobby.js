/* Byte transport for the existing Rust lobby owner. Loaded only by wasm-net. */
(() => {
    const visits = new Map();
    let next = 1;

    globalThis.veraLobbyOpen = function(proxyOrigin) {
        const url = new URL(proxyOrigin);
        const local = ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname);
        if (!(url.protocol === 'wss:' || (url.protocol === 'ws:' && local)) || url.username || url.password
            || url.pathname !== '/' || url.search || url.hash) throw new Error('Invalid proxy origin');
        url.protocol = url.protocol === 'wss:' ? 'https:' : 'http:';
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

    globalThis.veraLobbyRequest = async function(id, path, token, body, limit) {
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
            const headers = {'Accept': 'application/json'};
            if (token) headers.Authorization = 'Bearer ' + token;
            if (body !== undefined) headers['Content-Type'] = 'application/json';
            const response = await fetch(url.href, {
                method: body === undefined ? 'GET' : 'POST', headers, body,
                credentials: 'omit', redirect: 'error', cache: 'no-store', signal: abort.signal,
            });
            if (response.status === 401) {
                await response.body?.cancel();
                return JSON.stringify([401, '']);
            }
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
