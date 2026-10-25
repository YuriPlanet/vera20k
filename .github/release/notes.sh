#!/usr/bin/env bash
# Write the release notes for the Release workflow (.github/workflows/release.yml)
# to file $1: a short list of what changed since the previous release, then a few
# lines on which installer to run, since a release holds only the three
# installers. Both of its paths use them: a dev build, and a release promoted
# from a dev build (PROMOTED_FROM set to the dev build's tag).
#
# GitHub turns every line break in release notes into a visible one, so each
# paragraph and list item is a single line.
#
# Environment: GITHUB_REPOSITORY, GH_TOKEN, COMMIT (the full sha the installers
# were built from) and, for a release, PROMOTED_FROM. Run from a checkout with
# history back to the previous release.
set -euo pipefail
out="$1"
repo_url="https://github.com/$GITHUB_REPOSITORY"

# The previous release is the newest published one. A dev build counts releases
# of either kind; a release counts releases only, so its list spans every dev
# build since the previous release. Drafts never count.
list=(--exclude-drafts)
if [ -n "${PROMOTED_FROM:-}" ]; then
  list+=(--exclude-pre-releases)
fi
prev_tag="$(gh release list --repo "$GITHUB_REPOSITORY" "${list[@]}" \
  --limit 1 --json tagName --jq '.[0].tagName // empty')"
base=""
if [ -n "$prev_tag" ]; then
  git fetch --quiet --no-tags origin "+refs/tags/$prev_tag:refs/tags/$prev_tag"
  base="$(git rev-parse --verify "refs/tags/$prev_tag^{commit}")"
fi

# What changed: each change merged into main since then, newest first, named by
# its pull request's title (or its commit subject without one), without a
# conventional-commit prefix or a trailing (#123). A change that only sets the
# version is left out. A few at most, then a link to the rest.
CHANGES_LIMIT=8
changes=()
if [ -n "$base" ]; then
  while read -r sha; do
    if ! git diff-tree --no-commit-id --name-only -r -m --first-parent "$sha" \
        | grep -qvxE 'Cargo\.toml|src/Cargo\.toml|Cargo\.lock'; then
      continue
    fi
    title="$(gh api "repos/$GITHUB_REPOSITORY/commits/$sha/pulls" \
      --jq '.[0].title // empty' 2>/dev/null || true)"
    if [ -z "$title" ]; then
      title="$(git log -1 --format=%s "$sha")"
    fi
    changes+=("$(sed -E 's/^[a-z]+(\([^)]*\))?!?: //; s/ \(#[0-9]+\)$//' <<< "$title" \
      | awk '{ print toupper(substr($0, 1, 1)) substr($0, 2) }')")
  done < <(git rev-list --first-parent "$base..$COMMIT")
fi

{
  if [ -z "${PROMOTED_FROM:-}" ]; then
    echo "**Development build** of commit [\`${COMMIT:0:7}\`]($repo_url/commit/$COMMIT): newer than the latest release, but tested less."
    echo
  fi
  if [ "${#changes[@]}" -gt 0 ]; then
    echo "### Changes since $prev_tag"
    echo
    for ((i = 0; i < ${#changes[@]} && i < CHANGES_LIMIT; i++)); do
      echo "- ${changes[$i]}"
    done
    if [ "${#changes[@]}" -gt "$CHANGES_LIMIT" ]; then
      echo "- [and $((${#changes[@]} - CHANGES_LIMIT)) more]($repo_url/compare/$prev_tag...$COMMIT)"
    fi
    echo
  fi
  cat <<'EOF'
You need your own copy of Yuri's Revenge.

- **Windows:** run the `-setup.exe`. If SmartScreen warns, click More info, then Run anyway.
- **macOS (Apple Silicon):** open the `.dmg` and drag VERA20K into Applications. The first time, macOS blocks it: click Done, then Open Anyway in System Settings > Privacy & Security.
- **Linux:** make the `.AppImage` executable and start it.
EOF
} > "$out"
cat "$out"
