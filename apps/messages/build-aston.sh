#!/bin/sh
# Cross-build for the Ace 3 (Arch Linux ARM, glibc 2.43). Links against the
# phone-matched sysroot so no newer glibc symbols from the host leak in.
set -eu
cd "$(dirname "$0")"
sysroot=/mnt/development/denial-ace3/cross/sysroot
target=aarch64-unknown-linux-gnu

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# gcc searches its own /usr/aarch64-linux-gnu (glibc 2.44) before --sysroot;
# -B puts the sysroot first for startfiles and libraries.
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C link-arg=--sysroot=$sysroot -C link-arg=-B$sysroot/lib -C link-arg=-Wl,-rpath-link,$sysroot/usr/lib -C link-arg=-Wl,-rpath-link,$sysroot/lib"
export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CFLAGS_aarch64_unknown_linux_gnu="--sysroot=$sysroot"
export PKG_CONFIG_ALLOW_CROSS=1
# The sysroot has libfontconfig.so.1 but no fontconfig.pc; load it at runtime.
export RUST_FONTCONFIG_DLOPEN=1
export PKG_CONFIG_SYSROOT_DIR="$sysroot"
export PKG_CONFIG_LIBDIR="$sysroot/usr/lib/pkgconfig:$sysroot/usr/share/pkgconfig"

cargo build --release --target "$target"
