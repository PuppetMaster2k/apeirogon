"""Generate Apeirogon app + tray icons (SVG masters, PNG, ICO, ICNS)."""
import io, os, shutil, sys
import cairosvg
from PIL import Image

OUT = sys.argv[1]
ACCENT_L, ACCENT_D = "#2F5BEA", "#7088FF"
INK_L, INK_D = "#12151A", "#FFFFFF"
DANGER_L, DANGER_D = "#C42B26", "#F0675E"

# --- glyph geometry on two pixel grids (stroke 2, integer coords => crisp) ---------------
GRIDS = {
    16: dict(oct=[(5, 1), (11, 1), (15, 5), (15, 11), (11, 15), (5, 15), (1, 11), (1, 5)], gap=(6, 10), slash=((14, 2), (2, 14))),
    24: dict(oct=[(8, 2), (16, 2), (22, 8), (22, 16), (16, 22), (8, 22), (2, 16), (2, 8)], gap=(10, 14), slash=((21, 3), (3, 21))),
}


def pts(ps):
    return " ".join(f"{x},{y}" for x, y in ps)


def half(g, side):
    o = GRIDS[g]["oct"]
    l, r = GRIDS[g]["gap"]
    top, bot = o[0][1], o[4][1]
    if side == "right":
        return [(r, top), o[1], o[2], o[3], o[4], (r, bot)]
    return [(l, top), o[0], o[7], o[6], o[5], (l, bot)]


def glyph(g, state, ink, fill, danger=None, sw=2):
    """Two octagon halves with a gap (the boundary). The filled half is where the pointer is.
    state: idle (left = this machine), remote (right = another machine), paused (both outlined),
    error (both outlined + slash)."""
    def poly(side, filled):
        f = fill if filled else "none"
        c = fill if filled else ink
        return (f'<polygon points="{pts(half(g, side))}" fill="{f}" stroke="{c}" '
                f'stroke-width="{sw}" stroke-linejoin="round"/>')
    shape = poly("left", state == "idle") + poly("right", state == "remote")
    if state == "error":
        # disconnected / daemon stopped: the other side fades (mono) or turns red (color)
        far = (f'<polygon points="{pts(half(g, "right"))}" fill="none" stroke="{danger or ink}" '
               f'stroke-opacity="{1 if danger else 0.35}" stroke-width="{sw}" stroke-linejoin="round"/>')
        return poly("left", False) + far
    return shape


def svg(size, inner, vb=None):
    vb = vb or size
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {vb} {vb}">{inner}</svg>'


def app_icon(macos):
    """1024 master. macOS: 824 body on 1024 canvas with shadow. Others: near full-bleed tile."""
    if macos:
        x, s, r = 100, 824, 185
    else:
        x, s, r = 32, 960, 200
    scale = s * 0.56 / 24
    off = x + (s - 24 * scale) / 2
    mark = glyph(24, "idle", "#FFFFFF", "#FFFFFF")
    shadow = ('<filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="12" stdDeviation="14" flood-color="#0A0E1C" flood-opacity="0.32"/></filter>'
              if macos else "")
    return svg(1024,
               f'<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#3D69F2"/><stop offset="1" stop-color="#2449C4"/></linearGradient>{shadow}</defs>'
               f'<rect x="{x}" y="{x}" width="{s}" height="{s}" rx="{r}" fill="url(#bg)" {"filter=\"url(#sh)\"" if macos else ""}/>'
               f'<rect x="{x + 1}" y="{x + 1}" width="{s - 2}" height="{s - 2}" rx="{r - 1}" fill="none" stroke="#FFFFFF" stroke-opacity="0.14" stroke-width="2"/>'
               f'<g transform="translate({off} {off}) scale({scale})">{mark}</g>')


def png(svg_text, size):
    data = cairosvg.svg2png(bytestring=svg_text.encode(), output_width=size, output_height=size)
    return Image.open(io.BytesIO(data)).convert("RGBA")


def tray_png(state, size, ink, fill, danger):
    g = 16 if size <= 18 else 24
    inner = glyph(g, state, ink, fill, danger)
    if size == 18:  # macOS @1x: 16 glyph centered in 18
        return png(svg(18, f'<g transform="translate(1 1)">{inner}</g>', 18), 18)
    if size == 36:  # macOS @2x: 16 grid x2 inside 36
        inner = glyph(16, state, ink, fill, danger)
        return png(svg(36, f'<g transform="translate(2 2) scale(2)">{inner}</g>', 36), 36)
    return png(svg(g, inner), size)


