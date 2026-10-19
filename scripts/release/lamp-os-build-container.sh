#!/usr/bin/env bash
# Runs only inside the disposable ARM64 builder; no device or cloud credentials.
set -euo pipefail
cp -a /source /work-lampOS
mkdir /work-adapter
cp /adapter/Cargo.toml /adapter/Cargo.lock /work-adapter/
cp -a /adapter/src /work-adapter/src
cd /work-lampOS
cargo build --locked --release --target aarch64-unknown-linux-gnu --workspace --bins
cargo build --locked --release --target aarch64-unknown-linux-gnu --manifest-path /work-adapter/Cargo.toml
mkdir -p /out/binaries /out/lib
for binary in target/aarch64-unknown-linux-gnu/release/lamp-*; do
  if [ -f "$binary" ] && [ -x "$binary" ]; then
    cp "$binary" /out/binaries/
    ldd "$binary" >> /out/linked-libraries.txt
  fi
done
cp /work-adapter/target/aarch64-unknown-linux-gnu/release/lamp-os-adapter /out/service
ldd /assets/libonnxruntime.so.1.30.0 >> /out/linked-libraries.txt
if grep -q 'not found' /out/linked-libraries.txt; then
  echo 'A required native runtime library is missing' >&2; exit 1
fi
# glibc and the ELF loader are supplied by the supported Debian 12 host.
awk '/=> \/.* \(/ {print $3}' /out/linked-libraries.txt | sort -u | while read -r library; do
  case "$(basename "$library")" in
    libc.so.*|libm.so.*|libdl.so.*|libpthread.so.*|librt.so.*|ld-linux-*) continue ;;
  esac
  cp -L "$library" /out/lib/
done
rustc --version > /out/toolchain.txt
