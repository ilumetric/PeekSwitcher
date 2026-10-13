"""Renders the app icon (app/icons) and tray icons (app/icons/tray) with Pillow.

App icon: black squircle with "Й"; small sizes (title bar) show "Q", the same key in English.
Tray icons: rounded badge with the current layout ("EN"/"RU"); grey when auto switching is off,
an empty outline when the whole app is turned off.
Run:  python tools/gen_icons.py
"""
import os

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "app", "icons")
TRAY = os.path.join(ICONS, "tray")
FONT_BOLD = r"C:\Windows\Fonts\segoeuib.ttf"
FONT_SEMI = r"C:\Windows\Fonts\seguisb.ttf"


def squircle_mask(size, n=5.0, inset=0.14):
    """Superellipse |x|^n + |y|^n = 1, like an iOS icon; drawn 4x and downsampled."""
    big = size * 4
    mask = Image.new("L", (big, big), 0)
    px = mask.load()
    r = big * (0.5 - inset)
    c = big / 2
    for y in range(big):
        dy = abs((y + 0.5 - c) / r)
        if dy > 1:
            continue
        half = r * (1 - dy ** n) ** (1 / n)
        for x in range(int(c - half), int(c + half)):
            px[x, y] = 255
    return mask.resize((size, size), Image.LANCZOS)


def centered_text(draw, box, text, font, fill):
    x0, y0, x1, y1 = box
    l, t, r, b = draw.textbbox((0, 0), text, font=font)
    x = x0 + (x1 - x0 - (r - l)) / 2 - l
    y = y0 + (y1 - y0 - (b - t)) / 2 - t
    draw.text((x, y), text, font=font, fill=fill)


def app_icon(letter, size=1024, inset=0.14, height=0.40):
    """Black squircle with one white letter; `height` is the glyph height relative to the icon."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    black = Image.new("RGBA", (size, size), (0, 0, 0, 255))
    img.paste(black, (0, 0), squircle_mask(size, inset=inset))
    d = ImageDraw.Draw(img)
    probe = ImageFont.truetype(FONT_BOLD, 100)
    l, t, r, b = d.textbbox((0, 0), letter, font=probe)
    font = ImageFont.truetype(FONT_BOLD, int(100 * size * height / (b - t)))
    centered_text(d, (0, 0, size, size), letter, font, (255, 255, 255, 255))
    return img


def tray_icon(text, state, size=128):
    """state: "on", "off" (auto switching off) or "disabled" (the app is turned off)."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    box = (4, 14, size - 5, size - 15)
    font = ImageFont.truetype(FONT_BOLD, int(size * 0.5))
    if state == "disabled":
        grey = (150, 150, 150, 255)
        d.rounded_rectangle(box, radius=int(size * 0.2), outline=grey, width=int(size * 0.07))
        centered_text(d, box, text, font, grey)
        return img
    fill = (236, 236, 236, 255) if state == "on" else (120, 120, 120, 255)
    d.rounded_rectangle(box, radius=int(size * 0.2), fill=fill)
    centered_text(d, box, text, font, (23, 23, 23, 255))
    return img


# Sizes Windows uses for small icons (title bar, small lists); larger ones get the big icon.
SMALL = [16, 20, 24, 32]
LARGE = [40, 48, 64, 128, 256]


def main():
    os.makedirs(TRAY, exist_ok=True)
    big = app_icon("Й")
    # Small icons fill more of the square so the letter stays readable at 16-24 px.
    small = app_icon("Q", inset=0.03, height=0.62)
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(ICONS, "icon.png"))
    frames = [small.resize((n, n), Image.LANCZOS) for n in SMALL] + [big.resize((n, n), Image.LANCZOS) for n in LARGE]
    frames[-1].save(
        os.path.join(ICONS, "icon.ico"),
        sizes=[(n, n) for n in SMALL + LARGE],
        append_images=frames[:-1],
    )
    for lang in ("en", "ru"):
        for state, suffix in (("on", ""), ("off", "_off"), ("disabled", "_disabled")):
            icon = tray_icon(lang.upper(), state).resize((32, 32), Image.LANCZOS)
            icon.save(os.path.join(TRAY, f"{lang}{suffix}.png"))
    print("icons written to", ICONS)


if __name__ == "__main__":
    main()
