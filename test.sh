#!/usr/bin/env bash
# test.sh — force-rebuild integration test artifacts and run all tests.
#
# Cargo's change-detection fingerprinting is unreliable on the Windows NTFS
# drive mounted at /mnt/c/. Deleting the stale integration test binary before
# each run guarantees a fresh compile and prevents "tests appear to pass but
# are running old code" false-positives.
#
# Usage:
#   ./test.sh              # run all tests
#   ./test.sh <filter>     # run only tests whose name contains <filter>
#                          # e.g.  ./test.sh parses_matrix

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "==> Removing stale integration test artifacts..."
rm -f target/debug/deps/integration-*

if [[ $# -gt 0 ]]; then
    echo "==> Running tests matching: $1"
    cargo test "$1" 2>&1
else
    echo "==> Running all tests..."
    cargo test 2>&1
fi

echo "==> Done."
