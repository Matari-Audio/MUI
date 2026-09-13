export const q = (s: string) => {
  let out = '"';
  for (const ch of s) {
    const code = ch.codePointAt(0)!;
    if (code >= 0xd800 && code <= 0xdfff) throw new Error("unpaired surrogate in string");
    if (ch === '"' || ch === "\\") out += "\\" + ch;
    else if (code < 32 || code === 127) out += `\\u{${code.toString(16)}}`;
    else out += ch;
  }
  return out + '"';
};
export const n = (v: number) => {
  if (!Number.isFinite(v)) throw new Error(`non-finite number: ${v}`);
  return `${Object.is(v,-0) ? "-0" : String(v)}f64`;
};
