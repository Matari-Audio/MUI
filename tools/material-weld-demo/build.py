#!/usr/bin/env python3
"""Build the self-contained offline demo without downloads or dependencies."""
from pathlib import Path
import argparse
import re

ROOT = Path(__file__).resolve().parent

def build() -> str:
    template = (ROOT / 'template.html').read_text(encoding='utf-8')
    engine = (ROOT / 'engine.mjs').read_text(encoding='utf-8')
    app = (ROOT / 'app.js').read_text(encoding='utf-8')
    for marker in ('/*__ENGINE__*/', '/*__APP__*/'):
        if template.count(marker) != 1:
            raise ValueError(f'Expected one {marker}')
    engine = re.sub(r'\bexport\s+', '', engine)
    if re.search(r'</script', engine + app, re.I):
        raise ValueError('Script closing tag cannot be embedded safely')
    return template.replace('/*__ENGINE__*/', engine).replace('/*__APP__*/', app)

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    expected = build()
    target = ROOT / 'index.html'
    if args.check:
        if not target.exists() or target.read_text(encoding='utf-8') != expected:
            raise SystemExit('The offline HTML differs from its source. Run build.py.')
        print('Offline HTML matches its source.')
    else:
        target.write_text(expected, encoding='utf-8')
        print(target)
