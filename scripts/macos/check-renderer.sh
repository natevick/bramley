#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
features() {
  cargo tree -p bramley --target "$1" -e features || { echo "FAIL: cargo tree failed for $1" >&2; exit 1; }
}
linux="$(features x86_64-unknown-linux-gnu)"
mac="$(features aarch64-apple-darwin)"
has() { grep -qE "$2" <<<"$1"; }
status=0
fail() { echo "FAIL: $1" >&2; status=1; }
has "$linux" '^│?.*egui_glow v' || { echo "FAIL: the check is blind; the Linux build does not reach egui_glow" >&2; exit 1; }
has "$mac" 'wgpu-hal feature "metal"' || { echo "FAIL: the check is blind; the macOS build does not reach wgpu-hal's metal backend" >&2; exit 1; }
has "$linux" 'wgpu' && fail "the Linux build reaches wgpu"
has "$mac" 'egui_glow|glow v' && fail "the macOS build reaches glow"
has "$mac" 'macos-window-resize-jitter-fix' && fail "the macOS build enables macos-window-resize-jitter-fix"
has "$mac" 'wgpu(-hal|-core)? feature "(vulkan|gles|dx12|webgpu|webgl|angle|vulkan-portability)"' && fail "the macOS build enables a wgpu backend other than metal"
[ "$status" -eq 0 ] && echo "PASS: Linux has glow and no wgpu; macOS has wgpu with metal only, no glow and no resize-jitter fix"
exit "$status"
