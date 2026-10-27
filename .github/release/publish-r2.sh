#!/usr/bin/env bash
# Copy the three attested stable-release installers to immutable R2 keys.
# Credentials come only from the environment; never enable shell tracing here.
set +x
set -euo pipefail
umask 077

die() { printf '%s\n' "Download publication failed: $*" >&2; exit 1; }
[[ $# == 1 ]] || die 'usage: publish-r2.sh vMAJOR.MINOR.PATCH'
tag=$1
[[ $tag =~ ^v(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$ ]] \
  || die 'only canonical stable version tags are accepted'
version=${tag#v}
repo=YuriPlanet/vera20k
bucket=vera20k-downloads
[[ ${R2_ACCOUNT_ID:-} =~ ^[0-9a-f]{32}$ ]] || die 'R2_ACCOUNT_ID is required'
[[ -n ${AWS_ACCESS_KEY_ID:-} && -n ${AWS_SECRET_ACCESS_KEY:-} ]] \
  || die 'S3 access credentials are required'
for command in gh aws jq; do command -v "$command" >/dev/null || die "$command is required"; done
export AWS_DEFAULT_REGION=auto AWS_EC2_METADATA_DISABLED=true AWS_PAGER=''
export AWS_RETRY_MODE=standard AWS_MAX_ATTEMPTS=5
export AWS_REQUEST_CHECKSUM_CALCULATION=when_required AWS_RESPONSE_CHECKSUM_VALIDATION=when_required
endpoint="https://${R2_ACCOUNT_ID}.r2.cloudflarestorage.com"

# The workflow serializes all publications. This additional atomic local lock
# protects manual invocations on the same machine; S3 conditional writes cover
# races with another machine. A killed process can leave a lock to inspect.
lock=${R2_PUBLISH_LOCK_DIR:-${TMPDIR:-/tmp}/vera20k-downloads-publish.lock}
mkdir "$lock" 2>/dev/null || die 'another local publication holds the lock'
work=''
cleanup() {
  if [[ -n $work ]]; then rm -rf -- "$work"; fi
  rmdir "$lock" 2>/dev/null || true
}
trap cleanup EXIT
work=$(mktemp -d "${TMPDIR:-/tmp}/vera20k-r2-publish.XXXXXX")
mkdir "$work/assets"

sha256() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}
byte_count() { wc -c < "$1" | tr -d '[:space:]'; }
s3() { aws --endpoint-url "$endpoint" --region auto s3api "$@"; }

# Fetching the exact tag peels annotated tags without trusting target_commitish,
# which may contain a mutable branch name in GitHub release metadata.
tag_commit() {
  local object depth=0 type digest
  object=$(gh api "repos/$repo/git/ref/tags/$tag") || die 'cannot resolve release tag'
  while :; do
    type=$(jq -er '.object.type' <<< "$object") || die 'invalid tag object'
    digest=$(jq -er '.object.sha | select(test("^[0-9a-f]{40}$"))' <<< "$object") \
      || die 'invalid tag commit'
    if [[ $type == commit ]]; then printf '%s\n' "$digest"; return; fi
    [[ $type == tag && $depth -lt 8 ]] || die 'tag does not resolve to a commit'
    object=$(gh api "repos/$repo/git/tags/$digest") || die 'cannot peel release tag'
    depth=$((depth + 1))
  done
}
release_snapshot() {
  jq -cS '{id,tag_name,draft,prerelease,assets:(.assets | map({id,name,size,digest,state}) | sort_by(.name))}' "$1"
}

gh api "repos/$repo/releases/tags/$tag" > "$work/release.json"
names=("vera20k-$version-windows-x64-setup.exe" "vera20k-$version-macos-arm64.dmg" "vera20k-$version-linux-x64.AppImage")
platforms=(windows macos linux)
types=(application/vnd.microsoft.portable-executable application/x-apple-diskimage application/octet-stream)
jq -e --arg tag "$tag" --arg a "${names[0]}" --arg b "${names[1]}" --arg c "${names[2]}" '
  .tag_name == $tag and .draft == false and .prerelease == false
  and (.assets | length == 3)
  and ([.assets[].name] | sort == ([$a,$b,$c] | sort))
  and all(.assets[]; .state == "uploaded" and (.size | type == "number") and .size > 0
    and (.digest | type == "string") and (.digest | test("^sha256:[0-9a-f]{64}$")))
' "$work/release.json" >/dev/null || die 'release must contain exactly three published stable installers with SHA-256 digests'
commit=$(tag_commit)
jq -nS --arg version "$version" --arg tag "$tag" --arg commit "$commit" \
  '{version:$version,tag:$tag,commit:$commit,files:{}}' > "$work/manifest.json"

# Complete every GitHub download/provenance check before writing any R2 object.
for index in 0 1 2; do
  name=${names[$index]}
  file="$work/assets/$name"
  gh release download "$tag" --repo "$repo" --dir "$work/assets" --pattern "$name"
  [[ -f $file && ! -L $file ]] || die "missing regular installer: $name"
  size=$(byte_count "$file")
  digest=$(sha256 "$file")
  jq -e --arg name "$name" --arg digest "sha256:$digest" --argjson size "$size" \
    '.assets[] | select(.name == $name) | .size == $size and .digest == $digest' \
    "$work/release.json" >/dev/null || die "GitHub installer size or digest differs: $name"
  gh attestation verify "$file" --repo "$repo" --source-digest "$commit" \
    --signer-workflow "$repo/.github/workflows/release.yml" \
    --deny-self-hosted-runners >/dev/null
  jq -S --arg platform "${platforms[$index]}" --arg name "$name" \
    --arg key "releases/$tag/$name" --arg sha256 "$digest" --argjson size "$size" \
    '.files[$platform] = {key:$key,name:$name,sha256:$sha256,size:$size}' \
    "$work/manifest.json" > "$work/next-manifest.json"
  mv "$work/next-manifest.json" "$work/manifest.json"
done
gh api "repos/$repo/releases/tags/$tag" > "$work/release-final.json"
[[ "$(release_snapshot "$work/release.json")" == "$(release_snapshot "$work/release-final.json")" \
  && $commit == "$(tag_commit)" ]] || die 'GitHub release changed during verification'

# Return 4 only for an absent object; auth, transport and service errors fail.
# Metadata from the GET, not an earlier HEAD, supplies the latest pointer's ETag.
fetch_optional() {
  local key=$1 target=$2
  if s3 head-object --bucket "$bucket" --key "$key" --output json \
      > "$target.head.json" 2> "$target.error"; then
    s3 get-object --bucket "$bucket" --key "$key" --output json "$target" \
      > "$target.get.json" 2> "$target.error" || die "cannot read R2 object: $key"
    return 0
  fi
  if grep -Eq 'An error occurred \((404|NoSuchKey|NotFound)\)' "$target.error"; then return 4; fi
  die "cannot inspect R2 object: $key"
}
verify_copy() {
  [[ $(byte_count "$1") == "$(byte_count "$2")" && $(sha256 "$1") == "$(sha256 "$2")" ]] \
    || die "R2 bytes differ: $3"
}
publish_immutable() {
  local key=$1 source=$2 content_type=$3 target="$work/readback" result
  local disposition=inline
  if [[ -n ${4:-} ]]; then disposition="attachment; filename=\"$4\""; fi
  if fetch_optional "$key" "$target"; then
    verify_copy "$source" "$target" "$key"
    return
  else
    result=$?
    [[ $result == 4 ]] || die "cannot inspect immutable object: $key"
  fi
  if ! s3 put-object --bucket "$bucket" --key "$key" --body "$source" \
      --if-none-match '*' --content-type "$content_type" \
      --content-disposition "$disposition" \
      --cache-control 'public,max-age=31536000,immutable' \
      --metadata "sha256=$(sha256 "$source")" --output json \
      > "$work/put.json" 2> "$work/put.error"; then
    # Another authorized publisher may have completed the exact same object.
    grep -Eq 'An error occurred \((412|PreconditionFailed)\)' "$work/put.error" \
      || die "immutable upload failed: $key"
  fi
  fetch_optional "$key" "$target" || die "uploaded object is absent: $key"
  verify_copy "$source" "$target" "$key"
}

for index in 0 1 2; do
  publish_immutable "releases/$tag/${names[$index]}" "$work/assets/${names[$index]}" "${types[$index]}" "${names[$index]}"
done
publish_immutable "releases/$tag/manifest.json" "$work/manifest.json" application/json

gh api "repos/$repo/releases/latest" > "$work/latest-release.json"
if ! jq -e --arg tag "$tag" '.tag_name == $tag and .draft == false and .prerelease == false' \
    "$work/latest-release.json" >/dev/null; then
  printf '%s\n' "Mirrored $tag; GitHub has a different latest release, so latest.json stays unchanged."
  exit 0
fi

conditions=(--if-none-match '*')
if fetch_optional latest.json "$work/prior-latest.json"; then
  jq -e '
    type == "object" and (.tag | type == "string") and (.version | type == "string")
    and (.version | test("^(0|[1-9][0-9]{0,8})\\.(0|[1-9][0-9]{0,8})\\.(0|[1-9][0-9]{0,8})$"))
    and .tag == ("v" + .version) and (.commit | test("^[0-9a-f]{40}$"))
    and (.files | type == "object") and (.files | keys == ["linux","macos","windows"])
  ' "$work/prior-latest.json" >/dev/null || die 'existing latest manifest is malformed'
  if jq -e --arg version "$version" \
      '(.version | split(".") | map(tonumber)) > ($version | split(".") | map(tonumber))' \
      "$work/prior-latest.json" >/dev/null; then
    printf '%s\n' "Mirrored $tag; latest.json already names a newer version and will not roll back."
    exit 0
  fi
  if [[ $(jq -r .tag "$work/prior-latest.json") == "$tag" ]]; then
    verify_copy "$work/manifest.json" "$work/prior-latest.json" latest.json
    printf '%s\n' "Downloads for $tag are already published and verified."
    exit 0
  fi
  etag=$(jq -er '.ETag | select(type == "string" and length > 0)' "$work/prior-latest.json.get.json") \
    || die 'missing latest manifest ETag'
  conditions=(--if-match "$etag")
else
  result=$?
  [[ $result == 4 ]] || die 'cannot inspect latest manifest'
fi

# Recheck immediately before the atomic conditional update. A failed PUT leaves
# the prior pointer intact; a conflicting publisher is never overwritten.
latest_tag=$(gh api "repos/$repo/releases/latest" --jq '.tag_name')
if [[ $latest_tag != "$tag" ]]; then
  printf '%s\n' "Mirrored $tag; latest changed during publication, so latest.json stays unchanged."
  exit 0
fi
s3 put-object --bucket "$bucket" --key latest.json --body "$work/manifest.json" \
  "${conditions[@]}" --content-type application/json --cache-control no-cache \
  --metadata "sha256=$(sha256 "$work/manifest.json")" --output json \
  > "$work/latest-put.json" 2> "$work/latest-put.error" || die 'latest pointer update failed; prior/concurrent pointer was preserved'
fetch_optional latest.json "$work/final-latest.json" || die 'latest manifest is absent after publication'
verify_copy "$work/manifest.json" "$work/final-latest.json" latest.json
printf '%s\n' "Published and verified the three $tag downloads."
