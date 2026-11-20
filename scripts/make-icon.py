"""Draw app/icon.png: an original 256x256 icon of the Sage's notebook.

A hardcover notebook in A5 proportions (1 : 1.41), seen a little from above
and to the left, so its spine and the top of the page block show. The front
is laid out like a real bound cover: a gilt board, a gilt rule, a sage-green
border, a lighter green-grey border and a smaller peach panel with cut
corners, all at even margins. The spine is sage green with gilt bands at its
head and foot. Centred on the panel is an
eight-pointed star with a light contour: long deeper-gold cardinal rays,
shorter pale diagonal rays, and a red stone in a gilt ring. A wide gilt strap crosses the book at the
star's height and fastens with a clasp tab at the fore-edge.

Every edge sits on whole pixels of the final image, so the icon stays crisp.
Drawn at 4x and downsampled.

    python3 scripts/make-icon.py [out.png]
"""
import math
import sys

from PIL import Image, ImageDraw

S = 4                      # supersampling
W = 256 * S
OUT = sys.argv[1] if len(sys.argv) > 1 else "app/icon.png"

INK = (84, 66, 34)
GOLD = (212, 180, 92)
GOLD_DARK = (170, 138, 62)
GOLD_LIGHT = (240, 222, 152)
SAGE = (132, 154, 114)
SAGE_LINE = (104, 126, 90)
SAGE_PALE = (184, 198, 172)     # the lighter green-grey border inside the sage
SAGE_PALE_LINE = (150, 168, 138)
PEACH = (232, 152, 112)
PEACH_LINE = (208, 126, 90)
SPINE = (132, 154, 114)         # the spine: sage green like the cover border
SPINE_SHADE = (104, 126, 90)
STRAP_SIDE = (140, 110, 46)     # the strap where it turns over the spine
PAGES = (247, 240, 212)
PAGE_LINE = (218, 204, 158)
RAY_CARD = (222, 184, 78)       # cardinal rays: deeper gold
RAY_CARD_SHADE = (192, 152, 56)
RAY_DIAG = (248, 236, 172)      # diagonal rays: pale gold
RAY_DIAG_SHADE = (232, 214, 138)
STAR_EDGE = (252, 244, 206)     # the star's light contour
STRAP = (180, 146, 64)
STRAP_LIGHT = (210, 178, 96)
RED = (194, 52, 40)
RED_DARK = (132, 30, 22)
RED_LIGHT = (240, 140, 112)

img = Image.new("RGB", (W, W), (255, 255, 255))
d = ImageDraw.Draw(img)


def P(x, y):
    return (round(x * S), round(y * S))


def poly(points, fill=None, outline=INK, width=2):
    pts = [P(*p) for p in points]
    if fill:
        d.polygon(pts, fill=fill)
    if outline:
        d.line(pts + [pts[0]], fill=outline, width=width * S, joint="curve")


def rect(x0, y0, x1, y1, fill=None, outline=INK, width=2, r=0):
    d.rounded_rectangle([P(x0, y0), P(x1, y1)], radius=r * S, fill=fill,
                        outline=outline, width=width * S if outline else 0)


# Geometry: an A5 front (132 x 186), 14 px of depth, centred on the canvas.
DEP = 14
FW, FH = 132, 186
L = (256 - FW - DEP) // 2 + DEP
T = (256 - FH - DEP) // 2 + DEP
R, B = L + FW, T + FH

# Top of the book: the page block between the boards.
poly([(L, T), (R, T), (R - DEP, T - DEP), (L - DEP, T - DEP)], PAGES)
for i in (1, 2, 3):
    y = T - DEP * i / 4
    x0 = L - DEP * i / 4 + 3
    x1 = R - DEP * i / 4 - 3
    d.line([P(x0, y), P(x1, y)], fill=PAGE_LINE, width=S)

# The spine: sage green, with a narrow gilt band at the head and the foot.
poly([(L, T), (L - DEP, T - DEP), (L - DEP, B - DEP), (L, B)], SPINE)
d.line([P(L - DEP / 2, T - DEP / 2 + 10), P(L - DEP / 2, B - DEP / 2 - 10)], fill=SPINE_SHADE, width=S)
for y0, y1 in ((T + 6, T + 10), (B - 10, B - 6)):    # clear of the edges, so they read as bands
    poly([(L, y0), (L - DEP, y0 - DEP), (L - DEP, y1 - DEP), (L, y1)], GOLD, outline=None)
