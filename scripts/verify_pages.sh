#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf '%s\n' "$1" >&2
  exit 1
}

usage() {
  printf 'Usage: %s {validate PROOF|current [--wait SECONDS]|verify PROOF URL}\n' "$0" >&2
  exit "${1:-2}"
}

read_proof() {
  jq -ers --arg commit "$GITHUB_SHA" '
    if length != 1 then error("Expected a single Pages publication proof") else .[0] end
    | if type != "object" or .commit != $commit then
        error("The publication proof is for a different commit")
      elif (.index_sha256 | type != "string" or (test("\\A[0-9a-f]{64}\\z") | not)) then
        error("The publication proof must include a lowercase index_sha256")
      else .index_sha256 end
  ' < "$1"
}

github_api() {
  gh api "$1" --jq "$2" || fail 'Could not check the current default-branch commit'
}

check_current() {
  local endpoint default_branch branch_path tip current=false
  endpoint="repos/$GITHUB_REPOSITORY"
  default_branch=$(github_api "$endpoint" '.default_branch')
  if [[ "$GITHUB_REF_NAME" == "$default_branch" ]]; then
    branch_path=$(jq -nr --arg branch "$GITHUB_REF_NAME" '$branch | split("/") | map(@uri) | join("/")')
    tip=$(github_api "$endpoint/git/ref/heads/$branch_path" '.object.sha')
    if [[ "$tip" == "$GITHUB_SHA" ]]; then
      current=true
    fi
  fi
  printf 'current=%s\n' "$current" >> "$GITHUB_OUTPUT"
  if [[ "$current" == false ]]; then
    printf '%s\n' 'This commit is no longer at the tip of the default branch. Pages deployment was skipped.' >> "$GITHUB_STEP_SUMMARY"
  fi
}

fetch() {
  curl --disable --fail --silent --show-error --location --globoff \
    --proto '=https' --proto-redir '=https' --max-time 30 \
    --header 'Accept-Encoding: identity' --header 'Cache-Control: no-cache' \
    --output "$2" -- "$1"
}

verify_publication() (
  proof=$1
  url=$2
  working=$(mktemp -d)
  trap 'rm -f -- "$working/proof" "$working/response"; rmdir -- "$working"' EXIT
  cp -- "$proof" "$working/proof"
  index_hash=$(read_proof "$working/proof")

  authority=${url#*://}
  authority=${authority%%/*}
  host_pattern='^([[][^]]+[]]|[[:alnum:]_.-]+)(:[0-9]+)?$'
  if [[ "$url" != [Hh][Tt][Tt][Pp][Ss]://* || "$url" == *['?#']* ||
        "$url" == *[[:space:][:cntrl:]]* || "$authority" == *@* ||
        ! "$authority" =~ $host_pattern ]]; then
    fail 'The successful Pages deployment did not return a valid HTTPS URL'
  fi
  base=$url
  while [[ "$base" == */ ]]; do base=${base%/}; done

  commit_query=$(jq -nr --arg commit "$GITHUB_SHA" '$commit | @uri')
  for ((attempt = 1; attempt <= 6; attempt++)); do
    query="bevy-cicd=$commit_query-$attempt"
    response="$working/response"
    if ! fetch "$base/bevy-cicd-build.json?$query" "$response"; then
      reason='Could not fetch the published marker'
    elif ! cmp -s "$working/proof" "$response"; then
      reason='The published marker differs from the expected publication proof'
    elif ! fetch "$base/?$query" "$response"; then
      reason='Could not fetch the published index'
    else
      checksum=$(sha256sum < "$response")
      if [[ "${checksum%% *}" == "$index_hash" ]]; then
        printf 'page_url=%s\n' "$url" >> "$GITHUB_OUTPUT"
        printf 'Verified the deployed Pages marker and index for `%s`.\n' "$GITHUB_SHA" >> "$GITHUB_STEP_SUMMARY"
        exit 0
      fi
      reason='The published index SHA-256 differs from the publication proof'
    fi

    printf 'HTTP verification attempt %s/6: %s\n' "$attempt" "$reason" >&2
    if (( attempt < 6 )); then sleep 10; fi
  done
  fail 'Published Pages content did not match the checked artifact'
)

case "${1-}" in
  validate)
    (( $# == 2 )) || usage
    read_proof "$2" > /dev/null
    ;;
  current)
    if (( $# == 3 )) && [[ "$2" == --wait && "$3" =~ ^[0-9]+$ ]]; then
      sleep "$3"
    elif (( $# != 1 )); then
      usage
    fi
    check_current
    ;;
  verify)
    (( $# == 3 )) || usage
    verify_publication "$2" "$3"
    ;;
  -h|--help) usage 0 ;;
  *) usage ;;
esac
