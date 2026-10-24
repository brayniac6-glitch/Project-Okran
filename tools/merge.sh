#!/bin/bash
# merge.sh <branch> "<merge title>" [--no-checks]
#
# The safe merge into main: the branch is merged without committing, then checked; the merge is
# committed only when everything passes, and aborted otherwise, so main never holds an
# unverified merge.
#   1. git merge --no-ff --no-commit (stops on a conflict)
#   2. the KCD check on what the merge brings in
#   3. cargo build --release, then the workspace's lib tests (each retried once on a transient
#      error: a rustc crash, "failed to mmap", a locked file)
#   4. tools/checks.sh into a fresh folder, and its diff against tools/checks_baseline/, printed
#      for the lead to judge (skipped with --no-checks, for doc-only merges)
#   5. commit, push, refresh dist/ (unless the game from dist/ is running)
# MERGE_REPO overrides the repository (for testing the script on a scratch clone).
# Exit codes: 1 KCD hit / bad use, 2 conflicts, 3 build failed, 4 tests failed, 5 checks failed
# to run. A check diff does not stop the merge by itself: the lead reads it.
repo=${MERGE_REPO:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$repo" || exit 1
b=$1; title=$2; checks=1
[ "$3" = "--no-checks" ] && checks=0
[ -z "$b" ] || [ -z "$title" ] && { echo "usage: merge.sh <branch> \"<title>\" [--no-checks]"; exit 1; }
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || { echo "not on main"; exit 1; }
[ -n "$(git status --porcelain --untracked-files=no)" ] && { echo "main has uncommitted changes"; exit 1; }
n=$(git rev-list --count main..$b)
echo "== $b: $n commits"
[ "$n" = "0" ] && { echo "nothing to merge"; exit 0; }
log=/tmp/merge.out
abort() { git merge --abort 2>/dev/null; echo "merge aborted; main left at $(git log --oneline -1)"; exit $1; }

# 1. merge, uncommitted
git merge --no-ff --no-commit $b > "$log" 2>&1
u=$(git diff --name-only --diff-filter=U)
[ -n "$u" ] && { echo "CONFLICTS: $u"; abort 2; }
git rev-parse -q --verify MERGE_HEAD > /dev/null || { cat "$log"; echo "no merge in progress"; exit 1; }

# 2. the KCD check, on what the merge brings in
# The material terms only: this script's own words for the check ("KCD check", "KCD hit") are
# taken out first, and the terms are spelt in pieces here, so the script in a diff does not hit
# itself; hkcd (Havok) is not a hit.
terms="k""cd|king""dom.?come|wh""game|war""horse|rad""zig|wh_""cs_|rpg_""param"
k=$(git diff --cached main | sed -E 's/k''cd (check|hit)//Ig' | grep -i -E "$terms" | grep -v -i "hk""cd" | head -3)
[ -n "$k" ] && { echo "KCD CHECK HIT:"; echo "$k"; abort 1; }

# 3. build and lib tests, each retried once on a transient error
transient="failed to mmap|could not compile .* \(signal|STATUS_ACCESS_VIOLATION|STATUS_STACK_BUFFER_OVERRUN|rustc .* crashed|internal compiler error|being used by another process|Access is denied. \(os error 5\)|os error 32|failed to remove file|failed to rename"
build() { cargo build --release > "$log.build" 2>&1; }
build
if [ $? -ne 0 ] && grep -q -E "$transient" "$log.build"; then
  echo "build: transient error, retrying once"; build
fi
e=$(grep -E "^error" -A8 "$log.build" | head -20)
[ -n "$e" ] && { echo "BUILD FAILED:"; echo "$e"; abort 3; }
tests() { cargo test --release --workspace --lib > "$log.test" 2>&1; }
tests
if grep -q -E "$transient" "$log.test"; then
  echo "tests: transient error, retrying once"; tests
fi
t=$(grep -E "test result|FAILED|panicked|^error" "$log.test" | grep -v "ok. 0 passed" | grep -E "FAILED|panicked|^error" | head -5)
[ -n "$t" ] && { echo "TESTS FAILED:"; echo "$t"; abort 4; }
echo "build and tests ok"

# 4. the checks, diffed against the baseline
if [ $checks = 1 ]; then
  out=/tmp/merge-checks-$(date +%s)
  bash tools/checks.sh "$out" > "$log.checks" 2>&1 || { tail -20 "$log.checks"; echo "CHECKS FAILED TO RUN"; abort 5; }
  echo "== checks against the baseline (tools/checks_baseline):"
  bash tools/checks_diff.sh tools/checks_baseline "$out"
  echo "== (checks in $out)"
fi

# 5. commit, push, dist
git -c user.email=335742381+brayniac6-glitch@users.noreply.github.com commit -q -m "$title" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" || abort 1
git push -q origin main 2>&1 | tail -1
powershell -NoProfile -Command "\$r = Get-Process kenshi -ErrorAction SilentlyContinue | Where-Object { \$_.Path -like '*kenshi-rewrite\dist*' }; if (\$r) { 'dist running, not copied' } else { Copy-Item target\release\kenshi.exe dist\kenshi.exe -Force; 'dist refreshed' }"
git log --oneline -1
