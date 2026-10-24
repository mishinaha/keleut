// rtcore の libm の試験の wasm32 側。
// native が書いた入力を rt.wasm の krt_sin などに渡し、x86_64 版の結果とビット列で比べる。
// 使い方: node check.mjs OUTDIR(OUTDIR に inputs.bin、native.bin、rt.wasm がある)
// NaN どうしは積載と符号に依らず一致とみなす(Keleut の show は NaN を常に nan と書く)。
// 積載か符号だけが違う NaN の数は、参考として別に出す。
// ただし入力と結果は JS の Number を通るので、V8 が NaN の積載を正規化してもこの試験では分からない。
import { readFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
if (!dir) { console.error("usage: node check.mjs OUTDIR"); process.exit(2); }
const buf = (p) => { const b = readFileSync(join(dir, p)); return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength); };
const inputs = new Float64Array(buf("inputs.bin"));
const native = new BigUint64Array(buf("native.bin"));
const n = inputs.length;
if (native.length !== 4 * n) { console.error("size mismatch"); process.exit(2); }

const mod = new WebAssembly.Module(readFileSync(join(dir, "rt.wasm")));
const imports = WebAssembly.Module.imports(mod);
console.log(`rt.wasm imports: ${imports.length}`);
if (imports.length !== 0) { console.error(imports); process.exit(1); }
const { exports } = new WebAssembly.Instance(mod, {});

const tmp = new Float64Array(1);
const tmpBits = new BigUint64Array(tmp.buffer);
const isNaNBits = (b) => (b & 0x7fffffffffffffffn) > 0x7ff0000000000000n;
let total = 0;
for (const [j, name] of ["sin", "cos", "exp", "log"].entries()) {
  const f = exports[`krt_${name}`];
  let diff = 0, nanPayload = 0;
  for (let i = 0; i < n; i++) {
    tmp[0] = f(inputs[i]);
    const w = tmpBits[0], x = native[j * n + i];
    if (w === x) continue;
    if (isNaNBits(w) && isNaNBits(x)) { nanPayload++; continue; }
    if (diff < 5) {
      console.log(`  ${name}(${inputs[i]}): x86_64 0x${x.toString(16)} wasm32 0x${w.toString(16)}`);
    }
    diff++;
  }
  console.log(`x86_64 vs wasm32 ${name}: ${diff} differ of ${n} (NaN differing only in payload or sign: ${nanPayload})`);
  total += diff;
}
console.log(`differences: ${total}`);
process.exit(total === 0 ? 0 : 1);
