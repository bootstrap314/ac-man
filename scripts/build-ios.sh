#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Requires Xcode with an iOS SDK, XcodeGen, and BoltFFI 0.30.1.
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
export IPHONEOS_DEPLOYMENT_TARGET=17.0
"${BOLTFFI:-boltffi}" --overlay boltffi.ios.toml pack apple --release --deny-skipped
xcodegen generate --spec ios/project.yml
xcodebuild -project ios/AcMan.xcodeproj -scheme AcManIOS \
  -destination "${IOS_DESTINATION:-generic/platform=iOS Simulator}" \
  -derivedDataPath "${IOS_BUILD_DIR:-dist/ios}" \
  "${1:-build}"
