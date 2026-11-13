#!/bin/sh
# usage: build.sh [OUT]   builds the macOS calendar and reminders reader (kaku-calendar)
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/../../target/debug/kaku-calendar"}
mkdir -p "$(dirname "$out")"
# the usage descriptions macOS shows when it asks for access are part of the program
swiftc -O "$here/kaku-calendar.swift" -o "$out" -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$here/Info.plist"
echo "$out"
