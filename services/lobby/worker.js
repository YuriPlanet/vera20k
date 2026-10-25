import { DurableObject } from 'cloudflare:workers';
import { LobbyService, SqlStore } from './service.js';

// SQLite state survives actor eviction. The account and room directory is one
// authority; simulation packets go directly to the separately deployed relay.
export class Lobby extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    const store = new SqlStore(ctx.storage.sql, (fn) => ctx.storage.transactionSync(fn));
    this.service = new LobbyService(store, env);
  }
  async fetch(request) {
    const response = await this.service.fetch(request);
    if (await this.ctx.storage.getAlarm() === null) await this.ctx.storage.setAlarm(Date.now() + 60000);
    return response;
  }
  async alarm() {
    await this.service.maintenance();
    if (this.service.store.list('session:').length) await this.ctx.storage.setAlarm(Date.now() + 60000);
  }
}

export default {
  fetch(request, env) {
    if (request.method !== 'GET' && request.method !== 'POST') return new Response(null, { status: 405 });
    return env.LOBBY.get(env.LOBBY.idFromName('directory-v1')).fetch(request);
  },
};
