#!/bin/sh
set -eu

TARGET_DIR=${TARGET_DIR:-target/riscv64gc-unknown-none-elf/debug}
MONITOR_SOCKET=${QEMU_MONITOR_SOCKET:-$TARGET_DIR/qemu-monitor.sock}

rm -f "$MONITOR_SOCKET"

exec qemu-system-riscv64 \
  -machine virt \
  -bios none \
  -serial stdio \
  -display vnc=127.0.0.1:1 \
  -monitor unix:"$MONITOR_SOCKET",server,nowait \
  -drive file="$TARGET_DIR/fs.img",if=none,format=raw,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
  -device virtio-gpu-device,bus=virtio-mmio-bus.1 \
  -device virtio-keyboard-device,bus=virtio-mmio-bus.2 \
  -kernel "$TARGET_DIR/wpj-tg-rcore-tutorial-t3l8-doom"
