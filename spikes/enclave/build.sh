#!/bin/sh
# usage: build.sh [OUT]        the shipped helper (always shows the native dialog)
#        build.sh --test [OUT] test build: the dialog is replaced by an automatic decision. Never ship it.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
if [ "${1:-}" = "--test" ]; then
  out=${2:-"$here/../../target/debug/kaku-enclave-test"}
  mkdir -p "$(dirname "$out")"
  swiftc -O -D KAKU_TEST_AUTOCONFIRM "$here/kaku-enclave.swift" -o "$out"
else
  out=${1:-"$here/../../target/debug/kaku-enclave"}
  mkdir -p "$(dirname "$out")"
  swiftc -O "$here/kaku-enclave.swift" -o "$out"
fi
echo "$out"
