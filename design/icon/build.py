"""Enji's icons: Fluent's "device eq" (five rounded bars) in the accent, on
a pure white plate where the platform has a shape, pure black in the dark
appearance.

Writes next to this script:
  enji.svg           Windows and Linux: the glyph alone, no plate
  plate.svg          Android and the rest: the glyph on the white plate
  apple/Enji.icon    the Liquid Glass document macOS and iOS use (Bern
                     compiles it with actool: glass from macOS 26 and iOS
                     26, a flat picture drawn from it before)
  preview/           that document rendered by Xcode's ictool

The glyph is ic_fluent_device_eq_24_filled from Microsoft's Fluent UI
System Icons (MIT): source/. export.sh turns these into the files Bern.toml
names.
"""

import json
import os
import re
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
ACCENT = "#FF0732"
PLATE, PLATE_DARK = "#FFFFFF", "#000000"
# The glyph's larger side as a part of the plate's width, as the first
# icon had it.
SCALE = 0.56
# Without a plate the glyph fills its square, as Windows' and Linux's own
# icons do.
BARE_SCALE = 0.92
ICTOOL = "/Applications/Xcode.app/Contents/Applications/Icon Composer.app/Contents/Executables/ictool"
RENDITIONS = ["Default", "Dark", "ClearLight", "ClearDark", "TintedLight", "TintedDark"]

source = open(os.path.join(HERE, "source", "ic_fluent_device_eq_24_filled.svg")).read()
D = re.search(r' d="([^"]+)"', source).group(1)


def bounds(d):
    """The path's box, its curves sampled (the path is M, L, H, V, C and Z
    only, all absolute)."""
    xs, ys = [], []
    pos = start = (0.0, 0.0)
    for cmd, args in re.findall(r"([MLHVCZ])([^MLHVCZ]*)", d):
        n = [float(v) for v in re.findall(r"-?\d*\.?\d+", args)]
        if cmd == "M":
            pos = start = (n[0], n[1])
        elif cmd == "L":
            pos = (n[0], n[1])
        elif cmd == "H":
            pos = (n[0], pos[1])
        elif cmd == "V":
            pos = (pos[0], n[0])
        elif cmd == "C":
            for i in range(0, len(n), 6):
                p0, p1, p2, p3 = pos, (n[i], n[i + 1]), (n[i + 2], n[i + 3]), (n[i + 4], n[i + 5])
                for k in range(41):
                    t = k / 40
                    u = 1 - t
                    xs.append(u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0])
                    ys.append(u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1])
                pos = p3
        else:
            pos = start
        xs.append(pos[0]), ys.append(pos[1])
    return min(xs), min(ys), max(xs), max(ys)


X0, Y0, X1, Y1 = bounds(D)


def glyph(x, y, w, scale, color):
    """The glyph centred in the square at (x, y) of side w, its larger side
    `scale` of w."""
    side = max(X1 - X0, Y1 - Y0)
    k = w * scale / side
    tx = x + w / 2 - (X0 + X1) / 2 * k
    ty = y + w / 2 - (Y0 + Y1) / 2 * k
    return f'<path transform="translate({tx:.3f} {ty:.3f}) scale({k:.5f})" d="{D}" fill="{color}"/>'


def svg(body, side=1024):
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 0 {side} {side}">{body}</svg>\n'


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as out:
        out.write(text)


def rgb(color):
    return "srgb:" + ",".join("%.5f" % (int(color[i:i + 2], 16) / 255) for i in (1, 3, 5)) + ",1.00000"


for name in ("apple", "preview"):
    shutil.rmtree(os.path.join(HERE, name), ignore_errors=True)

write(os.path.join(HERE, "enji.svg"), svg(glyph(0, 0, 1024, BARE_SCALE, ACCENT)))
write(os.path.join(HERE, "plate.svg"), svg(f'<rect width="1024" height="1024" fill="{PLATE}"/>' + glyph(0, 0, 1024, SCALE, ACCENT)))

# Liquid Glass: one opaque glass layer over the white fill, the black fill
# in the dark appearance; clear and tinted come from the system.
icon = os.path.join(HERE, "apple", "Enji.icon")
write(os.path.join(icon, "Assets", "eq.svg"), svg(glyph(0, 0, 1024, SCALE, ACCENT)))
write(os.path.join(icon, "icon.json"), json.dumps({
    "fill": {"solid": rgb(PLATE)},
    "fill-specializations": [{"value": {"solid": rgb(PLATE)}}, {"appearance": "dark", "value": {"solid": rgb(PLATE_DARK)}}],
    "groups": [{
        "layers": [{"image-name": "eq.svg", "name": "eq", "glass": True,
                    "fill-specializations": [{"appearance": "dark", "value": {"solid": rgb(ACCENT)}}]}],
        "shadow": {"kind": "neutral", "opacity": 0.5},
        "translucency": {"enabled": False, "value": 0.5},
    }],
    "supported-platforms": {"squares": "shared"},
}, indent=2) + "\n")

if os.path.exists(ICTOOL):
    for rendition in RENDITIONS:
        out = os.path.join(HERE, "preview", f"glass-{rendition}.png")
        os.makedirs(os.path.dirname(out), exist_ok=True)
        subprocess.run([ICTOOL, icon, "--export-image", "--output-file", out, "--platform", "iOS",
                        "--rendition", rendition, "--width", "512", "--height", "512", "--scale", "1"],
                       check=True, stdout=subprocess.DEVNULL)
else:
    print("no Icon Composer: the Liquid Glass previews are skipped")
print("ok")
