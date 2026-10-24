#!/bin/bash
# checks.sh <out_dir>: the headless check suite, each example's output saved to <out_dir>/<name>.txt
# with its timings stripped, so two runs compare exactly (tools/checks_diff.sh <a> <b>).
#
# Builds the examples first (CHECKS_NO_BUILD=1 skips that), then runs them CHECKS_JOBS at a time
# (default 4). The runs use a scratch APPDATA under <out_dir>, never the user's own. The Kenshi
# install is KENSHI_INSTALL, else Steam's. A run that fails or times out (CHECKS_TIMEOUT
# seconds, default 1800) says so on its last line ("exit N").
# The baseline of main is in tools/checks_baseline/.
out=$1
[ -z "$out" ] && { echo "usage: checks.sh <out_dir>"; exit 1; }
mkdir -p "$out" || exit 1
out=$(cd "$out" && pwd)
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo" || exit 1
inst=${KENSHI_INSTALL:-"C:/Program Files (x86)/Steam/steamapps/common/Kenshi"}
jobs=${CHECKS_JOBS:-4}
limit=${CHECKS_TIMEOUT:-1800}
ex="${CARGO_TARGET_DIR:-$repo/target}/release/examples"
examples="sim_bench task_census world_check carry_trip crawl_trip fights haul_trip feed_trip jobs_trip idle_trip farm_trip law_test save_trip old_save"

if [ -z "$CHECKS_NO_BUILD" ]; then
  args=""
  for e in $examples; do args="$args --example $e"; done
  cargo build --release -p kenshi-game $args > "$out/.build.log" 2>&1 || { tail -30 "$out/.build.log"; echo "BUILD FAILED"; exit 2; }
fi

# a scratch APPDATA (saves, options), fresh each time
appdata="$out/.appdata"
rm -rf "$appdata"
mkdir -p "$appdata"
APPDATA=$(cygpath -w "$appdata" 2>/dev/null || echo "$appdata")
export APPDATA
# a scratch TEMP too (save_trip writes its save there)
mkdir -p "$out/.tmp"
TEMP=$(cygpath -w "$out/.tmp" 2>/dev/null || echo "$out/.tmp")
TMP=$TEMP
export TEMP TMP
# the debug switches the runs print with, and nothing else of the caller's
for v in $(env | grep -o "^KENSHI_[A-Z_]*" | grep -v "^KENSHI_INSTALL$"); do unset "$v"; done
unset BENCH_TRACE BENCH_DUMP BENCH_DUMP_SAVE BENCH_TOWNS CENSUS_IDLE CENSUS_STALLED CENSUS_OFFMESH CENSUS_TRACE_TICKS

# Lines and fields that are wall-clock timings, not results.
strip() {
  sed -E \
    -e '/^\s*(Finished|Running|Compiling|Blocking) /d' \
    -e 's/[0-9]+(\.[0-9]+)? ?(ms|µs|us)\/step/~ ms\/step/g' \
    -e 's/[0-9]+(\.[0-9]+)? ?(ms|µs|us)\b/~ ms/g' \
    -e 's/\b[0-9]+(\.[0-9]+)? s wall/~ s wall/g' \
    -e 's/(loaded|took|in|wall|elapsed|real)( time)?:? [0-9]+(\.[0-9]+)? ?s\b/\1 ~ s/g' \
    -e 's/[0-9]+(\.[0-9]+)?x real ?time/~x real time/g' \
    -e 's/[0-9]+(\.[0-9]+)? steps\/s/~ steps\/s/g'
}

# run <name> <env...> -- <exe> <args...>
run() {
  name=$1; shift
  envs=""
  while [ "$1" != "--" ]; do envs="$envs $1"; shift; done
  shift
  exe=$1; shift
  (
    start=$(date +%s)
    env $envs timeout "$limit" "$ex/$exe.exe" "$@" > "$out/$name.raw" 2>&1
    code=$?
    strip < "$out/$name.raw" > "$out/$name.txt"
    echo "exit $code" >> "$out/$name.txt"
    echo "$name: exit $code, $(( $(date +%s) - start )) s"
  ) &
  # no more than $jobs at once
  while [ "$(jobs -p | wc -l)" -ge "$jobs" ]; do sleep 2; done
}

run sim_bench_hub -- sim_bench "$inst" 20 32 120
run task_census_hub -- task_census "$inst" 20 32
run task_census_clownsteady -- task_census "$inst" 18 51
run task_census_stack -- task_census "$inst" 19 29
run world_check -- world_check "$inst" 22 30 300
run carry_trip -- carry_trip "$inst" 37 35
run crawl_trip -- crawl_trip "$inst" 37 35
run fights_fog_raid -- fights "$inst" 34 46 120 "squad:Fog raid@1"
run haul_trip KENSHI_JOBS_DEBUG=1 -- haul_trip "$inst" 21 42 Well
run feed_trip KENSHI_JOBS_DEBUG=1 -- feed_trip "$inst" 21 42
run jobs_trip KENSHI_JOBS_DEBUG=1 -- jobs_trip "$inst" 21 42
run idle_trip KENSHI_JOBS_DEBUG=1 -- idle_trip "$inst" 21 42
run farm_trip KENSHI_BUILD_DEBUG=1 -- farm_trip "$inst" 21 42
run law_test KENSHI_LAW_DEBUG=1 -- law_test "$inst" 24 27
run save_trip -- save_trip "$inst" 20 32 60
# a save written by v0.3.0-alpha (4a92f09) loads, and a round trip keeps its counts
run old_save -- old_save "$inst" "$repo/src/game/tests/fixtures/v0.3.0-alpha.ron"
wait
rm -rf "$out"/*.raw "$out/.appdata" "$out/.tmp" "$out/.build.log"
echo "checks in $out"
