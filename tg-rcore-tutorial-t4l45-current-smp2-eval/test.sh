#!/bin/bash
set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
set -o pipefail

run_base() {
    echo "运行 t4l45 基础启动测试..."
    cargo clean
    echo -e "${YELLOW}────────── cargo run 输出 ──────────${NC}"
    local log_file
    log_file="$(mktemp)"

    if cargo run 2>&1 | tee "${log_file}" && grep -q '\[t4l45-sched-summary\]' "${log_file}"; then
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${GREEN}✓ t4l45 基础启动测试通过${NC}"
        cargo clean
        rm -f "${log_file}"
        return 0
    else
        echo ""
        echo -e "${YELLOW}────────── 测试结果 ──────────${NC}"
        echo -e "${RED}✗ t4l45 基础启动测试失败${NC}"
        cargo clean
        rm -f "${log_file}"
        return 1
    fi
}

run_scheduler() {
    echo "运行 t4l45 调度矩阵..."
    "$SCRIPT_DIR/verify.sh" --mode scheduler "$@"
}

run_sync() {
    echo "运行 t4l45 同步基础矩阵..."
    "$SCRIPT_DIR/verify.sh" --mode sync "$@"
}

run_control() {
    echo "运行 t4l45 同步对照矩阵..."
    "$SCRIPT_DIR/verify.sh" --mode control "$@"
}

run_robust() {
    echo "运行 t4l45 鲁棒性与性能矩阵..."
    "$SCRIPT_DIR/verify.sh" --mode robust "$@"
}

run_all() {
    run_base
    echo ""
    "$SCRIPT_DIR/verify.sh" --mode all "$@"
}

case "${1:-all}" in
    base)
        shift
        run_base "$@"
        ;;
    scheduler)
        shift
        run_scheduler "$@"
        ;;
    sync)
        shift
        run_sync "$@"
        ;;
    control)
        shift
        run_control "$@"
        ;;
    robust)
        shift
        run_robust "$@"
        ;;
    all)
        shift
        run_all "$@"
        ;;
    *)
        echo "用法: $0 [base|scheduler|sync|control|robust|all]"
        exit 1
        ;;
esac
