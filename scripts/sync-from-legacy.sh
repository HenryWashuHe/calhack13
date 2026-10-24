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
  # macOS ships bash 3.2 (no mapfile).
  while IFS= read -r line; do branches+=("$line"); done < <(git for-each-ref --format='%(refname:short)' refs/remotes/legacy | sed 's#^legacy/##' | grep -v '^HEAD$' | grep -vx legacy)
fi
start=$(git branch --show-current)
status=0
for b in "${branches[@]}"; do
  if ! git rev-parse -q --verify "origin/$b" >/dev/null; then
    # A branch that exists only in legacy must not be pushed by SHA: its history
    # is the legacy history. Find its fork point against the legacy branch it
    # diverged from least, map that fork point to the commit here with the same
    # tree, and cherry-pick only the branch's own commits onto it.
    best=""; bestn=999999; bestmb=""
    for base in $(git for-each-ref --format='%(refname:short)' refs/remotes/legacy | sed 's#^legacy/##' | grep -v "^HEAD$" | grep -vx "$b"); do
      mb=$(git merge-base "legacy/$b" "legacy/$base" 2>/dev/null) || continue
      n=$(git rev-list --count "$mb..legacy/$b")
      if [ "$n" -lt "$bestn" ]; then bestn=$n; best=$base; bestmb=$mb; fi
    done
    if [ -z "$best" ] || ! git rev-parse -q --verify "origin/$best" >/dev/null; then
      echo "NEW   $b: no mapped fork point here (closest legacy base: ${best:-none}); port by hand" >&2
      status=1; continue
    fi
    tree=$(git rev-parse "$bestmb^{tree}")
    anchor=$(git log --format='%H %T' "origin/$best" | awk -v t="$tree" '$2==t{print $1; exit}')
    if [ -z "$anchor" ]; then
      echo "NEW   $b: fork point $bestmb (from $best) has no same-tree commit here; port by hand" >&2
      status=1; continue
    fi
    own=$(git rev-list --reverse "$bestmb..legacy/$b")
    echo "NEW   $b: $bestn commit(s) from fork point of legacy/$best, anchored at ${anchor:0:7} here"
    echo "$own" | while read -r sha; do git log -1 --format='        %h %s' "$sha"; done
    if [ $push -eq 1 ]; then
      git switch -q -C "sync/$b" "$anchor"
      if git cherry-pick -x $own; then
        git push -q origin "sync/$b:refs/heads/$b" && echo "      created origin/$b"
      else
        echo "      CONFLICT creating $b: resolve in branch sync/$b, then: git push origin sync/$b:refs/heads/$b" >&2
        git cherry-pick --abort || true; status=1
      fi
    fi
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
