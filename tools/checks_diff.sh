#!/bin/bash
# checks_diff.sh <a_dir> <b_dir>: what changed between two runs of tools/checks.sh (each check's
# output, timings already stripped). Prints "no difference" when they are the same; exits 1
# when anything differs, 0 otherwise.
a=$1; b=$2
[ -d "$a" ] && [ -d "$b" ] || { echo "usage: checks_diff.sh <a_dir> <b_dir>"; exit 2; }
changed=0
names=$( (ls "$a"; ls "$b") | grep ".txt$" | sort -u)
for n in $names; do
  if [ ! -f "$a/$n" ]; then
    echo "== ${n%.txt}: only in $b"; changed=1; continue
  fi
  if [ ! -f "$b/$n" ]; then
    echo "== ${n%.txt}: only in $a"; changed=1; continue
  fi
  if ! cmp -s "$a/$n" "$b/$n"; then
    lines=$(diff "$a/$n" "$b/$n" | grep -c '^[<>]')
    echo "== ${n%.txt}: $lines lines differ"
    diff -U1 "$a/$n" "$b/$n" | tail -n +3 | head -${CHECKS_DIFF_LINES:-60}
    changed=1
  fi
done
[ $changed = 0 ] && echo "no difference"
exit $changed
