#!/usr/bin/env bash
# Create annotated tag $1 on commit $2 for the Release workflow
# (.github/workflows/release.yml), refusing a tag that already exists.
#
# An annotated tag names its tagger. A lightweight one names nobody, and GitHub
# then shows the tagged commit's author as having tagged the release. The API
# leaves an omitted tagger undocumented, so it is set explicitly.
#
# Environment: GITHUB_REPOSITORY, GH_TOKEN.
set -euo pipefail
tag="$1"
commit="$2"

if gh api "repos/$GITHUB_REPOSITORY/git/ref/tags/$tag" >/dev/null 2>&1; then
  echo "Tag $tag already exists." >&2
  exit 1
fi
object="$(jq -n --arg tag "$tag" --arg sha "$commit" '{
    tag: $tag, object: $sha, type: "commit", message: "VERA20K \($tag)",
    tagger: {name: "github-actions[bot]",
             email: "41898282+github-actions[bot]@users.noreply.github.com",
             date: (now | todate)}}' \
  | gh api -X POST "repos/$GITHUB_REPOSITORY/git/tags" --input - --jq .sha)"
gh api -X POST "repos/$GITHUB_REPOSITORY/git/refs" \
  -f ref="refs/tags/$tag" -f sha="$object" > /dev/null
echo "Created annotated tag $tag at $commit"
