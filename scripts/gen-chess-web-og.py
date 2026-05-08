#!/usr/bin/env python3
"""Generate the Open Graph share-card PNG for chess-web.

Run from the repo root:
    python3 scripts/gen-chess-web-og.py

Writes to clients/chess-web/og-image.png. Trunk picks it up via the
`copy-file` rel in clients/chess-web/index.html and ships it to dist/,
so the deployed URL serves it at:
    https://daviddwlee84.github.io/Chinese-Chess_Xiangqi/og-image.png

Re-run whenever the brand / tagline / theme colors change. The PNG is
committed (not built in CI) so deploys do not need Pillow.
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

# Theme colors mirrored from clients/chess-web/style.css (:root vars).
BG = (26, 22, 20)         # --bg     #1a1614
FG = (244, 210, 163)      # --fg     #f4d2a3
RED = (178, 34, 34)       # --red    #b22222
ACCENT = (212, 165, 92)   # --accent #d4a55c
MUTED = (140, 124, 102)   # bumped from --muted for readability at thumb size

W, H = 1200, 630  # Facebook / Twitter / Discord standard OG image size.

# Debian / Ubuntu system fonts. Both are present on standard runners and
# on the dev container — the script will assert if either is missing.
LIBERATION_BOLD = "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf"
LIBERATION_REG = "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"
WQY_ZENHEI = "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc"


def main() -> None:
    for path in (LIBERATION_BOLD, LIBERATION_REG, WQY_ZENHEI):
        assert Path(path).exists(), f"missing system font: {path}"

    out = Path("clients/chess-web/og-image.png")
    out.parent.mkdir(parents=True, exist_ok=True)

    img = Image.new("RGB", (W, H), BG)
    draw = ImageDraw.Draw(img, "RGBA")

    # Subtle diagonal gold-tint gradient for visual interest.
    for y in range(H):
        alpha = 6 + int((y / H) * 18)
        draw.line([(0, y), (W, y)], fill=(*ACCENT, alpha))

    # 帥 disc on the left — same vibe as the picker-hero glyph.
    disc_cx, disc_cy = 270, H // 2
    disc_r = 165
    # Faint red glow ring.
    for r in range(disc_r + 14, disc_r, -1):
        a = int(60 * (1 - (r - disc_r) / 14))
        draw.ellipse(
            (disc_cx - r, disc_cy - r, disc_cx + r, disc_cy + r),
            outline=(*RED, a),
            width=2,
        )
    draw.ellipse(
        (
            disc_cx - disc_r,
            disc_cy - disc_r,
            disc_cx + disc_r,
            disc_cy + disc_r,
        ),
        fill=BG,
        outline=RED,
        width=10,
    )
    cjk_glyph_font = ImageFont.truetype(WQY_ZENHEI, 220)
    glyph = "帥"
    g_bbox = draw.textbbox((0, 0), glyph, font=cjk_glyph_font)
    gw = g_bbox[2] - g_bbox[0]
    gh = g_bbox[3] - g_bbox[1]
    draw.text(
        (disc_cx - gw // 2 - g_bbox[0], disc_cy - gh // 2 - g_bbox[1]),
        glyph,
        fill=RED,
        font=cjk_glyph_font,
    )

    # Right-hand title block.
    text_x = 500
    title_font = ImageFont.truetype(LIBERATION_BOLD, 86)
    cjk_title_font = ImageFont.truetype(WQY_ZENHEI, 76)
    # Mixed Latin+CJK lines need a font that has both glyph sets — Pillow
    # does not auto-fallback. WQY Zen Hei covers both.
    cjk_sub_font = ImageFont.truetype(WQY_ZENHEI, 30)
    sub_font = ImageFont.truetype(LIBERATION_REG, 30)
    tag_font = ImageFont.truetype(LIBERATION_REG, 22)

    draw.text((text_x, 170), "Chinese Chess", fill=ACCENT, font=title_font)
    draw.text((text_x, 280), "中國象棋", fill=FG, font=cjk_title_font)
    draw.text(
        (text_x, 400),
        "Xiangqi 象棋 · Banqi 暗棋 · 三國暗棋",
        fill=FG,
        font=cjk_sub_font,
    )
    draw.text(
        (text_x, 445),
        "Local pass-and-play or online multiplayer",
        fill=MUTED,
        font=sub_font,
    )

    # Footer attribution.
    draw.text(
        (text_x, H - 60),
        "Open source · Rust + WASM · daviddwlee84/Chinese-Chess_Xiangqi",
        fill=MUTED,
        font=tag_font,
    )

    img.save(out, "PNG", optimize=True)
    print(f"wrote {out} ({out.stat().st_size:,} bytes, {W}x{H})")


if __name__ == "__main__":
    main()
