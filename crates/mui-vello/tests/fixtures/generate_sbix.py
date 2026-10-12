"""Generate MUI's original synthetic bitmap font; no third-party glyph artwork.
Requires fontTools and Pillow. The single 8x8 green glyph has a deliberately
nonzero sbix bearing and a non-Apple PostScript name for placement regressions.
"""
from pathlib import Path
from io import BytesIO
from PIL import Image
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import newTable
from fontTools.ttLib.tables.sbixStrike import Strike
from fontTools.ttLib.tables.sbixGlyph import Glyph

builder = FontBuilder(1024, isTTF=True)
names = [".notdef", "space", "bitmap"]
builder.setupGlyphOrder(names)
builder.setupCharacterMap({32: "space", 0xE000: "bitmap"})
builder.setupGlyf({name: TTGlyphPen(None).glyph() for name in names})
builder.setupHorizontalMetrics({name: (1024, 0) for name in names})
builder.setupHorizontalHeader(ascent=768, descent=-256)
builder.setupOS2(sTypoAscender=768, sTypoDescender=-256, usWinAscent=768, usWinDescent=256)
builder.setupNameTable({"familyName": "MUI Bitmap Fixture", "styleName": "Regular", "uniqueFontIdentifier": "MUIBitmapFixture", "fullName": "MUI Bitmap Fixture", "psName": "MUIBitmapFixture", "version": "Version 1.0"})
builder.setupPost()
image = BytesIO()
Image.new("RGBA", (8, 8), (0, 255, 0, 255)).save(image, format="PNG")
strike = Strike(ppem=32, resolution=72)
strike.glyphs["bitmap"] = Glyph(glyphName="bitmap", originOffsetX=6, originOffsetY=4, graphicType="png ", imageData=image.getvalue())
sbix = newTable("sbix")
sbix.version = 1
sbix.flags = 1
sbix.strikes = {32: strike}
builder.font["sbix"] = sbix
# Fixed timestamps make regeneration deterministic.
builder.font["head"].created = 2082844800
builder.font["head"].modified = 2082844800
builder.font.recalcTimestamp = False
builder.save(Path(__file__).with_name("non_apple_sbix.ttf"))
