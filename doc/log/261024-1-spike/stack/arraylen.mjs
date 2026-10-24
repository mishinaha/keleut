// Measures the largest length that V8 accepts for array.new and array.new_default on
// (array (mut i8)) and (array (mut eqref)), and for array.new_default on
// (array (mut i64)), and what happens just beyond it and for a negative length (an i32
// operand is read as u32, so -1 is 2^32 - 1).
// The module is generated as bytes by wasmgen.mjs; nothing has to be built first.
// Usage: node arraylen.mjs   (allocates arrays of up to about 1 GiB)
import { I, T, module, funcType, arrayType } from "./wasmgen.mjs";

const bytes = module({
  types: [arrayType(T.i8, true), arrayType(T.eqref, true), funcType([T.i32], [T.i32]), arrayType(T.i64, true)],
  funcs: [
    { type: 2, locals: [], body: [...I.i32Const(7), ...I.localGet(0), ...I.arrayNew(0), ...I.arrayLen], export: "new_i8" },
    { type: 2, locals: [], body: [...I.localGet(0), ...I.arrayNewDefault(0), ...I.arrayLen], export: "new_default_i8" },
    { type: 2, locals: [], body: [...I.i32Const(7), ...I.refI31, ...I.localGet(0), ...I.arrayNew(1), ...I.arrayLen], export: "new_eqref" },
    { type: 2, locals: [], body: [...I.localGet(0), ...I.arrayNewDefault(1), ...I.arrayLen], export: "new_default_eqref" },
    { type: 2, locals: [], body: [...I.localGet(0), ...I.arrayNewDefault(3), ...I.arrayLen], export: "new_default_i64" },
  ],
});
const { exports } = new WebAssembly.Instance(new WebAssembly.Module(bytes));

function attempt(f, n) {
  try {
    const len = f(n | 0) >>> 0;
    if (len !== n >>> 0) throw new Error(`array.len ${len} != ${n >>> 0}`);
    return { ok: true };
  } catch (e) {
    if (!(e instanceof WebAssembly.RuntimeError)) throw e;
    return { ok: false, error: `${e.constructor.name}: ${e.message}` };
  }
}

console.log(`node ${process.version}  v8 ${process.versions.v8}`);
console.log("function            max length   (= 2^k - d)  bytes of payload  beyond max                                         length -1");
for (const name of ["new_i8", "new_default_i8", "new_eqref", "new_default_eqref", "new_default_i64"]) {
  const f = exports[name];
  // Binary search over 1 .. 2^32 - 1.
  let lo = 1, hi = 2 ** 32 - 1;
  if (!attempt(f, lo).ok) throw new Error(`${name}: length 1 fails`);
  while (lo < hi) {
    const m = Math.floor((lo + hi + 1) / 2);
    if (attempt(f, m).ok) lo = m;
    else hi = m - 1;
  }
  const beyond = attempt(f, lo + 1);
  const neg = attempt(f, -1);
  const elem = name.endsWith("i8") ? 1 : 8;
  console.log(`${name.padEnd(18)}  ${String(lo).padStart(10)}   ${`2^${Math.ceil(Math.log2(lo))} - ${2 ** Math.ceil(Math.log2(lo)) - lo}`.padStart(10)}   ${String(lo * elem).padStart(16)}  ${(beyond.ok ? "ok?" : beyond.error).padEnd(49)}  ${neg.ok ? "ok?" : neg.error}`);
}
