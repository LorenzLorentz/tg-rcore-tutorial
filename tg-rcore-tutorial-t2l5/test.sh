#!/bin/bash
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
set -o pipefail

run_base() {
    echo "运行 t2l5 基础场景测试..."
    cargo clean
    echo -e "${YELLOW}────────── cargo run 输出 ──────────${NC}"
    local log_file
    log_file="$(mktemp)"

    if cargo run 2>&1 | tee "${log_file}" && grep -q '\[t2l5-summary\]' "${log_file}"; then
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${GREEN}✓ t2l5 基础场景测试通过${NC}"
        cargo clean
        rm -f "${log_file}"
        return 0
    else
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${RED}✗ t2l5 基础场景测试失败${NC}"
        cargo clean
        rm -f "${log_file}"
        return 1
    fi
}

run_success() {
    echo "运行 t2l5 success cases..."
    "$SCRIPT_DIR/verify.sh" --mode success
}

run_control() {
    echo "运行 t2l5 control cases..."
    "$SCRIPT_DIR/verify.sh" --mode control
}

run_all() {
    run_base
    echo ""
    "$SCRIPT_DIR/verify.sh" --mode all
}

case "${1:-all}" in
    base)
        run_base
        ;;
    success)
        run_success
        ;;
    control)
        run_control
        ;;
    all)
        run_all
        ;;
    *)
        echo "用法: $0 [base|success|control|all]"
        exit 1
        ;;
esac
