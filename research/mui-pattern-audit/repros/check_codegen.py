"""Check formerly failing generator cases now compile successfully.
Run with Python 3 and installed TS dependencies; no workspace output is changed.
"""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

repo = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[3]
package = repo / "packages/mui-ts"

def run(args, **kwargs):
    return subprocess.run(args, text=True, capture_output=True, **kwargs)

with tempfile.TemporaryDirectory(prefix="mui-codegen-audit-") as directory:
    temp = Path(directory)
    dist = temp / "dist"
    built = run([str(package / "node_modules/.bin/tsc"), "-p", str(package / "tsconfig.json"), "--outDir", str(dist)])
    assert built.returncode == 0, built.stdout + built.stderr
    (dist / "package.json").write_text('{"type":"module"}')
    (temp / "src").mkdir()
    (temp / "Cargo.toml").write_text(
        '[package]\nname="mui-codegen-audit"\nversion="0.0.0"\nedition="2021"\n'
        '[workspace]\n[dependencies]\n' + ''.join(
            f'{name}={{path={json.dumps(str(repo / "crates" / name))}}}\n'
            for name in ["mui-core", "mui-layout"]))
    (temp / "src/main.rs").write_text('mod generated; fn main() { let _ = generated::generated_scene(); }')
    cases = [
        ("control", 'root: leaf([1,1]), theme: {palette: {}}, surfaces: []'),
        ("missing_palette", 'root: leaf([1,1]), surfaces: []'),
        ("control_character_id", 'root: leaf([1,1], {id: "unit\\u0001"}), theme: {palette: {}}, surfaces: []'),
        ("integer_exponent", 'root: leaf([1e21,1]), theme: {palette: {}}, surfaces: []'),
    ]
    for name, scene in cases:
        fixture = temp / "fixture.mjs"
        fixture.write_text(f'import {{defineScene, leaf}} from {json.dumps((dist / "src/index.js").as_uri())};\n'
                           + f'export default defineScene({{{scene}}});\n')
        generated = run(["node", str(dist / "src/compiler.js"), str(fixture), str(temp / "src/generated.rs")])
        assert generated.returncode == 0, generated.stderr
        checked = run(["cargo", "check", "--manifest-path", str(temp / "Cargo.toml"), "--offline"])
        assert checked.returncode == 0, checked.stderr
        print(f"PASS {name}: generated Rust compiles")
