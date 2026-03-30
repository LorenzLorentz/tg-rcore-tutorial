#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

export RUSTUP_SKIP_UPDATE_CHECK="${RUSTUP_SKIP_UPDATE_CHECK:-1}"

python3 "$SCRIPT_DIR/scripts/run_suite.py" "$@"
