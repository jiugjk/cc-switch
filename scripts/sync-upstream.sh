#!/usr/bin/env bash
# Merge farion1231/cc-switch into this Windows fork.
#
# Recurring conflicts are policy, not code:
#   - README.md / README_ZH.md were rewritten (no sponsor table)
#   - README_DE.md / README_JA.md were deleted (EN/ZH only)
#   - most assets/partners/* and several upstream workflows were dropped
#
# This script keeps those fork decisions and takes everything else from upstream.
# Unexpected conflicts (real code) abort so a human can look.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

UPSTREAM_URL="${UPSTREAM_URL:-https://github.com/farion1231/cc-switch.git}"
UPSTREAM_REF="${UPSTREAM_REF:-main}"
REMOTE="${UPSTREAM_REMOTE:-upstream}"

usage() {
  cat <<'EOF'
Usage: scripts/sync-upstream.sh [--push]

  Merge upstream/main into the current branch, auto-resolving known
  README / partner-asset / dropped-workflow conflicts.

  --push   push the current branch to origin afterwards
EOF
}

PUSH=0
for arg in "$@"; do
  case "$arg" in
    -h|--help) usage; exit 0 ;;
    --push) PUSH=1 ;;
    *) echo "unknown arg: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ -n "$(git status --porcelain)" ]]; then
  echo "working tree is dirty; commit or stash first" >&2
  exit 1
fi

git config merge.ours.driver true

if ! git remote get-url "$REMOTE" >/dev/null 2>&1; then
  git remote add "$REMOTE" "$UPSTREAM_URL"
fi

git fetch "$REMOTE" "$UPSTREAM_REF"

OURS="$(git rev-parse HEAD)"
THEIRS="$(git rev-parse "$REMOTE/$UPSTREAM_REF")"
BASE="$(git merge-base "$OURS" "$THEIRS")"

if [[ "$BASE" == "$THEIRS" ]]; then
  echo "already up to date with $REMOTE/$UPSTREAM_REF ($THEIRS)"
  exit 0
fi

echo "merging $REMOTE/$UPSTREAM_REF ($THEIRS)"
echo "  ours: $OURS"
echo "  base: $BASE"

set +e
git merge --no-edit --no-ff "$REMOTE/$UPSTREAM_REF"
MERGE_RC=$?
set -e

is_keep_ours() {
  case "$1" in
    README.md|README_ZH.md|README_EN.md) return 0 ;;
    *) return 1 ;;
  esac
}

is_keep_deleted() {
  local f="$1"
  case "$f" in
    README_DE.md|README_JA.md) return 0 ;;
    .github/labeler.yml) return 0 ;;
    .github/workflows/claude.yml|.github/workflows/labeler.yml) return 0 ;;
    .github/workflows/release.yml|.github/workflows/stale.yml) return 0 ;;
    .github/workflows/sync-r2.yml) return 0 ;;
    scripts/rewrite-updater-manifest.mjs) return 0 ;;
    assets/partners/*)
      # Drop partner art this fork does not ship. Files still in HEAD are not
      # "deleted" and will not appear as DU.
      return 0
      ;;
    *) return 1 ;;
  esac
}

UNEXPECTED=()

if [[ "$MERGE_RC" -ne 0 ]]; then
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    status="${line:0:2}"
    path="${line:3}"
    # unmerged paths look like "UU path", "DU path", ...
    case "$status" in
      UU|AA)
        if is_keep_ours "$path"; then
          git checkout --ours -- "$path"
          git add -- "$path"
          echo "keep ours: $path"
        else
          UNEXPECTED+=("$status $path")
        fi
        ;;
      DU)
        if is_keep_deleted "$path" || is_keep_ours "$path"; then
          git rm -f -- "$path" >/dev/null
          echo "keep deleted: $path"
        else
          UNEXPECTED+=("$status $path")
        fi
        ;;
      UD)
        if is_keep_ours "$path"; then
          git checkout --ours -- "$path"
          git add -- "$path"
          echo "keep ours (they deleted): $path"
        else
          UNEXPECTED+=("$status $path")
        fi
        ;;
      *)
        UNEXPECTED+=("$status $path")
        ;;
    esac
  done < <(git diff --name-only --diff-filter=U | while read -r p; do
    # recover the two-letter unmerged status
    git status --porcelain --untracked-files=no -- "$p"
  done)

  if ((${#UNEXPECTED[@]})); then
    echo "unexpected conflicts (not auto-resolved):" >&2
    printf '  %s\n' "${UNEXPECTED[@]}" >&2
    echo "resolve those, then: git commit" >&2
    exit 1
  fi

  if [[ -n "$(git diff --name-only --diff-filter=U)" ]]; then
    echo "unmerged files remain:" >&2
    git diff --name-only --diff-filter=U >&2
    exit 1
  fi

  git commit --no-edit
fi

echo "merged $REMOTE/$UPSTREAM_REF -> $(git rev-parse --short HEAD)"

if [[ "$PUSH" -eq 1 ]]; then
  git push origin HEAD
  echo "pushed $(git rev-parse --abbrev-ref HEAD)"
fi