def w(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


STATES = ["idle", "remote", "paused", "error"]
shutil.rmtree(OUT, ignore_errors=True)

# ---- SVG masters -------------------------------------------------------------------------
w(f"{OUT}/svg/app-icon.svg", app_icon(False))
w(f"{OUT}/svg/app-icon-macos.svg", app_icon(True))
for g in (16, 24):
    for st in STATES:
        w(f"{OUT}/svg/tray-mono-{st}-{g}.svg", svg(g, glyph(g, st, "#000000", "#000000")))
        w(f"{OUT}/svg/tray-on-dark-{st}-{g}.svg", svg(g, glyph(g, st, INK_D, ACCENT_D, DANGER_D)))
        w(f"{OUT}/svg/tray-on-light-{st}-{g}.svg", svg(g, glyph(g, st, INK_L, ACCENT_L, DANGER_L)))

# ---- Windows -----------------------------------------------------------------------------
app = app_icon(False)
app_imgs = [png(app, s) for s in (16, 20, 24, 32, 40, 48, 64, 256)]
os.makedirs(f"{OUT}/windows", exist_ok=True)
app_imgs[-1].save(f"{OUT}/windows/apeirogon.ico", sizes=[i.size for i in app_imgs], append_images=app_imgs[:-1])
for st in STATES:
    for theme, ink, fill, danger in (("dark", INK_D, ACCENT_D, DANGER_D), ("light", INK_L, ACCENT_L, DANGER_L)):
        imgs = [tray_png(st, s, ink, fill, danger) for s in (16, 20, 24, 32, 40, 48)]
        imgs[-1].save(f"{OUT}/windows/tray-{st}-{theme}-taskbar.ico", sizes=[i.size for i in imgs], append_images=imgs[:-1])

# ---- macOS -------------------------------------------------------------------------------
mac = app_icon(True)
iconset = f"{OUT}/macos/AppIcon.iconset"
os.makedirs(iconset)
for base in (16, 32, 128, 256, 512):
    png(mac, base).save(f"{iconset}/icon_{base}x{base}.png")
    png(mac, base * 2).save(f"{iconset}/icon_{base}x{base}@2x.png")
png(mac, 1024).save(f"{OUT}/macos/AppIcon.icns", sizes=[(16, 16), (32, 32), (64, 64), (128, 128), (256, 256), (512, 512), (1024, 1024)])
for st in STATES:
    tray_png(st, 18, "#000000", "#000000", None).save(f"{OUT}/macos/tray-{st}Template.png")
    tray_png(st, 36, "#000000", "#000000", None).save(f"{OUT}/macos/tray-{st}Template@2x.png")

# ---- Linux (freedesktop hicolor) ---------------------------------------------------------
hic = f"{OUT}/linux/hicolor"
for s in (16, 22, 24, 32, 48, 64, 128, 256, 512):
    os.makedirs(f"{hic}/{s}x{s}/apps", exist_ok=True)
    png(app, s).save(f"{hic}/{s}x{s}/apps/apeirogon.png")
w(f"{hic}/scalable/apps/apeirogon.svg", app)
for st in STATES:
    w(f"{hic}/scalable/status/apeirogon-{st}.svg", svg(24, glyph(24, st, INK_D, ACCENT_D, DANGER_D)))
    w(f"{hic}/symbolic/status/apeirogon-{st}-symbolic.svg", svg(16, glyph(16, st, "#2e3436", "#2e3436")))
    for s in (22, 24):
        os.makedirs(f"{hic}/{s}x{s}/status", exist_ok=True)
        tray_png(st, s, INK_D, ACCENT_D, DANGER_D).save(f"{hic}/{s}x{s}/status/apeirogon-{st}.png")

# ---- previews for the canvas / guide -------------------------------------------------------
os.makedirs(f"{OUT}/_preview", exist_ok=True)
png(mac, 512).save(f"{OUT}/_preview/app-macos-512.png")
png(app, 512).save(f"{OUT}/_preview/app-512.png")
print("ok")
