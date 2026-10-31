---
name: release
description: "Make a VERA20K dev build or release with the Release workflow, keeping the release page to the three installers, a really small changes list in plain words and the install lines."
---

# Release

`.github/workflows/release.yml` builds, checks and publishes; its header describes dev
builds and releases. This skill is the maintainer's order of steps and the page style.

## Page style

- Downloads: only the Windows setup, the macOS `.dmg` and the Linux AppImage. The workflow
  uploads nothing else; GitHub adds the source code archives itself.
- Text: a really small list of what changed, then the install lines and the license line
  that `.github/release/notes.sh` writes. No commit dumps, tables, checksums or other legal
  text.
- For a release, rewrite the list `notes.sh` drafts from pull request titles in plain player
  words: about 5 to 9 bullets, the most visible first, small fixes grouped, internal work
  (tests, refactors, version bumps, rules plumbing with no effect yet) left out, then an
  "All changes" compare link. Check each bullet against its pull request's description.
- Installed games list these bullets in their update offer: publishing signs the bullets
  under the first "Changes" heading into the download manifest, so later edits to the page
  do not reach the game.
- One line per paragraph or bullet: GitHub shows every line break in release notes.
- Name contributors by GitHub login without `@`, which would notify them.

## Dev build

`nightly.yml` starts one from main at 18:00 UTC each day that main has changed, unless it is
disabled (`gh workflow list --all` shows its state). By hand:

1. `gh workflow run release.yml --ref main -f publish=prerelease`; `draft` to look before
   publishing, `none` to only build and test.
2. After the builds, its `sign and notarize macOS` job waits in the `macos-signing`
   environment until the maintainer approves it on the run's page (Review deployments); a
   nightly's does too. Tell the maintainer when you start the run, and don't approve it
   yourself.
3. Wait with one background `gh run watch <run> --exit-status`.

## Release

1. Tag the commit of the published dev build that was tested with its version, using the
   maintainer's login, then promote the build. The workflow checks that the tag names that
   commit and copies the build's three installers under plain release filenames.
   Before creating the draft, it verifies the Mac installer's signatures, stapled
   Apple notarization ticket and Gatekeeper acceptance. An old unnotarized dev
   build cannot be promoted:

   ```bash
   git fetch origin tag <dev build tag>
   git tag -a vX.Y.Z -m "VERA20K vX.Y.Z" "<dev build tag>^{commit}"
   git push origin refs/tags/vX.Y.Z
   gh workflow run release.yml --ref main -f promote=<dev build tag>
   ```

2. Replace the draft's change list with the plain-words list, keeping the install and license
   lines (`gh release edit vX.Y.Z --notes-file <file>`), read the rendered draft, then publish it:
   `gh release edit vX.Y.Z --draft=false --latest`.
3. Check that it is Latest, holds the three installers, and that
   `gh attestation verify <download> --repo YuriPlanet/vera20k` passes for each.
4. The `downloads.yml` run it started waits for approval in the `downloads` environment.
   Check that it is this release's run (event `release`, the new tag), then approve it on
   the run's page (Review deployments) or with:

   ```bash
   gh api -X POST repos/YuriPlanet/vera20k/actions/runs/<run>/pending_deployments \
     -F "environment_ids[]=$(gh api repos/YuriPlanet/vera20k/environments/downloads --jq .id)" \
     -f state=approved -f comment=vX.Y.Z
   ```

   Check that it passed and that `https://downloads.vera20k.org/latest.json` names the new
   version: installed games offer their update from that file. The website's download links
   lead to R2 only when that version's signature verifies, and otherwise send players to
   GitHub without an error, so check that each line this prints starts with
   `https://downloads.vera20k.org/releases/vX.Y.Z/`:

   ```bash
   for p in windows macos linux; do
     curl -s -o /dev/null -w '%{redirect_url}\n' "https://vera20k.org/download/$p"
   done
   ```

5. In a PR, raise the version in `Cargo.toml`, `src/Cargo.toml` and their two `Cargo.lock`
   entries above the release and check all four versions agree. This allows the development
   build workflow to start again.

For example, after publishing `v0.2.0`, main can move to `0.2.1` for future development
builds while `v0.2.0` remains Latest.

Dev builds run from `main`: the `macos-signing` environment allows only that exact
branch, so an arbitrary dispatched branch cannot read the Apple credentials.
The separate signing job must pass before updater tests or publication. Stable
promotion keeps the notarized installer bytes unchanged. See
[Mac signing](../../../.github/release/macos/README.md) for environment setup and
resuming a submission that outlasted the job. A patch release from another branch
needs a separate, explicitly authorized signing policy; do not widen the environment
to an arbitrary branch pattern.

The repository ruleset `Release tags` lets only repository admins create, move or delete a tag
other than a dev build's (`v*-*`), so nobody else can publish a release under a version
number. The workflow's token is not an admin and makes only dev build tags.
`gh api repos/YuriPlanet/vera20k/rulesets` lists it.

A published release cannot change: GitHub keeps its tag and installers, and a deleted
release's version number cannot be used again. To stop a bad one reaching more players, make
the release before it Latest and roll `latest.json` back to it (`.github/release/downloads.md`,
Stop a bad release), then fix forward with a new version.

## Only with the maintainer's yes

- Deleting a release or a tag.
- Changing the `Release tags` ruleset.
- Stopping a release by rolling `latest.json` back.
- Deleting dev builds; the workflow prunes them past the newest ten.
