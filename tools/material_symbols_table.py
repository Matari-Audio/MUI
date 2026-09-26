#!/usr/bin/env python3
"""Regenerate crates/mui-symbols/src/lib.rs below its header.

    python3 tools/material_symbols_table.py \
        'MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].codepoints'

The input is the `name hex` per line file from google/material-design-icons
(variablefont/). Rewrites everything from the first `#[rustfmt::skip]` to the
end of the file: one `sym::NAME` const per name (a leading digit gets a `_`),
and the name table sorted by name bytes, which `codepoint`'s binary search
needs.
"""
import pathlib
import sys

RS = pathlib.Path(__file__).resolve().parent.parent / "crates/mui-symbols/src/lib.rs"

pairs = {}
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    if line.strip():
        name, hexcode = line.split()
        pairs[name] = int(hexcode, 16)
names = sorted(pairs, key=str.encode)


def const(n):
    c = n.upper()
    return "_" + c if c[0].isdigit() else c


assert len({const(n) for n in names}) == len(names), "two names map to one const"
head = RS.read_text().split("#[rustfmt::skip]")[0]
consts = "".join(f"    pub const {const(n)}: char = '\\u{{{pairs[n]:04X}}}';\n" for n in names)
rows = "".join(f"    (\"{n}\", sym::{const(n)}),\n" for n in names)
RS.write_text(
    head
    + "#[rustfmt::skip]\n/// One `char` per symbol: `sym::HOME`, `sym::_10K`.\npub mod sym {\n"
    + consts
    + "}\n\n#[rustfmt::skip]\nstatic TABLE: &[(&str, char)] = &[\n"
    + rows
    + "];\n"
)
