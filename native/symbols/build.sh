#!/bin/sh
# Build the SF Symbols helper into native/symbols/build/lotse-symbols (Apple Silicon, macOS 13+).
# Optional for Alpha: without it the UI keeps the Lucide icons.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$here/build"
swiftc -O -target arm64-apple-macos13 -o "$here/build/lotse-symbols" "$here/main.swift"
echo "$here/build/lotse-symbols"
