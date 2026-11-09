#!/bin/sh
# usage: build.sh [OUT]   builds the macOS text recognition helper (kaku-ocr)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/../../target/debug/kaku-ocr"}
mkdir -p "$(dirname "$out")"
swiftc -O "$here/kaku-ocr.swift" -o "$out"
echo "$out"
