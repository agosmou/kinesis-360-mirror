#!/bin/sh
# Regenerate the README screenshots.
#
# Renders the real frontend in headless Chrome rather than capturing a desktop
# window, so the images are reproducible and free of desktop clutter. The
# ?demo= parameter lights keys without a keyboard attached.
set -e

CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
[ -x "$CHROME" ] || { echo "Google Chrome not found at $CHROME"; exit 1; }

PORT=8791
OUT=docs
mkdir -p "$OUT"

python3 -m http.server "$PORT" --bind 127.0.0.1 --directory src >/dev/null 2>&1 &
SERVER=$!
trap 'kill $SERVER 2>/dev/null' EXIT
sleep 1

# Height derived from the board's own 2.28 aspect plus the status row, so the
# capture is snug instead of letterboxed: (1180-32)/2.28 + 24 + 6 + 20.
shot() {
  name=$1; demo=$2
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars \
    --force-device-scale-factor=2 --window-size=1180,554 \
    --screenshot="$OUT/$name.png" \
    "http://127.0.0.1:$PORT/index.html?demo=$demo" >/dev/null 2>&1
  echo "wrote $OUT/$name.png"
}

# Base layer, mid-chord: left shift + A, a right-hand key, and a thumb key.
shot board "E1,04,0F,2C"

# F5 (0x3E) only exists on the Fn layer, so this also demonstrates the layer
# inference: legends repaint and the badge picks up its "?".
shot layer-fn "3E"

# A keypad usage (KP_7) flips the display to the Kp layer the same way.
shot layer-keypad "5F"
