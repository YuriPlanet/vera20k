import { bindings, defineConfig, exports } from 'cf/config';

// Account selection comes from the authenticated CLI profile. Deployment
// secrets are passed via `cf deploy --secrets-file` outside this repository.
export default defineConfig({
  worker: {
    name: 'vera20k-lobby',
    entrypoint: './worker.js',
    compatibilityDate: '2026-10-22',
    // workers.dev and preview hostnames carry the account's subdomain.
    domains: ['lobby.vera20k.org'],
    workersDev: false,
    previewUrls: false,
    exports: { Lobby: exports.durableObject({ storage: 'sqlite' }) },
    env: {
      LOBBY: bindings.durableObject({ worker: 'vera20k-lobby', exportName: 'Lobby' }),
      RELAY_ADDRESS: bindings.text(process.env.VERA20K_RELAY_ADDRESS ?? '127.0.0.1:42000'),
      // Chat translation is a separately deployed Worker. Without it,
      // `/v1/translate` reports `translation_unavailable`.
      ...(process.env.VERA20K_TRANSLATOR_WORKER
        ? { TRANSLATOR: bindings.worker({ worker: process.env.VERA20K_TRANSLATOR_WORKER }) } : {}),
    },
  },
});
