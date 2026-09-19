#!/usr/bin/env bash
#
# Local mirror of .github/workflows/crates-code-quality.yml
#
# Runs the same steps, in the same order, with the same flags as CI. If this
# script passes, the Rust quality pipeline should pass on push/PR too (modulo
# OS/toolchain differences — CI additionally tests stable+beta on all 3 OSes).
#
# Usage:
#   ./scripts/check-pipeline.sh            # stop at the first failing step
#   ./scripts/check-pipeline.sh -c         # run every step, report all failures
#
set -u
cd "$(dirname "$0")/.."

CONTINUE_ON_FAIL=0
case "${1:-}" in
-c | --continue)
    CONTINUE_ON_FAIL=1
    shift
    ;;
esac

if [[ -n "${1:-}" ]]; then
    echo "usage: $0 [-c|--continue]" >&2
    exit 2
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

# Steps exactly as defined in .github/workflows/crates-code-quality.yml.
# Note: CI runs `cargo test` only after fmt+clippy pass (needs: [fmt, clippy]);
# sequential fail-fast mode replicates that.
steps=(
    "fmt|cargo fmt --all -- --check"
    "clippy|cargo clippy --all-targets --all-features -- -D warnings"
    "build|cargo build --all-features"
    "test|cargo test"
)

declare -a failed=()
declare -a passed=()

start_all=$SECONDS
for entry in "${steps[@]}"; do
    name="${entry%%|*}"
    cmd="${entry#*|}"

    echo -e "${BOLD}==> $name${NC}: $cmd"
    start=$SECONDS

    if eval "$cmd"; then
        passed+=("$name")
        echo -e "${GREEN}==> $name passed${NC} ($((SECONDS - start))s)"
    else
        failed+=("$name")
        echo -e "${RED}==> $name FAILED${NC}"
        if [[ $CONTINUE_ON_FAIL -eq 0 ]]; then
            echo -e "${YELLOW}Stopping (fix this, then re-run; use -c to run all steps)${NC}"
            break
        fi
    fi
    echo
done
total=$((SECONDS - start_all))

echo -e "${BOLD}── Summary (total ${total}s) ──${NC}"
for name in "${passed[@]}"; do
    echo -e "  ${GREEN}✔${NC} $name"
done
for name in "${failed[@]}"; do
    echo -e "  ${RED}✘${NC} $name"
done

if [[ ${#failed[@]} -eq 0 ]]; then
    echo -e "${GREEN}Pipeline would pass.${NC}"
    exit 0
else
    echo -e "${RED}Pipeline would fail (${failed[*]}).${NC}"
    exit 1
fi
