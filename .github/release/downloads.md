# Website downloads

The release workflow builds and verifies four installers: Windows, Linux and a
Mac disk image each for Apple Silicon and Intel Macs (`macos-x64`, which
releases before 0.2.4 lack). After a stable release is published,
`downloads.yml` runs `publish-r2.sh` to mirror those exact files to Cloudflare
R2. It never rebuilds or uploads game data.

The publisher verifies the release tag, GitHub size/SHA-256 and the build
attestation. It uploads immutable `releases/v<version>/<filename>` objects,
reads them back and checks their bytes, then stores `manifest.json` for that
version and `manifest.json.sig`, its Ed25519 signature. The manifest's `notes`
are the release page's change list in plain text, the bullets under its first
"Changes" heading that are more than a link, which the game's update offer
lists. Its `updater` is the number in `updater-version` at the commit the
installers were built from (none before that file), and its `update_key` the
public key that signs it. A retry keeps these fields as the version was first
published, and none for a version mirrored before manifests had them.
Finally it conditionally replaces `latest.json`, only if this is GitHub's
latest stable release and the pointer would not move backward, unless it is
stopping a bad release (below). `latest.json`
holds the same bytes as the version's `manifest.json`, so the website's
`/download/<platform>` routes read it unchanged.

The game's updater (`src/app/updater`) reads `latest.json`, and installs a
newer version only when `releases/v<version>/manifest.json.sig` verifies those
exact bytes with the key in `update-key.pub` and the download matches the
manifest's size and SHA-256. A newer version whose `updater` is higher than
the game's own `updater-version` it offers as a download by hand from
vera20k.org. One that another key signs, the key its `update_key` names, as
after a key change, it offers as a download from the GitHub releases, showing
only its version: anyone who can write to R2 could sign such a manifest, and
the website's download links follow `latest.json`. Raise `updater-version` in
the change after which older games can no longer install a release themselves,
such as a new installer format. Whatever else later manifests change, they
keep `version`, `tag`, `updater`, `update_key` and `notes` in this form, which
every older game reads.

## Stop a bad release

GitHub's Latest release decides which version `latest.json` may name. To stop a
published release, say `v0.2.4`, from reaching more players, make the stable
release before it Latest again, then dispatch `downloads.yml` for that release
with `roll_back` and approve its run:

```sh
gh release edit v0.2.3 --latest --repo YuriPlanet/vera20k
gh workflow run downloads.yml --ref main --repo YuriPlanet/vera20k -f tag=v0.2.3 -f roll_back=true
```

The publisher checks `v0.2.3` as on publication and refuses unless GitHub marks
it Latest. Then it puts `releases/v0.2.3/manifest.json` back as `latest.json`,
which that version's signature still verifies; the website's download links and
the game's update offer follow it. Games that already installed `v0.2.4` keep
it, since the updater never installs an older version, and a retry of
`v0.2.4`'s run now leaves `latest.json` alone. If that run still waits for
approval, reject it and skip the dispatch: `latest.json` never named `v0.2.4`.
Fix forward with a new version, whose publication makes it Latest, and say on
`v0.2.4`'s release page that it was withdrawn.

Releases are immutable (a repository setting): once published, a release's tag
and installers cannot change, and a deleted release's version number cannot be
used again. A bad release is replaced by a new version, never by new files.

## Setup and retry

The publish job's secrets live only in the `downloads` environment:
`R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` and
`UPDATE_SIGNING_KEY`. Only `v*` tags and `main` may use it, and each run waits
until the maintainer approves it, so a run nobody approved never sees them.
Approve only a run you started, by publishing a release or dispatching a retry.
Use an R2 S3 token with object read/write permission restricted to the
`vera20k-downloads` bucket; bucket administration is unnecessary.
The public custom domain is `downloads.vera20k.org`; r2.dev stays disabled.
Credentials must not appear in code, command arguments or logs.

The bucket lock rule `releases-permanent` keeps every object under `releases/`
indefinitely: R2 refuses to overwrite or delete them, so the website's direct
links serve exactly the files their signed manifest names. `latest.json` stays
outside it. `wrangler r2 bucket lock list vera20k-downloads` shows the rule.

The secret `UPDATE_SIGNING_KEY` holds the PEM Ed25519 private key whose public
half is `update-key.pub` (base64 of the raw 32 bytes). The publisher refuses to
upload anything when the two do not match. Installed games install only updates
that key signed: after it is replaced, they offer each newer release as a
download from the GitHub releases, until their players install one that
carries the new `update-key.pub`.
To make a key at `$KEY`, a path outside the repository readable only by you,
print its public half and set the secret:

```sh
(umask 077; openssl genpkey -algorithm ed25519 -out "$KEY")
openssl pkey -in "$KEY" -pubout -outform DER | tail -c 32 | openssl base64 -A
gh secret set UPDATE_SIGNING_KEY --env downloads --repo YuriPlanet/vera20k < "$KEY"
```

The Ubuntu 24.04 runner supplies `gh`, AWS CLI, `jq` and OpenSSL 3. To retry,
dispatch `downloads.yml` from `main` with the published stable tag, for example
`v0.2.2`, and approve its run.
A local run uses `bash .github/release/publish-r2.sh v0.2.2` with `R2_ACCOUNT_ID`,
`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `UPDATE_SIGNING_KEY`, OpenSSL 3 or
later, and GitHub authentication already in the environment. Never enable shell
tracing.

A release whose tagged commit predates this workflow or its environment, such
as v0.2.2, is mirrored by a dispatch from `main`. Later releases start it on
publication.

Retries reuse existing objects only when their bytes match. A failed upload
leaves the prior latest pointer intact. Workflow concurrency and a local
atomic-directory lock serialize normal runs; S3 conditional writes preserve
objects and competing pointer updates across machines. After an interrupted
local run, inspect a remaining temporary lock before removing it. GitHub and
R2 cannot provide one cross-service transaction; the publisher rechecks the
latest release immediately before updating the pointer.

Tests: `python -m unittest tools.test_release_downloads`; the repository's
`python -m tools.run_tests` includes these local CLI fixtures. Each dev build's
`update-test` job publishes it with this script and a throwaway key
(`tools/update_test.py`), then updates installed older builds to it on every
platform: its own commit built as 0.0.1, and the newest release built from its
tag and installed from its own installer, as players have it, when that release
can install it itself.

References: [R2 tokens](https://developers.cloudflare.com/r2/api/tokens/),
[AWS CLI access](https://developers.cloudflare.com/r2/examples/aws/aws-cli/),
[conditional writes](https://developers.cloudflare.com/r2/api/s3/api/#implemented-object-level-operations).
