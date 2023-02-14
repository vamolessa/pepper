This is the web version of the editor.

## building
While on this directory, execute the following commands:
```
# RELEASE
cargo build --target=wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/pepper_web.wasm .

# DEBUG
cargo build --target=wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/debug/pepper_web.wasm .
```

### checking wasm output
```
scoop install wabt
wasm2wat target/wasm32-unknown-unknown/release/pepper_web.wasm
```
