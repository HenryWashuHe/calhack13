#!/usr/bin/env bash
# Port commits that exist only in the legacy repository (HenryWashuHe/genesis)
# into this one, branch by branch, by patch identity rather than by SHA.
# The two repositories share every tree but no commit SHAs (commit metadata was
# rewritten here), so `git merge`/`cherry-pick <sha>` across them is wrong;
# `git cherry` matches commits by patch-id and cherry-picks only the missing ones.
#
# Usage:  scripts/sync-from-legacy.sh [--push] [--branch <name>]...
#   Dry run by default: prints what would be ported. Never force-pushes.
#   Requires a remote named `legacy` (added automatically) and a clean tree.
set -euo pipefail
LEGACY_URL="https://github.com/HenryWashuHe/genesis.git"
push=0; branches=()
while [ $# -gt 0 ]; do
  case "$1" in
    --push) push=1 ;;
    --branch) shift; branches+=("$1") ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done
git diff --quiet && git diff --cached --quiet || { echo "working tree must be clean" >&2; exit 2; }
git remote get-url legacy >/dev/null 2>&1 || git remote add legacy "$LEGACY_URL"
git fetch -q --prune legacy
git fetch -q --prune origin
if [ ${#branches[@]} -eq 0 ]; then
  mapfile -t branches < <(git for-each-ref --format='%(refname:short)' refs/remotes/legacy | sed 's#^legacy/##' | grep -v '^HEAD$')
fi
start=$(git branch --show-current)
status=0
for b in "${branches[@]}"; do
  if ! git rev-parse -q --verify "origin/$b" >/dev/null; then
    echo "NEW   $b: exists only in legacy; create it here from legacy/$b (trees are shared, history is not): git push origin legacy/$b:refs/heads/$b"
    [ $push -eq 1 ] && git push origin "legacy/$b:refs/heads/$b"
    continue
  fi
  # Commits on legacy/<b> whose patch does not appear on origin/<b>.
  missing=$(git cherry "origin/$b" "legacy/$b" | awk '$1=="+"{print $2}')
  if [ -z "$missing" ]; then
    continue
  fi
  echo "PORT  $b: $(echo "$missing" | wc -l | tr -d ' ') commit(s) only in legacy"
  echo "$missing" | while read -r sha; do git log -1 --format='        %h %s' "$sha"; done
  if [ $push -eq 1 ]; then
    git switch -q -C "sync/$b" "origin/$b"
    if git cherry-pick -x $missing; then
      git push -q origin "sync/$b:refs/heads/$b"
      echo "      pushed to origin/$b"
    else
      echo "      CONFLICT on $b: resolve in branch sync/$b, then: git push origin sync/$b:refs/heads/$b" >&2
      git cherry-pick --abort || true
      status=1
    fi
  fi
done
[ -n "$start" ] && git switch -q "$start"
exit $status
