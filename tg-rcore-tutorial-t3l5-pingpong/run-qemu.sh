#!/bin/sh
set -eu

kernel="$1"

if [ "${TG_PINGPONG_HEADLESS:-0}" = "1" ]; then
  exec qemu-system-riscv64 \
    -machine virt \
    -nographic \
    -bios none \
    -device virtio-gpu-device,bus=virtio-mmio-bus.0 \
    -device virtio-keyboard-device,bus=virtio-mmio-bus.1 \
    -kernel "$kernel"
fi

exec qemu-system-riscv64 \
  -machine virt \
  -bios none \
  -device virtio-gpu-device,bus=virtio-mmio-bus.0 \
  -device virtio-keyboard-device,bus=virtio-mmio-bus.1 \
  -serial stdio \
  -monitor none \
  -kernel "$kernel"
