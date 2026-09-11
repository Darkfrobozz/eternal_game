#!/usr/bin/env python3
"""Generate the tech-ball pixel assets.

The ball is split so it can be animated in layers:

  * ball_shell.png  (80x80)  the outer tyre that rolls along the floor.
                             Rotationally asymmetric (tread) so a roll reads.
  * ball_core.png   (80x80)  the stabilised inner chassis. Counter-rotates to
                             stay upright, carrying three battery bays on its
                             flat top.
  * battery_panel.png (12x14) the small battery that drops into a bay.
  * preview.png              assembled balls charged empty/mid/full, plus a
                             strip showing the shell rolling.

Everything is greyscale: the game tints the battery glass by charge, while the
metal shell/chassis stay neutral. Pure stdlib (zlib + struct), no Pillow.

Run:  python3 tools/make_pixel_assets.py
"""

import math
import os
import struct
import zlib

# --- palette (greyscale, alpha last) ---------------------------------------
T = (0, 0, 0, 0)
OUTLINE = (24, 26, 34, 255)
SHELL_DARK = (66, 70, 84, 255)
SHELL_MID = (118, 124, 138, 255)
SHELL_LIGHT = (176, 182, 196, 255)
TIRE_DARK = (52, 56, 68, 255)
TIRE_MID = (92, 98, 112, 255)
WIRE = (78, 84, 100, 255)
FRAME = (96, 102, 118, 255)
GLASS = (228, 234, 247, 255)
GLASS_HI = (255, 255, 255, 255)

METAL_DARK = (58, 64, 78, 255)
METAL_MID = (104, 112, 130, 255)
METAL_LIGHT = (156, 166, 186, 255)
METAL_HI = (204, 214, 232, 255)
RIDGE_DARK = (40, 44, 56, 255)

TAU = math.tau


def write_png(path, w, h, rows):
    raw = b"".join(b"\x00" + bytes(v for px in row for v in px) for row in rows)

    def chunk(tag, data):
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)  # 8-bit RGBA
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(png)


