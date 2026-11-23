#!/usr/bin/env bash
# Builds a static Linux binary locally in Docker. Usage: ./build-linux.sh [aarch64|x86_64]
set -euo pipefail

arch="${1:-aarch64}"
case "$arch" in
  aarch64) platform=linux/arm64 ;;
  x86_64) platform=linux/amd64 ;;
  *) echo "unknown arch: $arch (use aarch64 or x86_64)" >&2; exit 1 ;;
esac
target="$arch-unknown-linux-musl"

cd "$(dirname "$0")"
docker run --rm --platform "$platform" \
  -v "$PWD":/src -w /src \
  -v cpu-floor-cargo-registry:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR="/src/target/docker-$arch" \
  rust:1-bookworm \
  sh -c "rustup target add $target >/dev/null && cargo build --release --locked --target $target"

mkdir -p dist
cp "target/docker-$arch/$target/release/cpu-floor" "dist/cpu-floor-$arch-linux"
echo "built dist/cpu-floor-$arch-linux"