poly([(L, T), (L - DEP, T - DEP), (L - DEP, B - DEP), (L, B)], None)

# The front cover, at even margins: gilt board, gilt rule, sage border, a
# lighter green-grey border, then the panel.
# Square corners, so the front meets the spine and the top edge cleanly.
rect(L, T, R, B, GOLD)
rect(L + 6, T + 6, R - 6, B - 6, None, outline=GOLD_LIGHT, width=2)
rect(L + 10, T + 10, R - 10, B - 10, SAGE, outline=SAGE_LINE, width=1)
rect(L + 16, T + 16, R - 16, B - 16, SAGE_PALE, outline=SAGE_PALE_LINE, width=1)
# The panel: smaller, with its corners cut off.
def cut_rect(inset, cut):
    x0, y0, x1, y1 = L + inset, T + inset, R - inset, B - inset
    return [(x0 + cut, y0), (x1 - cut, y0), (x1, y0 + cut), (x1, y1 - cut),
            (x1 - cut, y1), (x0 + cut, y1), (x0, y1 - cut), (x0, y0 + cut)]


poly(cut_rect(27, 9), PEACH, width=2)
poly(cut_rect(32, 7), None, outline=PEACH_LINE, width=1)

# The strap, at the star's height: over the spine, across the front, under
# a clasp tab at the fore-edge.
cx, cy = (L + R) / 2, (T + B) / 2
SH = 11                                           # half the strap's height
poly([(L, cy - SH), (L - DEP, cy - SH - DEP), (L - DEP, cy + SH - DEP), (L, cy + SH)], STRAP_SIDE)
rect(L, cy - SH, R, cy + SH, STRAP, width=2)
d.line([P(L + 2, cy - SH + 4), P(R - 2, cy - SH + 4)], fill=STRAP_LIGHT, width=S)
d.line([P(L + 2, cy + SH - 4), P(R - 2, cy + SH - 4)], fill=STRAP_LIGHT, width=S)
rect(R - 18, cy - SH - 4, R + 3, cy + SH + 4, GOLD_LIGHT, width=2, r=2)
d.ellipse([P(R - 10.5, cy - 3), P(R - 4.5, cy + 3)], fill=GOLD_DARK, outline=INK, width=S)

# The star, centred on the panel and inside it.
def pt(k, rad):
    ang = -math.pi / 2 + k * math.pi / 8
    return (cx + rad * math.cos(ang), cy + rad * math.sin(ang))


def radius(k):
    k %= 16
    if k % 4 == 0:
        return 40       # cardinal points
    if k % 2 == 0:
        return 24       # diagonal points
    return 11           # notches between points


for k in (2, 6, 10, 14, 0, 4, 8, 12):
    tip = pt(k, radius(k))
    lit, shade = (RAY_CARD, RAY_CARD_SHADE) if k % 4 == 0 else (RAY_DIAG, RAY_DIAG_SHADE)
    poly([(cx, cy), pt(k - 1, radius(k - 1)), tip], lit, outline=None)
    poly([(cx, cy), tip, pt(k + 1, radius(k + 1))], shade, outline=None)
# A thin dark line keeps the shape crisp; the light contour sits just inside it.
star = [pt(k, radius(k)) for k in range(16)]
poly(star, None, width=2)
poly([(cx + (x - cx) * 0.93, cy + (y - cy) * 0.93) for x, y in star], None, outline=STAR_EDGE, width=2)

# The boss: a gilt ring round a red stone.
for r, fill, outline in ((10, GOLD_LIGHT, INK), (6.5, RED, RED_DARK)):
    d.ellipse([P(cx - r, cy - r), P(cx + r, cy + r)], fill=fill, outline=outline, width=2 * S)
d.ellipse([P(cx - 3.5, cy - 4), P(cx - 0.5, cy - 1)], fill=RED_LIGHT)

img.resize((256, 256), Image.LANCZOS).save(OUT, optimize=True)
print("wrote", OUT)
