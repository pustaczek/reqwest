#!/usr/bin/env bash

wasm-pack build -t nodejs

# Prepend node-fetch imports
cat ext-import.js pkg/wasm_test.js | sponge pkg/wasm_test.js
# Replace annoying Window type checking
sed -i 's/getObject(arg0) instanceof Window;/true; \/\/ A PATCH APPLIED BY BUILD\.SH/mg' pkg/wasm_test.js
