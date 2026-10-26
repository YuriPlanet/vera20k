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
- One line per paragraph or bullet: GitHub shows every line break in release notes.
- Name contributors by GitHub login without `@`, which would notify them.

## Dev build

`nightly.yml` starts one from main at 18:00 UTC each day that main has changed. By hand:

1. `gh workflow run release.yml --ref main -f publish=prerelease`; `draft` to look before
   publishing, `none` to only build and test.
2. Wait with one background `gh run watch <run> --exit-status`.

## Release

1. Promote the published dev build that was tested. The workflow uses that build's commit
   and copies its three installers under plain release filenames:
   `gh workflow run release.yml --ref main -f promote=<dev build tag>`. Merge no workflow
   change between the dev build and this step: creating a tag on a commit whose
   `.github/workflows` differ from main's fails with HTTP 403.
2. Replace the draft's change list with the plain-words list, keeping the install and license
   lines (`gh release edit vX.Y.Z --notes-file <file>`), read the rendered draft, then publish it:
   `gh release edit vX.Y.Z --draft=false --latest`.
3. Check that it is Latest, holds the three installers, and that
   `gh attestation verify <download> --repo YuriPlanet/vera20k` passes for each.
4. In a PR, raise the version in `Cargo.toml`, `src/Cargo.toml` and their two `Cargo.lock`
   entries above the release and check all four versions agree. This allows the development
   build workflow to start again.

For example, after publishing `v0.2.0`, main can move to `0.2.1` for future development
builds while `v0.2.0` remains Latest.

A release that main's version cannot carry, such as a patch while main is ahead, comes from
a branch: main plus one commit setting the version, dispatched with `--ref <branch>`. Its
`.github/workflows` must equal main's (`git diff origin/main <branch> -- .github/workflows`
is empty) through the dev build and the promote.

## Only with the maintainer's yes

- Deleting a release or a tag, or replacing a published release's files.
- Deleting dev builds; the workflow prunes them past the newest ten.
