#!/usr/bin/env python3
"""Cut a Material Symbols subset holding only the symbols an app draws.

    python3 tools/subset_symbols.py FONT OUT NAME_OR_DIR...

FONT is a Material Symbols variable font (crates/mui-symbols/fonts/), OUT the
subset to write. Each remaining argument is a symbol name (`home`) or a
directory scanned for `sym::NAME` in .rs files. Names map to codepoints via
the `.codepoints` file next to FONT (or `--codepoints PATH`). Every axis
stays, and `--layout-features='*'` keeps the GSUB FeatureVariations that swap
in the filled glyph at FILL 1. Needs fontTools (`pip install fonttools`).
"""
import argparse
import pathlib
import re
import sys

from fontTools import subset

SYM = re.compile(r"\bsym::(_?[A-Z0-9][A-Z0-9_]*)\b")


def wanted(args):
    """Explicit names, and names found by scanning (unchecked: a scan can
    match `sym::NAME` in prose)."""
    names, scanned = set(), set()
    for a in args:
        p = pathlib.Path(a)
        if p.is_dir():
            for rs in p.rglob("*.rs"):
                for c in SYM.findall(rs.read_text(encoding="utf-8", errors="replace")):
                    # The inverse of material_symbols_table.py's const().
                    scanned.add((c[1:] if c[0] == "_" and c[1].isdigit() else c).lower())
        else:
            names.add(a)
    return names, scanned


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("font", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("names", nargs="+", help="symbol names or Rust source dirs")
    ap.add_argument("--codepoints", type=pathlib.Path)
    a = ap.parse_args()

    cp_file = a.codepoints or next(a.font.parent.glob("*.codepoints"), None)
    if cp_file is None:
        sys.exit(f"no .codepoints next to {a.font}; pass --codepoints")
    table = dict(line.split() for line in cp_file.read_text().splitlines() if line.strip())

    names, scanned = wanted(a.names)
    missing = sorted(n for n in names if n not in table)
    if missing:
        sys.exit(f"not in {cp_file.name}: {' '.join(missing)}")
    for n in sorted(scanned - table.keys()):
        print(f"skipping sym::{n.upper()}: not in {cp_file.name}", file=sys.stderr)
    names |= scanned & table.keys()
    if not names:
        sys.exit("no symbols found")
    unicodes = ",".join(sorted({table[n] for n in names}))

    a.out.parent.mkdir(parents=True, exist_ok=True)
    subset.main([
        str(a.font),
        f"--unicodes={unicodes}",
        "--layout-features=*",
        "--no-hinting",
        f"--output-file={a.out}",
    ])
    print(f"{a.out}: {len(names)} symbols, {a.out.stat().st_size} bytes")


if __name__ == "__main__":
    main()
