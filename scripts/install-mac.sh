#!/bin/sh
# Build, install to /Applications, relaunch.
#
# Installing to a stable path matters: it keeps the granted app and the app you
# just built from drifting apart, and survives `cargo clean`.
set -e

# Local signing identity, gitignored — see README. Without it the build is
# ad-hoc signed, and macOS re-asks for Input Monitoring on every single
# rebuild because TCC identifies ad-hoc apps by a CDHash that always changes.
if [ -f .signing.env ]; then
  . ./.signing.env
else
  echo "warning: no .signing.env — building ad-hoc signed."
  echo "         expect to re-grant Input Monitoring after every build."
fi

bun run tauri build --debug --bundles app
rm -rf /Applications/kinesis-360-mirror.app
cp -R src-tauri/target/debug/bundle/macos/kinesis-360-mirror.app /Applications/
open /Applications/kinesis-360-mirror.app
