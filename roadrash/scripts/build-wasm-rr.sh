#!/bin/bash
# Build the Road Rash wasm + JS bindings (toolchain kept inside ~/theseus-roadrash/tools).
set -e
export RUSTUP_HOME=~/theseus-roadrash/tools/rustup CARGO_HOME=~/theseus-roadrash/tools/cargo
link_args=""
for a in --shared-memory --max-memory=1073741824 --import-memory --export=__heap_base --export=__wasm_init_tls --export=__tls_size --export=__tls_align --export=__tls_base; do link_args="$link_args -Clink-args=$a"; done
export RUSTFLAGS="-Ctarget-feature=+atomics$link_args"
cd ~/theseus-roadrash/theseus
s=$(date +%s)
~/.cargo/bin/cargo +nightly build --lib --release -Z build-std=std,panic_abort --target wasm32-unknown-unknown -p roadrash
~/theseus-roadrash/tools/bindgen/bin/wasm-bindgen --out-dir web/exe/roadrash --typescript --target web --reference-types target/wasm32-unknown-unknown/release/roadrash.wasm
echo "wasm build: $(( $(date +%s)-s )) s; $(ls -la web/exe/roadrash/roadrash_bg.wasm | awk '{print $5}') bytes"
