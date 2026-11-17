"""Generate static Regular/Bold/Italic/BoldItalic instances from iA's variable fonts.

GPUI's Linux text stack picks faces by family/weight/style and cannot set
variation axes, so we bake the axes in. Values follow iA's own typography
table (TypographyKit *.plist): wght 455 for body text (the plist subtracts 35
only on macOS, whose renderer emboldens text), 700 for strong. The plist's
spacing values ("15" for Mono/Duo, "1" for Quattro at regular width) are in
tenths of an axis unit: 1.5 reproduces the advance widths measured on iA's
own screenshots (≈0.6 em), while 15 would be 5 % too wide.

Run: python -m venv v && v/bin/pip install fonttools && v/bin/python tools/make_static_fonts.py
"""
from pathlib import Path

from fontTools.ttLib import TTFont
from fontTools.varLib.instancer import instantiateVariableFont

ROOT = Path(__file__).resolve().parent.parent / "assets" / "fonts"
OUT = ROOT / "static"
OUT.mkdir(exist_ok=True)

STYLES = [
    ("Regular", 455, False),
    ("Bold", 700, False),
    ("Italic", 455, True),
    ("Bold Italic", 700, True),
]

SPACING = {"Duo": 1.5, "Mono": 1.5, "Quattro": 0.1}

for family in ["Duo", "Mono", "Quattro"]:
    for style, weight, italic in STYLES:
        src = ROOT / f"iAWriter{family}{'-Italic' if italic else ''}.ttf"
        font = instantiateVariableFont(TTFont(src), {"wght": weight, "SPCG": SPACING[family]})
        name = font["name"]
        fam = f"iA Writer {family}"
        for rec in list(name.names):
            if rec.nameID in (16, 17, 21, 22, 25):
                name.removeNames(nameID=rec.nameID)
        name.setName(fam, 1, 3, 1, 0x409)
        name.setName(style, 2, 3, 1, 0x409)
        name.setName(f"{fam} {style}", 4, 3, 1, 0x409)
        name.setName(f"iAWriter{family}-{style.replace(' ', '')}", 6, 3, 1, 0x409)
        os2 = font["OS/2"]
        os2.usWeightClass = 700 if weight >= 700 else 400
        bold = weight >= 700
        os2.fsSelection = (os2.fsSelection & ~0b1100001) | (0x20 if bold else 0) | (0x01 if italic else 0)
        if not bold and not italic:
            os2.fsSelection |= 0x40
        font["head"].macStyle = (1 if bold else 0) | (2 if italic else 0)
        for t in ("STAT",):
            if t in font:
                del font[t]
        out = OUT / f"iAWriter{family}-{style.replace(' ', '')}.ttf"
        font.save(out)
        print(out.name)
