#!/usr/bin/env bash
# Write the release notes for the Release workflow (.github/workflows/release.yml)
# to file $1. Both of its paths use them: a dev build, and a release promoted
# from a dev build (PROMOTED_FROM set to the dev build's tag).
#
# Environment: GITHUB_REPOSITORY, GH_TOKEN, TAG, VERSION, STAMP (the build time),
# COMMIT (the full sha the archives were built from), DIST (the directory of
# archives) and, for a release, PROMOTED_FROM. Run from a checkout with full
# history.
set -euo pipefail
out="$1"
repo_url="https://github.com/$GITHUB_REPOSITORY"

# Base = the newest *published* release before this one. A dev build counts
# releases of either kind; a release counts releases only, so its changelog
# spans every dev build since the previous release. Drafts are excluded on
# purpose: an unpublished draft must not become the diff base.
list=(--exclude-drafts)
if [ -n "${PROMOTED_FROM:-}" ]; then
  list+=(--exclude-pre-releases)
fi
prev_tag="$(gh release list --repo "$GITHUB_REPOSITORY" "${list[@]}" \
  --limit 1 --json tagName --jq '.[0].tagName // empty' 2>/dev/null || true)"

# The base is the commit the previous release's tag names. Not its
# targetCommitish: releases made here record the dispatched branch (`main`),
# which now names a newer commit, so the range would come out empty. Fetch that
# one tag rather than rely on what checkout fetched; `^{commit}` peels an
# annotated tag.
base=""
if [ -n "$prev_tag" ] \
  && git fetch --quiet --no-tags origin "+refs/tags/$prev_tag:refs/tags/$prev_tag"; then
  base="$(git rev-parse --verify --quiet "refs/tags/$prev_tag^{commit}" || true)"
fi
if [ -n "$base" ]; then
  base_label="$prev_tag"
else
  base="$(git rev-list --max-parents=0 "$COMMIT" | tail -n 1)"
  base_label="the first commit"
fi

# `tformat` (not `format`) terminates every record with a newline; `format`
# drops the last one, so `read` at EOF discards the final line -- which silently
# hides the only commit in a one-commit range.
#
# A release page cannot show a whole project's history. The first release has
# no previous tag, so its range is every commit ever made; cap the list and link
# the full comparison instead.
CHANGELOG_LIMIT=30
total="$(git rev-list --count --no-merges "$base..$COMMIT")"
compare_url="$repo_url/compare/$base...$COMMIT"
commits=""
while IFS=$'\t' read -r h s; do
  if [ -z "$h" ]; then continue; fi
  commits+="- [\`${h:0:7}\`]($repo_url/commit/$h) $s"$'\n'
done < <(git log --no-merges --max-count="$CHANGELOG_LIMIT" \
  --pretty=tformat:'%H%x09%s' "$base..$COMMIT")
n="$(printf '%s' "$commits" | grep -c '^- ' || true)"

# One row per download, in a fixed order. Every download needs a row, so a new
# kind cannot reach players unlabelled.
rows=""
row_count=0
for spec in '*-windows-x64-*-setup.exe|Windows x64 installer' \
            '*-windows-x64-*.zip|Windows x64' \
            '*-linux-x64-*.tar.gz|Linux x64' \
            '*-macos-arm64-*.dmg|macOS Apple Silicon app' \
            '*-macos-arm64-*.tar.gz|macOS Apple Silicon'; do
  pattern="${spec%%|*}"
  label="${spec#*|}"
  # shellcheck disable=SC2086 # the pattern is a glob
  for file in "$DIST"/vera20k-$pattern; do
    if [ ! -f "$file" ]; then continue; fi
    rows+="| \`$(basename "$file")\` | $label |"$'\n'
    row_count=$((row_count + 1))
  done
done
archive_count="$(find "$DIST" -maxdepth 1 -type f -name 'vera20k-*' | wc -l | tr -d ' ')"
if [ "$row_count" -ne "$archive_count" ]; then
  echo "!! $DIST holds $archive_count downloads but only $row_count have a row" >&2
  exit 1
fi

if [ -n "${PROMOTED_FROM:-}" ]; then
  title="VERA20k \`$TAG\`"
  kind="The same files as dev build \`$PROMOTED_FROM\`, which testers played before this release."
else
  title="VERA20k dev build \`$TAG\`"
  kind="**Development build** from \`main\`: the newest changes, tested less than a
release. Most players want the newest release marked **Latest** on the
[Releases page]($repo_url/releases)."
fi

{
  cat <<EOF
## $title

VERA20k is a free, open-source reimplementation of Command & Conquer: Yuri's
Revenge. **It contains no game files**: you need your own copy of Yuri's
Revenge 1.001.

$kind

- Version: \`$VERSION\`
- Build time: \`$STAMP\` (UTC)
- Built from commit [\`${COMMIT:0:7}\`]($repo_url/commit/$COMMIT)

### Changelog

EOF
  if [ "$n" -gt 0 ]; then
    if [ "$total" -gt "$n" ]; then
      # Newest first, so the cap keeps what a player most wants to read.
      printf '%s commits since %s; the newest %s are below.\n\n' "$total" "$base_label" "$n"
    else
      printf '%s commit(s) since %s.\n\n' "$total" "$base_label"
    fi
    printf '%s' "$commits"
    if [ "$total" -gt "$n" ]; then
      printf '\n_%s more not shown — [full comparison](%s)._\n' "$((total - n))" "$compare_url"
    fi
  else
    echo 'No commits were added since the previous release.'
  fi
  cat <<EOF

### Downloads

On Windows, the installer is easiest: it finds your Yuri's Revenge folder and
sets everything up. On macOS, open the \`.dmg\` and drag VERA20k into
Applications. The first time, macOS blocks it because Apple has not notarized
it: click Done, then Open Anyway in System Settings > Privacy & Security. It
then finds your Yuri's Revenge folder, or asks for it.
Otherwise, extract an archive straight into your Yuri's Revenge folder, or keep
it in its own folder and point \`config.toml\` at the game (see the README.txt
inside).

| download | platform |
|---|---|
$rows
\`SHA256SUMS.txt\` lists every download's checksum. Builds are signed with
GitHub build provenance; verify one with:

\`\`\`sh
gh attestation verify <download> --repo $GITHUB_REPOSITORY
\`\`\`

VERA20k is licensed under the GNU General Public License version 3.
Source: $repo_url

Command & Conquer, Red Alert and Yuri's Revenge are trademarks of Electronic
Arts Inc. VERA20k is not affiliated with or endorsed by Electronic Arts.
EOF
} > "$out"

# Assert the substitution happened instead of shipping a literal placeholder to
# players. Both the shell form ($VAR / ${VAR}) and an @@NAME@@ template marker
# are checked; a quoted heredoc <<'EOF' would silently keep either one. Commit
# lines are skipped: a subject may legitimately mention a variable such as
# $RA2_DIR.
placeholder='(\$[{]?[A-Z_]+[}]?|@@[A-Z_]+@@)'
if grep -v '^- \[`' "$out" | grep -qE "$placeholder"; then
  echo "!! release notes still contain an unexpanded placeholder:" >&2
  grep -v '^- \[`' "$out" | grep -nE "$placeholder" >&2
  exit 1
fi
echo "--- $out ($(wc -l < "$out") lines) ---"
head -20 "$out"
