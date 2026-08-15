#!/usr/bin/env python3
"""Generate layout.json + keymap.json from the official Adv360-Pro-ZMK config.

Reads the upstream devicetree sources so the on-screen board matches the real
firmware instead of a hand-drawn approximation:

  config/boards/arm/adv360/adv360-layouts.dtsi  -> physical key geometry
  config/adv360.keymap                          -> per-layer bindings

Usage:  python3 tools/gen_layout.py <path-to-Adv360-Pro-ZMK> [-o src/assets]
"""

import argparse
import json
import os
import re
import sys

# HID keyboard/keypad usage page (0x07) IDs for every keycode the stock keymap
# uses. Extend this table if you add keycodes to your own keymap.
USAGE = {
    "ENTER": 0x28, "ESC": 0x29, "BSPC": 0x2A, "TAB": 0x2B, "SPACE": 0x2C,
    "MINUS": 0x2D, "EQUAL": 0x2E, "LBKT": 0x2F, "RBKT": 0x30, "BSLH": 0x31,
    "SEMI": 0x33, "SQT": 0x34, "GRAVE": 0x35, "COMMA": 0x36, "DOT": 0x37,
    "FSLH": 0x38, "CAPS": 0x39,
    "PSCRN": 0x46, "SLCK": 0x47, "PAUSE": 0x48, "INS": 0x49,
    "HOME": 0x4A, "PG_UP": 0x4B, "DEL": 0x4C, "END": 0x4D, "PG_DN": 0x4E,
    "RIGHT": 0x4F, "LEFT": 0x50, "DOWN": 0x51, "UP": 0x52,
    "KP_NUM": 0x53, "KP_DIVIDE": 0x54, "KP_MULTIPLY": 0x55, "KP_MINUS": 0x56,
    "KP_PLUS": 0x57, "KP_ENTER": 0x58, "KP_N0": 0x62, "KP_DOT": 0x63,
    "KP_EQUAL": 0x67,
    "LCTRL": 0xE0, "LSHFT": 0xE1, "LALT": 0xE2, "LGUI": 0xE3,
    "RCTRL": 0xE4, "RSHFT": 0xE5, "RALT": 0xE6, "RGUI": 0xE7,
}
for _i in range(26):
    USAGE[chr(ord("A") + _i)] = 0x04 + _i
for _i in range(1, 10):
    USAGE["N%d" % _i] = 0x1E + _i - 1
    USAGE["KP_N%d" % _i] = 0x59 + _i - 1
USAGE["N0"] = 0x27
for _i in range(1, 13):
    USAGE["F%d" % _i] = 0x3A + _i - 1

# Display labels for keycodes whose ZMK name reads badly on a small key cap.
# House style: letters stay uppercase, named keys are lowercase words.
LABEL = {
    "EQUAL": "=", "MINUS": "-", "BSLH": "\\", "SEMI": ";", "SQT": "'",
    "GRAVE": "`", "COMMA": ",", "DOT": ".", "FSLH": "/", "LBKT": "[",
    "RBKT": "]", "SPACE": "space", "BSPC": "bksp", "PG_UP": "pgup",
    "PG_DN": "pgdn", "LSHFT": "shift", "RSHFT": "shift", "LCTRL": "ctrl",
    "RCTRL": "ctrl", "LALT": "alt", "RALT": "alt", "LGUI": "cmd",
    "RGUI": "cmd", "CAPS": "caps", "LEFT": "←", "RIGHT": "→",
    "UP": "↑", "DOWN": "↓", "ENTER": "↵", "KP_ENTER": "↵",
    "KP_DIVIDE": "/", "KP_MULTIPLY": "*", "KP_MINUS": "-", "KP_PLUS": "+",
    "KP_DOT": ".", "KP_EQUAL": "=", "KP_NUM": "num",
    "TAB": "tab", "ESC": "esc", "DEL": "del", "INS": "ins",
    "HOME": "home", "END": "end", "PSCRN": "prtsc", "SLCK": "sclk",
    "PAUSE": "pause",
}
# ZMK spells the digit row N1..N0; the cap just says 1..0.
for _d in range(10):
    LABEL["N%d" % _d] = str(_d)
    LABEL["KP_N%d" % _d] = str(_d)

