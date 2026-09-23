#!/usr/bin/env python3
"""Regenerate the TABLE in crates/mui-scene/src/material_symbols.rs.

    python3 tools/material_symbols_table.py \
        'MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].codepoints'

The input is the `name hex` per line file from google/material-design-icons
(variablefont/). Rewrites everything from `#[rustfmt::skip]` to the end of the
file, sorted by name bytes, which is what `codepoint`'s binary search needs.
"""
import pathlib
import sys

RS = pathlib.Path(__file__).resolve().parent.parent / "crates/mui-scene/src/material_symbols.rs"

pairs = {}
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    if line.strip():
        name, hexcode = line.split()
        pairs[name] = int(hexcode, 16)
head = RS.read_text().split("#[rustfmt::skip]")[0]
rows = "".join(
    f"    (\"{n}\", '\\u{{{c:04X}}}'),\n" for n, c in sorted(pairs.items(), key=lambda kv: kv[0].encode())
)
RS.write_text(head + "#[rustfmt::skip]\nstatic TABLE: &[(&str, char)] = &[\n" + rows + "];\n")
