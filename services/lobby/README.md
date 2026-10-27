# Online lobby service

The Cloudflare Worker serves `protocol.md`. Its SQLite Durable Object owns guest
profiles, authenticated sessions, room membership, settings revisions, readiness
and start agreement. The relay and clients own UDP connectivity and simulation.

German Bunker (`Lob_41_2`) is the only public chat lobby. Its durable `public`
room id and membership survive the name migration; player-created game rooms
remain available. Players enter a username and receive a fresh guest identity
and bearer session. An active name is reserved without case; another sign-in
cannot take it over. A host can start against an explicitly selected AI opponent
without another human; AI rows remain outside the network roster.

```sh
npm ci
npm test
cf deploy --dry-run
```

Cloudflare's tooling currently pins vulnerable Zod and Undici patch versions.
The scoped package overrides select their compatible security fixes; remove them
when the upstream configuration and Miniflare packages require patched versions.
These packages run in local build/deployment tools and are not worker imports.

For deployment, use the authenticated `cf` profile and set
`VERA20K_RELAY_ADDRESS` to the public relay IPv4 address and port. Keep a temporary
mode-0600 JSON file outside the repository containing `RELAY_TICKET_KEY`, then run
`cf deploy --secrets-file /private/path/to/secrets.json`. The key must match the
relay's systemd credential. The CLI's authentication stays in its own credential
storage. No provider token or server key belongs in the build or client.

Chat translation (`POST /v1/translate`) calls a separately deployed Worker through a
service binding. Set `VERA20K_TRANSLATOR_WORKER` to its name when deploying; without it
the lobby deploys unbound and reports `translation_unavailable`. Deploy that Worker
first. The lobby caches its replies and owns the rate limits and daily budget.

Operators can post one notice into the lobby chat. Add `ADMIN_KEY`, a random
secret of at least 32 characters (`openssl rand -hex 32`), to the same secrets
file and keep a copy with the relay key; without it the endpoint does not exist.

```sh
curl https://lobby.vera20k.org/v1/admin/notice -H "Authorization: Bearer $ADMIN_KEY" \
  -H 'Content-Type: application/json' -d '{"text":"Server restart at 20:00 UTC.","minutes":60}'
```

The notice appears as public chat from `VERA20k`: at once for players in German
Bunker or a game room that has not started, and for the others when they next
enter German Bunker, until it expires. A new notice replaces it and
`{"minutes":0}` clears it. Players who show only buddy or clan chat don't see it.

The client defaults to the deployed lobby at `https://lobby.vera20k.org`, the
Worker's custom domain. Its workers.dev and preview URLs stay off because their
hostnames carry the account's subdomain. Normal players use
**Internet → Custom Match** without configuring a server address.

For a separate deployment, set `VERA20K_LOBBY_URL` at build time or use its runtime
environment override for staging. Clients require HTTPS, verify TLS and decline
redirects. The client keeps bearer tokens in memory. The lobby stores temporary
guest preferences and relay admission counters in SQLite, then removes them with
the session on logout or inactivity. Names are reusable after that cleanup, with
a fresh player identity. Existing password-account records remain untouched, but
new account creation and password sign-in return `upgrade_required`.

The source schema is configured in `cloudflare.config.ts` using the official
[Cloudflare configuration API](https://developers.cloudflare.com/cf/projects/cloudflare-config/).
Deployments preserve the existing SQLite data. Health is exposed at `/health`;
credentials and request bodies are excluded from application logging.

Lobby capability4 adds fixed-clock ordered multiplayer. Older clients cannot start
new games; existing v3 sessions can poll, renew and finish through ordinary cleanup.
Deploy the compatible relay before this service and its matching clients. Relay
tickets retain wire version2; authenticated ordering controls are an extension.
Unsupported older sessions retire through ordinary cleanup; account
records remain intact. Initial grants last 120 seconds and playing grants 300 seconds.
Each player identity has one monotonically ordered relay admission, so cancelled starts
cannot retain a pool of valid rooms. Payload encryption keys are delivered only
to the match participants over HTTPS and are never sent to the relay.
The HTTPS lobby is trusted with these keys. Relay-blind encryption does not
protect against a compromised lobby or participant.