# What is actually printed on the Adv360 keycaps, where it differs from the
# ZMK binding name. Read your board: the layer keys say "fn" and "Mod", the
# keypad toggle has a keypad glyph, and the four unassigned keys in the inner
# columns are numbered. Showing "mo 2" there means reading a different
# vocabulary on screen than the one under your fingers.
#
# Keyed by physical position, following the key order in adv360-layouts.dtsi:
# left row0 (0-6), right row0 (7-13), left row1 (14-20), right row1 (21-27),
# left row2 (28-34), thumbs (35-38), right row2 (39-45), ...
KEYCAP = {
    6: "keypad",   # left inner column, top  — &tog 1
    20: "①",       # left inner column, middle
    34: "②",       # left inner column, bottom
    7: "Mod",      # right inner column, top — &mo 3
    21: "③",       # right inner column, middle
    39: "④",       # right inner column, bottom
    60: "fn",      # bottom-left  — &mo 2
    75: "fn",      # bottom-right — &mo 2
}

KEY_ATTRS = re.compile(
    r"&key_physical_attrs\s+"
    r"(\(?-?\d+\)?)\s+(\(?-?\d+\)?)\s+(\(?-?\d+\)?)\s+"
    r"(\(?-?\d+\)?)\s+(\(?-?\d+\)?)\s+(\(?-?\d+\)?)\s+(\(?-?\d+\)?)"
)


def num(tok):
    """Devicetree writes negatives as (-1500)."""
    return int(tok.strip().lstrip("(").rstrip(")"))


def parse_layout(path):
    with open(path) as fh:
        src = fh.read()
    keys = []
    for i, m in enumerate(KEY_ATTRS.finditer(src)):
        w, h, x, y, r, rx, ry = (num(g) for g in m.groups())
        keys.append(
            {
                "i": i,
                # centi-units in the source; emit clean 1u-relative floats
                "x": x / 100.0, "y": y / 100.0,
                "w": w / 100.0, "h": h / 100.0,
                "r": r / 100.0, "rx": rx / 100.0, "ry": ry / 100.0,
            }
        )
    if not keys:
        sys.exit("no key_physical_attrs found in %s" % path)
    return keys


def strip_comments(src):
    src = re.sub(r"/\*.*?\*/", " ", src, flags=re.S)
    return re.sub(r"//[^\n]*", " ", src)


def split_bindings(blob):
    """Each binding starts with '&' and swallows following non-'&' tokens."""
    out, cur = [], None
    for tok in blob.split():
        if tok.startswith("&"):
            if cur:
                out.append(cur)
            cur = [tok[1:]]
        elif cur is not None:
            cur.append(tok)
    if cur:
        out.append(cur)
    return out


def describe(parts):
    """Turn a binding's tokens into {behavior, label, usage, kind}."""
    behavior, args = parts[0], parts[1:]
    entry = {"behavior": behavior, "args": args}

    if behavior == "kp" and args:
        code = args[0]
        entry["kind"] = "key"
        entry["usage"] = USAGE.get(code)
        entry["label"] = LABEL.get(code, code.replace("KP_N", "").title()
                                   if code.startswith("KP_N") else code.title())
        if entry["usage"] is None:
            entry["unmapped"] = code
    elif behavior in ("mo", "tog", "to", "sl"):
        entry["kind"] = "layer"
        entry["layer"] = int(args[0]) if args and args[0].isdigit() else None
        entry["label"] = "%s %s" % (behavior, args[0] if args else "?")
    elif behavior == "trans":
        entry["kind"] = "trans"
        entry["label"] = ""
    elif behavior == "none":
        entry["kind"] = "none"
        entry["label"] = ""
    else:
        # bt / rgb_ug / bl / stp / bootloader / studio_unlock / macros
        entry["kind"] = "other"
        entry["label"] = " ".join([behavior] + args).replace("_", " ")
    return entry


