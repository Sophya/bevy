#!/bin/bash

cargo build --release --example $@ --target wasm32-unknown-unknown

wasm-bindgen --out-name wasm_example \
  --out-dir examples/wasm/target \
  --target web target/wasm32-unknown-unknown/release/examples/$@.wasm && python3 -m http.server --directory examples/wasm





