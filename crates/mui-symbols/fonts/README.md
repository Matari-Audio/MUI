# Material Symbols variable fonts

The full upstream Material Symbols fonts (axes FILL, GRAD, opsz, wght),
Apache-2.0 (`LICENSE`), from google/material-design-icons at commit
`10ecc14790612ca8fd164e3f4ffe9c16fc50d5b7` (2026-10-23), directory
`variablefont/`:

    https://raw.githubusercontent.com/google/material-design-icons/10ecc14790612ca8fd164e3f4ffe9c16fc50d5b7/variablefont/

| File here | Upstream name | Bytes | sha256 |
|---|---|---|---|
| `MaterialSymbolsRounded.ttf` | `MaterialSymbolsRounded[FILL,GRAD,opsz,wght].ttf` | 15235612 | `7e74e36533ccee12c2b81e75bfcf3ccf123d3bb7e7e46d65d94445f23db0c97c` |
| `MaterialSymbolsOutlined.ttf` | `MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].ttf` | 10744832 | `f7837ba8573d9a7a74931618ced8307bc81f96d49af296bf6c8052c772ed5282` |
| `MaterialSymbols.codepoints` | `MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].codepoints` | 79421 | `a7404d9e5a1b63f496cf05ace45d386075886b5a7f123fba9f29527cb6006fde` |
| `LICENSE` | `LICENSE` (repo root) | 11357 | `58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd` |

The Rounded and Outlined `.codepoints` files are byte-identical, so one is
kept; `src/lib.rs` is generated from it by `tools/material_symbols_table.py`,
and `tools/subset_symbols.py` reads it to cut a small subset (see the crate
docs).

To update: download the four files at a newer commit, rename them as above,
update this table, and rerun `material_symbols_table.py` on the codepoints.
