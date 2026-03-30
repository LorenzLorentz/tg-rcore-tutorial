#!/bin/bash
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
set -o pipefail

run_base() {
    echo "运行 t2l4 基础场景测试..."
    cargo clean
    echo -e "${YELLOW}────────── cargo run 输出 ──────────${NC}"
    local log_file
    log_file="$(mktemp)"

    if cargo run 2>&1 | tee "${log_file}" && grep -q '\[t2l4-summary\]' "${log_file}"; then
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${GREEN}✓ t2l4 基础场景测试通过${NC}"
        cargo clean
        rm -f "${log_file}"
        return 0
    else
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${RED}✗ t2l4 基础场景测试失败${NC}"
        cargo clean
        rm -f "${log_file}"
        return 1
    fi
}

run_trace() {
    echo "运行 t2l4 trace 场景测试..."
    cargo clean
    echo -e "${YELLOW}────────── T2L4_TRACE=1 cargo run 输出 ──────────${NC}"
    local log_file
    log_file="$(mktemp)"

    if T2L4_TRACE=1 cargo run 2>&1 | tee "${log_file}" && grep -q '\[t2l4-trace\]' "${log_file}"; then
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${GREEN}✓ t2l4 trace 场景测试通过${NC}"
        cargo clean
        rm -f "${log_file}"
        return 0
    else
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${RED}✗ t2l4 trace 场景测试失败${NC}"
        cargo clean
        rm -f "${log_file}"
        return 1
    fi
}

run_suite() {
    echo "运行 t2l4 完整调度矩阵..."
    "$SCRIPT_DIR/verify.sh"
}

case "${1:-all}" in
    base)
        run_base
        ;;
    trace)
        run_trace
        ;;
    suite)
        run_suite
        ;;
    all)
        run_base
        echo ""
        run_suite
        ;;
    *)
        echo "用法: $0 [base|trace|suite|all]"
        exit 1
        ;;
esac
