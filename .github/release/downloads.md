# Website downloads

The release workflow builds and verifies three installers. After a stable release
is published, `downloads.yml` runs `publish-r2.sh` to mirror those exact files to
Cloudflare R2. It never rebuilds or uploads game data.

The publisher verifies the release tag, GitHub size/SHA-256 and the build
attestation. It uploads immutable `releases/v<version>/<filename>` objects,
reads them back and checks their bytes, then stores `manifest.json` for that
version. Finally it conditionally replaces `latest.json`, only if this is
GitHub's latest stable release and the pointer would not move backward.
The website's three `/download/<platform>` routes read that pointer.

## Setup and retry

GitHub Actions secrets are `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID` and
`R2_SECRET_ACCESS_KEY`. Use an R2 S3 token with object read/write permission
restricted to the `vera20k-downloads` bucket; bucket administration is unnecessary.
The public custom domain is `downloads.vera20k.org`; r2.dev stays disabled.
Credentials must not appear in code, command arguments or logs.

The Ubuntu 24.04 runner supplies `gh`, AWS CLI and `jq`. To retry, dispatch
`downloads.yml` with the published stable tag, for example `v0.2.2`.
A local run uses `bash .github/release/publish-r2.sh v0.2.2` with `R2_ACCOUNT_ID`,
`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and GitHub authentication already
in the environment. Never enable shell tracing.

The initial v0.2.2 build predates this workflow, so dispatch its first mirror
manually on `main`. Later releases whose tagged commit includes the workflow
start it on publication.

Retries reuse existing objects only when their bytes match. A failed upload
leaves the prior latest pointer intact. Workflow concurrency and a local
atomic-directory lock serialize normal runs; S3 conditional writes preserve
objects and competing pointer updates across machines. After an interrupted
local run, inspect a remaining temporary lock before removing it. GitHub and
R2 cannot provide one cross-service transaction; the publisher rechecks the
latest release immediately before updating the pointer.

Tests: `python -m unittest tools.test_release_downloads`; the repository's
`python -m tools.run_tests` includes these local CLI fixtures.

References: [R2 tokens](https://developers.cloudflare.com/r2/api/tokens/),
[AWS CLI access](https://developers.cloudflare.com/r2/examples/aws/aws-cli/),
[conditional writes](https://developers.cloudflare.com/r2/api/s3/api/#implemented-object-level-operations).
