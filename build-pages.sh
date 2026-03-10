#!/usr/bin/env bash
# Build the WASM package and assemble docs/ for GitHub Pages.
set -euo pipefail

PROJ_ROOT="$(cd "$(dirname "$0")" && pwd)"
DOCS="$PROJ_ROOT/docs"

echo "==> Building WASM package (web target)..."
wasm-pack build --target web "$PROJ_ROOT"

echo "==> Assembling docs/ for GitHub Pages..."
mkdir -p "$DOCS/pkg"

# Copy wasm-pack output (only the files the browser needs)
cp "$PROJ_ROOT/pkg/jenkinsfile_tester.js"         "$DOCS/pkg/"
cp "$PROJ_ROOT/pkg/jenkinsfile_tester_bg.wasm"    "$DOCS/pkg/"

# Copy index.html, rewriting the import path from ../pkg/ to ./pkg/
sed "s|from '../pkg/|from './pkg/|g" "$PROJ_ROOT/demo/index.html" > "$DOCS/index.html"

echo "==> Done. docs/ is ready for GitHub Pages."
echo "    docs/index.html"
echo "    docs/pkg/jenkinsfile_tester.js"
echo "    docs/pkg/jenkinsfile_tester_bg.wasm"
