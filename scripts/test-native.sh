#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $(uname -s) != Darwin ]]; then
  echo 'Native control tests require macOS.' >&2
  exit 1
fi
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
swiftc -swift-version 6 -warnings-as-errors \
  native/VentPositionControl.swift native/FanSpeedDisplay.swift native/VentSelectionMemory.swift \
  tests/native/ControlsTests.swift -o "$scratch/controls-tests"
"$scratch/controls-tests"
