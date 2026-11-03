#!/usr/bin/env bash
# release-gate.sh: the first check of the Desktop release workflow. Reads
# everything through the GitHub API (no checkout, no repository code) and
# fails, before anything is built or signed, unless:
#
#   1. the tagged commit is on master (an ancestor of master's tip);
#   2. the `Desktop` workflow (desktop.yml) concluded `success` on that commit.
#      With no run for the commit (desktop.yml is path-filtered, so a
#      docs-only commit has none) it dispatches `Desktop` on the tag and waits
#      for it. A cancelled, failed, timed-out or skipped run is a failure;
#   3. for a final tag (no -rc.N), a published (not draft) release
#      desktop-v<version>-rc.<N> whose tag points at the same commit;
#   4. no release for this tag exists yet.
#
# Environment: GH_TOKEN, REPO (owner/name), TAG, SHA (the tagged commit),
# VERSION (the tag without desktop-v). Optional: DEFAULT_BRANCH (master),
# POLL_SECONDS (30), GATE_TIMEOUT_SECONDS (7200), GH (the gh command).
set -euo pipefail

die() { echo "::error::$*" >&2; exit 1; }

: "${REPO:?Set REPO}" "${TAG:?Set TAG}" "${SHA:?Set SHA}" "${VERSION:?Set VERSION}"
branch=${DEFAULT_BRANCH:-master}
poll=${POLL_SECONDS:-30}
deadline=$((SECONDS + ${GATE_TIMEOUT_SECONDS:-7200}))
gh=${GH:-gh}

# 1. On master.
status=$("$gh" api "repos/$REPO/compare/$SHA...$branch" --jq .status) || die "Could not compare $SHA with $branch."
case "$status" in
  identical | ahead) echo "$SHA is on $branch." ;;
  *) die "$TAG ($SHA) is not on $branch (compare says '$status'). Tag a commit that is merged to $branch." ;;
esac

# 4. Not already released (a draft left by an earlier run counts).
if "$gh" release view "$TAG" -R "$REPO" > /dev/null 2>&1; then
  die "A release for $TAG already exists. Delete it (and its draft) before re-running."
fi

# 3. A final tag needs a published release candidate on the same commit.
case "$VERSION" in
  *-*) echo "$TAG is a release candidate; no candidate required." ;;
  *)
    found=
    while IFS= read -r rc_tag; do
      [ -n "$rc_tag" ] || continue
      rc_sha=$("$gh" api "repos/$REPO/commits/$rc_tag" --jq .sha 2> /dev/null || true)
      if [ "$rc_sha" = "$SHA" ]; then found=$rc_tag; break; fi
    done < <("$gh" api --paginate "repos/$REPO/releases?per_page=100" \
      --jq ".[] | select(.draft == false and .prerelease == true) | .tag_name | select(startswith(\"desktop-v$VERSION-rc.\"))")
    [ -n "$found" ] || die "$TAG needs a published release candidate (desktop-v$VERSION-rc.N) on $SHA, and there is none. Cut and promote an rc first."
    echo "Release candidate $found is published on $SHA."
    ;;
esac

# 2. Desktop passed on this commit.
latest_run() { # prints "<id> <status> <conclusion>" of the newest run, or nothing
  "$gh" api "repos/$REPO/actions/workflows/desktop.yml/runs?head_sha=$SHA&per_page=100" \
    --jq '.workflow_runs | sort_by(.created_at) | last | select(. != null) | "\(.id) \(.status) \(.conclusion // "none")"'
}

read -r run_id run_status run_conclusion < <(latest_run; echo) || true
if [ -z "${run_id:-}" ]; then
  echo "No Desktop run exists for $SHA; dispatching one on $TAG."
  "$gh" workflow run desktop.yml -R "$REPO" --ref "$TAG" || die "Could not dispatch the Desktop workflow."
  while [ -z "${run_id:-}" ]; do
    [ "$SECONDS" -lt "$deadline" ] || die "The dispatched Desktop run never appeared."
    sleep "$poll"
    read -r run_id run_status run_conclusion < <(latest_run; echo) || true
  done
fi

while [ "$run_status" != completed ]; do
  [ "$SECONDS" -lt "$deadline" ] || die "Desktop run $run_id did not finish in time."
  echo "Desktop run $run_id is $run_status; waiting."
  sleep "$poll"
  read -r run_id run_status run_conclusion < <(latest_run; echo)
done
[ "$run_conclusion" = success ] || die "Desktop run $run_id on $SHA concluded '$run_conclusion', not success (a cancelled run counts as a failure). Fix or re-run it, then tag again."
echo "Desktop run $run_id passed on $SHA. The gate is open."