def parse_keymap(path, nkeys):
    src = strip_comments(open(path).read())
    body = src[src.index("keymap {"):]

    layers = []
    for m in re.finditer(
        r"(\w+)\s*\{\s*display-name\s*=\s*\"([^\"]+)\"\s*;(.*?)\}\s*;",
        body, flags=re.S,
    ):
        ident, display, inner = m.group(1), m.group(2), m.group(3)
        if "reserved" in inner:
            continue
        bm = re.search(r"bindings\s*=\s*<(.*?)>\s*;", inner, flags=re.S)
        if not bm:
            continue
        bindings = [describe(p) for p in split_bindings(bm.group(1))]

        # Prefer the printed keycap over the binding name, but only where the
        # key does nothing typeable — if a layer maps a real keycode to one of
        # these positions, that keycode is the more useful thing to show.
        for pos, printed in KEYCAP.items():
            if pos < len(bindings) and bindings[pos]["kind"] in ("layer", "none"):
                bindings[pos]["label"] = printed

        if len(bindings) != nkeys:
            sys.exit(
                "layer %r has %d bindings but layout has %d keys"
                % (ident, len(bindings), nkeys)
            )
        layers.append({"id": ident, "name": display, "bindings": bindings})
    if not layers:
        sys.exit("no layers parsed from %s" % path)
    return layers


def build_reverse(layers):
    """usage -> positions that can emit it, per layer and resolved overall.

    &trans falls through to the base layer, so resolve it before indexing.
    """
    base = layers[0]["bindings"]
    per_layer, overall = [], {}

    for li, layer in enumerate(layers):
        table = {}
        for pos, b in enumerate(layer["bindings"]):
            if b["kind"] == "trans" and li > 0:
                b = base[pos]
            usage = b.get("usage")
            if usage is None:
                continue
            table.setdefault(usage, []).append(pos)
            overall.setdefault(usage, set()).add(pos)
        per_layer.append(table)

    resolved = {u: sorted(p) for u, p in overall.items()}
    ambiguous = {u: p for u, p in resolved.items() if len(p) > 1}
    return per_layer, resolved, ambiguous


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("zmk_root", help="checkout of KinesisCorporation/Adv360-Pro-ZMK")
    ap.add_argument("-o", "--out", default="src/assets")
    args = ap.parse_args()

    cfg = os.path.join(args.zmk_root, "config")
    keys = parse_layout(
        os.path.join(cfg, "boards/arm/adv360/adv360-layouts.dtsi")
    )
    layers = parse_keymap(os.path.join(cfg, "adv360.keymap"), len(keys))
    per_layer, resolved, ambiguous = build_reverse(layers)

    os.makedirs(args.out, exist_ok=True)

    xs = [k["x"] + k["w"] for k in keys] + [k["rx"] for k in keys]
    ys = [k["y"] + k["h"] for k in keys] + [k["ry"] for k in keys]
    layout = {
        "name": "Kinesis Advantage 360 Pro",
        "source": "KinesisCorporation/Adv360-Pro-ZMK",
        "keyCount": len(keys),
        "extent": {"w": max(xs), "h": max(ys)},
        "keys": keys,
    }
    with open(os.path.join(args.out, "layout.json"), "w") as fh:
        json.dump(layout, fh, indent=1)

    keymap = {
        "layers": [
            {"id": l["id"], "name": l["name"], "bindings": l["bindings"]}
            for l in layers
        ],
        "usageToPositions": {str(u): p for u, p in sorted(resolved.items())},
        "usageToPositionsByLayer": [
            {str(u): sorted(p) for u, p in sorted(t.items())} for t in per_layer
        ],
    }
    with open(os.path.join(args.out, "keymap.json"), "w") as fh:
        json.dump(keymap, fh, indent=1)

    unmapped = sorted({
        b["unmapped"] for l in layers for b in l["bindings"] if "unmapped" in b
    })
    print("keys       : %d" % len(keys))
    print("extent     : %.2fu x %.2fu" % (layout["extent"]["w"], layout["extent"]["h"]))
    print("layers     : %s" % ", ".join("%s(%s)" % (l["name"], l["id"]) for l in layers))
    print("usages     : %d distinct" % len(resolved))
    print("ambiguous  : %d %s" % (len(ambiguous), sorted(ambiguous) if ambiguous else ""))
    if unmapped:
        print("UNMAPPED   : %s" % ", ".join(unmapped))
    print("wrote      : %s/layout.json, %s/keymap.json" % (args.out, args.out))


if __name__ == "__main__":
    main()