class FB:
    """Hard-edged (no AA) pixel framebuffer using centre-of-pixel sampling."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.px = [[T for _ in range(w)] for _ in range(h)]

    def put(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y][x] = c

    def get(self, x, y):
        if 0 <= x < self.w and 0 <= y < self.h:
            return self.px[y][x]
        return T

    def disc(self, cx, cy, r, c):
        r2 = r * r
        for y in range(self.h):
            for x in range(self.w):
                dx, dy = x + 0.5 - cx, y + 0.5 - cy
                if dx * dx + dy * dy <= r2:
                    self.px[y][x] = c

    def ring(self, cx, cy, r_in, r_out, c):
        lo, hi = r_in * r_in, r_out * r_out
        for y in range(self.h):
            for x in range(self.w):
                dx, dy = x + 0.5 - cx, y + 0.5 - cy
                if lo < dx * dx + dy * dy <= hi:
                    self.px[y][x] = c

    def line(self, x0, y0, x1, y1, c):
        x0, y0, x1, y1 = int(x0), int(y0), int(x1), int(y1)
        dx, dy = abs(x1 - x0), -abs(y1 - y0)
        sx = 1 if x0 < x1 else -1
        sy = 1 if y0 < y1 else -1
        err = dx + dy
        while True:
            self.put(x0, y0, c)
            if x0 == x1 and y0 == y1:
                break
            e2 = 2 * err
            if e2 >= dy:
                err += dy
                x0 += sx
            if e2 <= dx:
                err += dx
                y0 += sy

    def rect(self, x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.put(x, y, c)

    def blit(self, other, ox, oy):
        for y in range(other.h):
            for x in range(other.w):
                c = other.px[y][x]
                if c[3]:
                    self.put(ox + x, oy + y, c)


# --- ball geometry ----------------------------------------------------------
SIZE = 80
CX = CY = 40.0
SHELL_OUT = 39.0   # outer tyre radius
SHELL_IN = 33.0    # inner tyre radius
CORE_R = 31.5      # stabilised chassis radius
CORE_FLAT = 10.0   # how far above centre the flat battery deck sits
BATT_W, BATT_H = 12, 14
BAY_DX = 14        # battery centres at CX-BAY_DX, CX, CX+BAY_DX


def make_shell():
    """Rolling tyre: hollow ring with a tread pattern so spin is visible."""
    fb = FB(SIZE, SIZE)
    fb.ring(CX, CY, SHELL_IN - 0.5, SHELL_OUT, OUTLINE)
    fb.ring(CX, CY, SHELL_IN, SHELL_OUT - 1.0, TIRE_MID)

    seg = TAU / 12.0
    for y in range(SIZE):
        for x in range(SIZE):
            dx, dy = x + 0.5 - CX, y + 0.5 - CY
            d = math.hypot(dx, dy)
            if not (SHELL_IN <= d <= SHELL_OUT - 1.0):
                continue
            a = math.atan2(dy, dx) + math.pi
            frac = (a % seg) / seg
            # Radial groove at each tread boundary.
            if frac < 0.035 or frac > 0.965:
                fb.put(x, y, TIRE_DARK)
            elif int(a / seg) % 2 == 0:
                fb.put(x, y, SHELL_LIGHT)
    return fb


def make_core():
    """Upright chassis: disc with a flat top deck, hub and battery bays."""
    fb = FB(SIZE, SIZE)
    fb.disc(CX, CY, CORE_R, OUTLINE)
    fb.disc(CX, CY, CORE_R - 1.0, SHELL_DARK)

    # Flatten the top into a deck the batteries can sit on.
    deck = CY - CORE_FLAT
    for y in range(SIZE):
        if y < deck:
            for x in range(SIZE):
                fb.put(x, y, T)
    half = math.sqrt((CORE_R - 1.0) ** 2 - CORE_FLAT ** 2)
    fb.rect(int(CX - half), int(deck), int(CX + half), int(deck), OUTLINE)
    fb.rect(int(CX - half) + 1, int(deck) + 1, int(CX + half) - 1, int(deck) + 2, SHELL_LIGHT)

    # Top highlight / bottom ballast so the chassis reads as bottom-weighted.
    for y in range(SIZE):
        for x in range(SIZE):
            dx, dy = x + 0.5 - CX, y + 0.5 - CY
            d = math.hypot(dx, dy)
            if d > CORE_R - 1.0 or y < deck:
                continue
            if dx + dy < -10.0 and y > deck + 2:
                fb.put(x, y, SHELL_MID)
            elif dy > 12.0 and d > CORE_R - 6.0:
                fb.put(x, y, OUTLINE)

    # Hub with wires fanning out to the three battery bays.
    hub = (CX, CY + 8.0)
    for bx in (CX - BAY_DX, CX, CX + BAY_DX):
        fb.line(hub[0], hub[1], bx, deck + 2, WIRE)
    fb.disc(hub[0], hub[1], 5.0, OUTLINE)
    fb.disc(hub[0], hub[1], 3.4, GLASS)

    # Recessed seats under each battery.
    for i in (-1, 0, 1):
        bx = int(CX + i * BAY_DX - BATT_W / 2)
        fb.rect(bx, int(deck) + 1, bx + BATT_W - 1, int(deck) + 3, OUTLINE)
        fb.rect(bx + 1, int(deck) + 1, bx + BATT_W - 2, int(deck) + 2, TIRE_DARK)
    return fb


def make_panel():
    """Small framed glass battery that drops into a bay."""
    W, H = BATT_W, BATT_H
    fb = FB(W, H)
    fb.rect(4, 0, 7, 2, OUTLINE)          # terminal nub
    fb.rect(5, 1, 6, 2, SHELL_LIGHT)
    fb.rect(0, 2, W - 1, H - 1, OUTLINE)
    fb.rect(1, 3, W - 2, H - 2, SHELL_MID)
    fb.rect(1, 3, W - 2, 3, SHELL_LIGHT)  # top bevel
    fb.rect(1, 3, 1, H - 2, SHELL_LIGHT)  # left bevel
    fb.rect(1, H - 2, W - 2, H - 2, SHELL_DARK)
    fb.rect(W - 2, 3, W - 2, H - 3, SHELL_DARK)
    fb.rect(2, 4, W - 3, H - 4, OUTLINE)  # glass recess
    fb.rect(3, 5, W - 4, H - 5, GLASS)
    fb.rect(3, 5, W - 4, 5, GLASS_HI)     # gloss
    fb.put(4, 6, GLASS_HI)
    fb.put(5, 7, GLASS_HI)
    fb.rect(3, H - 3, 4, H - 2, FRAME)    # contacts
    fb.rect(W - 5, H - 3, W - 4, H - 2, FRAME)
    return fb


# --- solid block ----------------------------------------------------------

def make_solid():
    """A metallic solid block. A raised beam runs from the middle out to each
    of the four corners, so neighbours meet corner-to-corner."""
    W = H = 16
    fb = FB(W, H)
    cx, cy = (W - 1) / 2.0, (H - 1) / 2.0

    # Base plate: bevel top-left light / bottom-right dark, with a brushed sheen
    # and a soft diagonal falloff so it reads as rolled metal.
    for y in range(H):
        for x in range(W):
            if x == 0 or y == 0:
                c = METAL_LIGHT
            elif x == W - 1 or y == H - 1:
                c = METAL_DARK
            else:
                brush = ((x * 3 + y * 5) % 4) - 1        # dither -1..2
                sheen = (x + y) - (W - 1)               # top-left brighter
                base = 118 + brush * 6 - sheen * 2
                base = max(64, min(152, base))
                c = (base, base + 10, base + 28, 255)
            fb.put(x, y, c)

    # Raised X beam: centre to each corner. A bright crest with dark flanks
    # gives it relief, and the corners line up with the neighbouring blocks.
    for y in range(H):
        for x in range(W):
            for d in (abs(x - y), abs((x + y) - (W - 1))):
                if d == 0:
                    fb.put(x, y, METAL_HI)
                elif d == 1:
                    fb.put(x, y, RIDGE_DARK)

    # Central hub over the crossing, with a bright rivet.
    fb.disc(cx + 0.5, cy + 0.5, 3.6, OUTLINE)
    fb.disc(cx + 0.5, cy + 0.5, 2.6, METAL_LIGHT)
    fb.disc(cx + 0.5, cy + 0.5, 1.0, METAL_HI)
    return fb


# --- rain drop / splat -----------------------------------------------------

DROP_DARK = (108, 128, 156, 255)
DROP_MID = (176, 200, 224, 255)
DROP_HI = (238, 248, 255, 255)


def make_drop():
    """A teardrop: round-bottomed, tapering to a point at the top. Falling
    straight down, the round end leads."""
    W, H = 16, 24
    fb = FB(W, H)
    cx, cy, r = W / 2.0, 16.0, 7.0
    for y in range(H):
        for x in range(W):
            fx, fy = x + 0.5, y + 0.5
            half = 0.0
            dy = fy - cy
            if -r <= dy <= r:
                half = max(half, (r * r - dy * dy) ** 0.5)
            if fy < cy:
                half = max(half, r * (fy / cy) ** 0.7)
            off = fx - cx
            if half <= 0.0 or abs(off) > half:
                continue
            edge = half - abs(off)
            if edge < 1.1:
                c = DROP_DARK
            elif off < -1.0 and -4.0 < dy < 2.0 and edge > 2.5:
                c = DROP_HI
            else:
                c = DROP_MID
            fb.put(x, y, c)
    return fb


def make_splat():
    """A soft ball, scaled non-uniformly in-engine into an ellipse and then a
    flat puddle."""
    S = 16
    fb = FB(S, S)
    c, r = S / 2.0, 7.0
    for y in range(S):
        for x in range(S):
            fx, fy = x + 0.5, y + 0.5
            dx, dy = fx - c, fy - c
            d = (dx * dx + dy * dy) ** 0.5
            if d > r:
                continue
            if d > r - 1.2:
                col = DROP_DARK
            elif dx < -1.5 and dy < 0.0 and d < r - 3.0:
                col = DROP_HI
            else:
                col = DROP_MID
            fb.put(x, y, col)
    return fb


# --- transform / tint / preview --------------------------------------------

def rotate(fb, deg):
    a = math.radians(deg)
    c, s = math.cos(a), math.sin(a)
    cx, cy = (fb.w - 1) / 2.0, (fb.h - 1) / 2.0
    out = FB(fb.w, fb.h)
    for y in range(fb.h):
        for x in range(fb.w):
            dx, dy = x - cx, y - cy
            sx = c * dx + s * dy + cx
            sy = -s * dx + c * dy + cy
            out.px[y][x] = fb.get(int(round(sx)), int(round(sy)))
    return out


def hsl_to_rgb(h, s, l):
    c = (1 - abs(2 * l - 1)) * s
    x = c * (1 - abs((h / 60.0) % 2 - 1))
    m = l - c / 2
    r, g, b = [(c, x, 0), (x, c, 0), (0, c, x), (0, x, c), (x, 0, c), (c, 0, x)][
        int(h // 60) % 6
    ]
    return tuple(int(255 * (v + m)) for v in (r, g, b))


def charge_tint(charge):
    t = max(0.0, min(1.0, charge / (charge + 8.0)))
    return hsl_to_rgb(120.0 * t, 0.85, 0.45 + 0.15 * t)


def show(fb, rgb):
    """Copy a framebuffer with an RGB multiplier applied to opaque pixels."""
    out = FB(fb.w, fb.h)
    for y in range(fb.h):
        for x in range(fb.w):
            r, g, b, a = fb.px[y][x]
            if a:
                out.px[y][x] = (r * rgb[0] // 255, g * rgb[1] // 255, b * rgb[2] // 255, a)
    return out


def assemble(shell, core, panel, angle, tint):
    """Full ball: rolling shell, upright core, charge-tinted batteries on top."""
    out = FB(SIZE, SIZE)
    out.blit(rotate(shell, angle), 0, 0)
    out.blit(core, 0, 0)
    charged = show(panel, tint)
    for i in (-1, 0, 1):
        out.blit(charged, int(CX + i * BAY_DX - BATT_W / 2), int(CY - CORE_FLAT - BATT_H + 1))
    return out


def scale_up(fb, factor):
    out = FB(fb.w * factor, fb.h * factor)
    for y in range(fb.h):
        for x in range(fb.w):
            c = fb.px[y][x]
            for dy in range(factor):
                for dx in range(factor):
                    out.px[y * factor + dy][x * factor + dx] = c
    return out


def tile_sheet(fb, cols, rows):
    out = FB(fb.w * cols, fb.h * rows)
    for r in range(rows):
        for c in range(cols):
            out.blit(fb, c * fb.w, r * fb.h)
    return out


def compose_preview(shell, core, panel, scale=4):
    charges = (0.0, 4.0, 16.0)
    pad = 6
    width = pad + len(charges) * (SIZE + pad)
    height = pad + SIZE + pad + SIZE + pad
    rows = [[(18, 18, 24, 255) for _ in range(width * scale)] for _ in range(height * scale)]

    def stamp(fb, gx, gy):
        for y in range(fb.h):
            for x in range(fb.w):
                px = fb.px[y][x]
                if px[3] == 0:
                    continue
                for sy in range(scale):
                    for sx in range(scale):
                        rows[(gy + y) * scale + sy][(gx + x) * scale + sx] = px

    for i, ch in enumerate(charges):
        x = pad + i * (SIZE + pad)
        ball = assemble(shell, core, panel, 22.0 * i, charge_tint(ch))
        stamp(ball, x, pad)
        # Shell alone at the same rotation, to show the tread rolling.
        stamp(rotate(shell, 22.0 * i), x, pad + SIZE + pad)
    return rows


def ascii_preview(fb, title):
    print(f"\n{title}  ({fb.w}x{fb.h})")
    ramp = " .:-=+*#%@"
    for row in fb.px:
        print("".join(" " if a == 0 else ramp[min(9, (r + g + b) // 77)] for r, g, b, a in row))


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, os.pardir, "assets", "sprites")

    shell = make_shell()
    core = make_core()
    panel = make_panel()
    solid = make_solid()
    drop = make_drop()
    splat = make_splat()

    write_png(os.path.join(out, "ball_shell.png"), shell.w, shell.h, shell.px)
    write_png(os.path.join(out, "ball_core.png"), core.w, core.h, core.px)
    write_png(os.path.join(out, "battery_panel.png"), panel.w, panel.h, panel.px)
    write_png(os.path.join(out, "solid_block.png"), solid.w, solid.h, solid.px)
    write_png(os.path.join(out, "rain_drop.png"), drop.w, drop.h, drop.px)
    write_png(os.path.join(out, "splat.png"), splat.w, splat.h, splat.px)

    # A grid of blocks, so the corner pattern can be checked for continuity.
    sheet = scale_up(tile_sheet(solid, 4, 3), 8)
    write_png(os.path.join(out, "solid_preview.png"), sheet.w, sheet.h, sheet.px)

    preview = compose_preview(shell, core, panel)
    write_png(os.path.join(out, "preview.png"), len(preview[0]), len(preview), preview)

    ascii_preview(shell, "ball_shell.png")
    ascii_preview(core, "ball_core.png")
    ascii_preview(panel, "battery_panel.png")
    ascii_preview(solid, "solid_block.png")
    ascii_preview(drop, "rain_drop.png")
    ascii_preview(splat, "splat.png")
    print("\nWrote ball_shell.png, ball_core.png, battery_panel.png,")
    print("solid_block.png, solid_preview.png, rain_drop.png, splat.png, preview.png")


if __name__ == "__main__":
    main()
