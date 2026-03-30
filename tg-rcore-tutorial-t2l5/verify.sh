#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [[ "${1:-}" != "--inner" ]] && [[ ! -f "/.dockerenv" ]]; then
    if [[ ! -f "$HOME/rcore_docker.sh" ]]; then
        echo "missing ~/rcore_docker.sh"
        exit 1
    fi
    cd "$REPO_ROOT"
    exec bash "$HOME/rcore_docker.sh" bash -lc "cd /workspace/tg-rcore-tutorial-t2l5 && ./verify.sh --inner ${*:1}"
fi

if [[ "${1:-}" == "--inner" ]]; then
    shift
fi

if [[ -d /workspace/.cargo-home/bin ]]; then
    export PATH="/workspace/.cargo-home/bin:$PATH"
fi
if [[ -d /workspace/.cargo-home ]]; then
    export CARGO_HOME="${CARGO_HOME:-/workspace/.cargo-home}"
fi
if [[ -d /workspace/.rustup-home ]]; then
    export RUSTUP_HOME="${RUSTUP_HOME:-/workspace/.rustup-home}"
fi
export RUSTUP_SKIP_UPDATE_CHECK="${RUSTUP_SKIP_UPDATE_CHECK:-1}"

python3 "$SCRIPT_DIR/scripts/run_suite.py" "$@"
