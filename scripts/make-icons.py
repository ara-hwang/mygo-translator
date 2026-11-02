r"""Render the HyMT app icon: crates/hymt-app/assets/{hymt.ico, icon-512.png}.

The tray icon painter in crates/hymt-app/src/tray.rs redoes this geometry at 32 px by hand,
so follow any coordinate change through to its constants. Requires pillow + numpy:
`py -3 <scripts path>`. Text is drawn with Arial Bold / Malgun Gothic Bold from `C:\Windows\Fonts`.
"""

import os
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "crates" / "hymt-app" / "assets"
ASSETS.mkdir(parents=True, exist_ok=True)

K, S, M = 4, 512 * 4, 512
C1, C2 = (37, 99, 235), (14, 165, 233)  # tile gradient #2563EB -> #0EA5E9
BLUE, CYAN = (29, 78, 216), (12, 134, 184)  # glyphs: korean, latin
CARD_BACK = (236, 247, 255, 255)

# geometry in 512-space
TILE_MARGIN, TILE_RADIUS = 26, 120
CARD = 190
CARD_X1, CARD_Y1 = 302, 118  # back card top-left
CARD_X0, CARD_Y0 = 110, 224  # front card top-left
CARD_RAD = 44
A_SIZE, KO_SIZE = 112, 134


def master():
    x, y = np.meshgrid(np.arange(S, dtype=np.float32), np.arange(S, dtype=np.float32))
    t = ((x + y) / (2 * S - 2))[..., None]
    grad = np.rint(
        np.array(C1, np.float32) * (1 - t) + np.array(C2, np.float32) * t
    ).astype(np.uint8)
    grad_solid = Image.fromarray(
        np.dstack([grad, np.full((S, S), 255, np.uint8)]), "RGBA"
    )
    tile = Image.new("L", (S, S), 0)
    ImageDraw.Draw(tile).rounded_rectangle(
        (TILE_MARGIN * K, TILE_MARGIN * K, S - TILE_MARGIN * K, S - TILE_MARGIN * K),
        radius=TILE_RADIUS * K,
        fill=255,
    )
    img = Image.composite(grad_solid, Image.new("RGBA", (S, S), (0, 0, 0, 0)), tile)

    back = Image.new("L", (S, S), 0)
    front = Image.new("L", (S, S), 0)
    ImageDraw.Draw(back).rounded_rectangle(
        (CARD_X1 * K, CARD_Y1 * K, (CARD_X1 + CARD) * K, (CARD_Y1 + CARD) * K),
        radius=CARD_RAD * K,
        fill=248,
    )
    ImageDraw.Draw(front).rounded_rectangle(
        (CARD_X0 * K, CARD_Y0 * K, (CARD_X0 + CARD) * K, (CARD_Y0 + CARD) * K),
        radius=CARD_RAD * K,
        fill=255,
    )
    img = Image.composite(Image.new("RGBA", (S, S), CARD_BACK), img, back)
    img = Image.composite(Image.new("RGBA", (S, S), (255, 255, 255, 255)), img, front)

    dm = ImageDraw.Draw(img)
    f_a = ImageFont.truetype("C:/Windows/Fonts/arialbd.ttf", A_SIZE * K)
    f_ko = ImageFont.truetype("C:/Windows/Fonts/malgunbd.ttf", KO_SIZE * K)
    dm.text(
        ((CARD_X1 + CARD / 2) * K, (CARD_Y1 + CARD / 2) * K),
        "A",
        font=f_a,
        fill=CYAN,
        anchor="mm",
    )
    dm.text(
        ((CARD_X0 + CARD / 2) * K, (CARD_Y0 + CARD / 2) * K),
        "\uac00",
        font=f_ko,
        fill=BLUE,
        anchor="mm",
    )
    return img.resize((M, M), Image.LANCZOS)


ICO_SIZES = [
    (16, 16),
    (20, 20),
    (24, 24),
    (32, 32),
    (48, 48),
    (64, 64),
    (128, 128),
    (256, 256),
]

img = master()
img.save(ASSETS / "icon-512.png")
img.save(ASSETS / "hymt.ico", sizes=ICO_SIZES)
print("assets written:", ASSETS)
