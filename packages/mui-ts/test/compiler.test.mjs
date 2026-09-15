import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { promisify } from "node:util";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";

const run = promisify(execFile);
const packageRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const compiler = join(packageRoot, "dist/src/compiler.js");
const index = pathToFileURL(join(packageRoot, "dist/src/index.js")).href;
const imports = `import { defineScene, frameSurface, insetSurface, leaf, px } from ${JSON.stringify(index)};\n`;

async function generate(t, name, body) {
  const directory = await mkdtemp(join(tmpdir(), "mui-ts-generator-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const input = join(directory, `${name}.mjs`);
  const output = join(directory, `${name}.rs`);
  await writeFile(input, `${imports}${body}\n`, "utf8");
  await run(process.execPath, [compiler, input, output], { cwd: packageRoot });
  return { output, source: await readFile(output, "utf8") };
}

async function rustfmt(source) {
  await run("rustfmt", ["--edition", "2021", "--emit", "stdout", source], {
    cwd: packageRoot,
    maxBuffer: 1024 * 1024,
  });
}

test("normal scenes get a Rust default palette and parse as Rust", async (t) => {
  const generated = await generate(t, "default-palette", `
    export default defineScene({ root: leaf([20, 10]), surfaces: [] });
  `);
  assert.match(generated.source, /palette: mui_core::Palette \{ \.\.mui_core::Palette::NEUTRAL \}/);
  await rustfmt(generated.output);
});

test("Rust escaping preserves controls and valid astral characters", async (t) => {
  const id = "ctl\u0001\b\f\u0000\n\t\"\\";
  const astral = "font😀";
  const generated = await generate(t, "escaping", `
    const id = ${JSON.stringify(id)};
    const astral = ${JSON.stringify(astral)};
    export default defineScene({
      root: leaf([20, 10], { id }),
      surfaces: [frameSurface(id), insetSurface(astral, id, px(2))],
    });
  `);
  for (const escape of ["\\u{1}", "\\u{8}", "\\u{c}", "\\u{0}", "\\n", "\\t", '\\"', "\\\\"]) {
    assert.ok(generated.source.includes(escape), `missing Rust escape ${escape}`);
  }
  assert.ok(generated.source.includes(astral));
  await rustfmt(generated.output);
});

test("unpaired UTF-16 surrogates are rejected before output replacement", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "mui-ts-generator-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  for (const [name, bad] of [["high", "bad\ud800"], ["low", "\udc00bad"]]) {
    const input = join(directory, `${name}.mjs`);
    const output = join(directory, `${name}.rs`);
    const source = `${imports}const bad = ${JSON.stringify(bad)};\nexport default defineScene({ root: leaf([20, 10], { id: bad }), surfaces: [] });\n`;
    await writeFile(input, source, "utf8");
    await writeFile(output, "sentinel\n", "utf8");
    await assert.rejects(
      run(process.execPath, [compiler, input, output], { cwd: packageRoot }),
      (error) => /unpaired (?:high|low) surrogate/.test(`${error.message}\n${error.stderr}`),
    );
    assert.equal(await readFile(output, "utf8"), "sentinel\n");
  }
});

test("exponent-form finite numbers remain valid Rust literals", async (t) => {
  const generated = await generate(t, "exponent", `
    export default defineScene({ root: leaf([1e21, 1e-7]), surfaces: [] });
  `);
  assert.match(generated.source, /leaf\(1e\+21, 1e-7\)/);
  assert.doesNotMatch(generated.source, /1e\+21\.0/);
  await rustfmt(generated.output);
});

test("non-finite numbers are rejected", async (t) => {
  for (const [name, value] of [["nan", "NaN"], ["positive-infinity", "Infinity"], ["negative-infinity", "-Infinity"]]) {
    const directory = await mkdtemp(join(tmpdir(), "mui-ts-generator-"));
    t.after(() => rm(directory, { recursive: true, force: true }));
    const input = join(directory, `${name}.mjs`);
    const output = join(directory, `${name}.rs`);
    await writeFile(input, `${imports}export default defineScene({ root: leaf([${value}, 10]), surfaces: [] });\n`, "utf8");
    await assert.rejects(
      run(process.execPath, [compiler, input, output], { cwd: packageRoot }),
      (error) => /non-finite number/.test(`${error.message}\n${error.stderr}`),
    );
  }
});
