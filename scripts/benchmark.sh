#!/usr/bin/env bash
# Reproducible performance numbers for Ferrum on large public repositories.
#
#   scripts/benchmark.sh [--runs N] [--no-gui] [REPO...]
#
# REPO is a known name (git, linux, rust, react) or a path to a repository.
# Default: git. Known repositories are cloned (full history) into
# $FERRUM_BENCH_DIR (default target/bench-repos) on the first run.
#
# For each repository it reports:
#   - size: commits, refs, tracked files
#   - `ferrum --benchmark`: every loading step (open, refs, first page of
#     commits, graph layout, status...), then the whole history (scrolling to
#     the end), a path search and a text search; best and average of N runs
#   - peak memory of that load (max RSS, from /usr/bin/time)
#   - GUI start: launch until the window has drawn the loaded history and
#     saved a screenshot (FERRUM_SCREENSHOT), wall time and max RSS; needs a
#     display, skipped with --no-gui or when there is none
#
# The results go to stdout and to target/bench/RESULTS-<name>.md.
# Needs: git, /usr/bin/time (GNU time), a release build (built if missing).
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD

runs=5
gui=1
repos=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --runs) runs=$2; shift 2 ;;
        --no-gui) gui=0; shift ;;
        -h | --help) sed -n '2,23p' "$0"; exit 0 ;;
        *) repos+=("$1"); shift ;;
    esac
done
[[ ${#repos[@]} -gt 0 ]] || repos=(git)

ferrum=${FERRUM:-$root/target/release/ferrum}
if [[ ! -x $ferrum ]]; then
    if command -v dev >/dev/null; then dev rust cargo build --release --locked -p ferrum
    else cargo build --release --locked -p ferrum; fi
fi
[[ -x /usr/bin/time ]] || { echo "needs GNU time (/usr/bin/time)" >&2; exit 1; }

bench_dir=${FERRUM_BENCH_DIR:-$root/target/bench-repos}
out_dir=$root/target/bench
mkdir -p "$bench_dir" "$out_dir"

url_of() {
    case $1 in
        git) echo https://github.com/git/git.git ;;
        linux) echo https://github.com/torvalds/linux.git ;;
        rust) echo https://github.com/rust-lang/rust.git ;;
        react) echo https://github.com/facebook/react.git ;;
        *) return 1 ;;
    esac
}

# Prints "seconds max_rss_kb" for a command.
measure() {
    local f
    f=$(mktemp)
    /usr/bin/time -f '%e %M' -o "$f" "$@" >/dev/null 2>&1 || true
    tail -1 "$f"
    rm -f "$f"
}

median() { sort -n | awk '{a[NR]=$1} END {print (NR%2 ? a[(NR+1)/2] : (a[NR/2]+a[NR/2+1])/2)}'; }

for spec in "${repos[@]}"; do
    if url=$(url_of "$spec"); then
        name=$spec
        path=$bench_dir/$name
        if [[ ! -d $path/.git ]]; then
            echo "Cloning $url (full history; this can take a while)..." >&2
            git clone --quiet "$url" "$path"
        fi
    else
        path=$(cd "$spec" && pwd)
        name=$(basename "$path")
    fi
    report=$out_dir/RESULTS-$name.md
    {
        echo "## $name"
        echo
        echo "- Date: $(date -u +%Y-%m-%d)"
        echo "- Ferrum: $(git -C "$root" describe --always --dirty 2>/dev/null || echo unknown)"
        echo "- Machine: $(uname -sm), $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //' || echo unknown CPU), $(awk '/MemTotal/ {printf "%.0f GB RAM", $2/1048576}' /proc/meminfo 2>/dev/null)"
        echo "- Repository: $(git -C "$path" remote get-url origin 2>/dev/null || echo "$path") at $(git -C "$path" rev-parse --short HEAD)"
        echo "- Size: $(git -C "$path" rev-list --count --all) commits, $(git -C "$path" for-each-ref | wc -l) refs, $(git -C "$path" ls-files | wc -l) tracked files"
        echo
        echo '```'
        "$ferrum" --benchmark "$path" "$runs" 2>/dev/null | grep -v '^Repository:'
        echo '```'
        echo
        read -r _ rss < <(measure "$ferrum" --benchmark "$path" 1)
        echo "- Peak memory of a full load (\`--benchmark\`, 1 run): $((rss / 1024)) MB"
        if [[ $gui == 1 && -n ${DISPLAY:-}${WAYLAND_DISPLAY:-} ]]; then
            times=() mems=()
            for _ in $(seq "$runs"); do
                data=$(mktemp -d)
                read -r t m < <(measure env FERRUM_DATA_DIR="$data" FERRUM_NO_UPDATE_CHECK=1 \
                    FERRUM_SCREENSHOT="$data/shot.ppm" FERRUM_SCREENSHOT_FRAMES=2 "$ferrum" "$path")
                rm -rf "$data"
                times+=("$t") mems+=("$m")
            done
            t=$(printf '%s\n' "${times[@]}" | median)
            m=$(printf '%s\n' "${mems[@]}" | median)
            echo "- GUI start to loaded history on screen (median of $runs): ${t} s, $((${m%.*} / 1024)) MB max RSS"
        else
            echo "- GUI start: skipped (no display or --no-gui)"
        fi
        echo
    } | tee "$report"
done
