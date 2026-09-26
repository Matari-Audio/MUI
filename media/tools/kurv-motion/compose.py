#!/usr/bin/env python3
"""Bundle the captured scene and shared browser adapter into the Kurv film."""
import json
from pathlib import Path
import shutil

ROOT=Path(__file__).resolve().parents[3]
PROJECT=ROOT/'videos/kurv-unfold'
scene=json.loads((PROJECT/'assets/kurv/scene.json').read_text())
shutil.copyfile(ROOT/'tools/film/layers.mjs',PROJECT/'assets/layers.mjs')
(PROJECT/'assets/scene.mjs').write_text('export default '+json.dumps(scene,separators=(',',':'))+';\n')
