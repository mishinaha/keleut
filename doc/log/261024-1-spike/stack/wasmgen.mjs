// A tiny WebAssembly binary writer used by bigframe.mjs and arraylen.mjs.
// The modules are built as bytes in JS so that the scripts run from a fresh clone
// without any build step. Each built module is validated with WebAssembly.validate.

export const T = { i32: 0x7f, i64: 0x7e, eqref: 0x6d, i31ref: 0x6c, i8: 0x78 };

export function uleb(n) {
  n = BigInt(n);
  const out = [];
  do {
    let b = Number(n & 0x7fn);
    n >>= 7n;
    if (n !== 0n) b |= 0x80;
    out.push(b);
  } while (n !== 0n);
  return out;
}

export function sleb(n) {
  n = BigInt(n);
  const out = [];
  for (;;) {
    const b = Number(n & 0x7fn);
    n >>= 7n;
    if ((n === 0n && (b & 0x40) === 0) || (n === -1n && (b & 0x40) !== 0)) {
      out.push(b);
      return out;
    }
    out.push(b | 0x80);
  }
}

export const vec = (items) => [...uleb(items.length), ...items.flat()];
const section = (id, body) => [id, ...uleb(body.length), ...body];
const name = (s) => vec([...Buffer.from(s, "utf8")]);

// Instructions used by the generators.
export const I = {
  localGet: (i) => [0x20, ...uleb(i)],
  localSet: (i) => [0x21, ...uleb(i)],
  globalGet: (i) => [0x23, ...uleb(i)],
  globalSet: (i) => [0x24, ...uleb(i)],
  call: (f) => [0x10, ...uleb(f)],
  i32Const: (v) => [0x41, ...sleb(v)],
  i64Const: (v) => [0x42, ...sleb(v)],
  i64Load: (offset) => [0x29, 3, ...uleb(offset)],
  i64Eqz: [0x50],
  i64Add: [0x7c],
  i64Sub: [0x7d],
  i32Add: [0x6a],
  ifEmpty: [0x04, 0x40],
  end: [0x0b],
  ret: [0x0f],
  drop: [0x1a],
  refI31: [0xfb, 0x1c],
  i31GetS: [0xfb, 0x1d],
  refCastI31: [0xfb, 0x16, T.i31ref],
  arrayNew: (t) => [0xfb, 0x06, ...uleb(t)],
  arrayNewDefault: (t) => [0xfb, 0x07, ...uleb(t)],
  arrayGet: (t) => [0xfb, 0x0b, ...uleb(t)],
  arrayLen: [0xfb, 0x0f],
};

// types: list of byte arrays (one composite type each).
// funcs: [{ type, locals: [[count, valtype], ...], body: [...bytes], export? }]
// globals: [{ type: [...valtype bytes], mut: bool, init: [...expr bytes without end] }]
export function module({ types, funcs, globals = [], memoryPages = 0 }) {
  const out = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
  out.push(...section(1, vec(types)));
  out.push(...section(3, vec(funcs.map((f) => uleb(f.type)))));
  if (memoryPages > 0) out.push(...section(5, vec([[0x00, ...uleb(memoryPages)]])));
  if (globals.length > 0) {
    out.push(...section(6, vec(globals.map((g) => [...g.type, g.mut ? 1 : 0, ...g.init, 0x0b]))));
  }
  const exps = funcs.flatMap((f, i) => (f.export ? [[...name(f.export), 0x00, ...uleb(i)]] : []));
  out.push(...section(7, vec(exps)));
  const bodies = funcs.map((f) => {
    const b = [...vec(f.locals.map(([n, t]) => [...uleb(n), t])), ...f.body, 0x0b];
    return [...uleb(b.length), ...b];
  });
  out.push(...section(10, vec(bodies)));
  const bytes = new Uint8Array(out);
  if (!WebAssembly.validate(bytes)) throw new Error("generated module does not validate");
  return bytes;
}

export const funcType = (params, results) => [0x60, ...vec(params.map((p) => [p].flat())), ...vec(results.map((r) => [r].flat()))];
export const arrayType = (storage, mutable) => [0x5e, ...[storage].flat(), mutable ? 1 : 0];
export const refNull = (typeIdx) => [0x63, ...sleb(typeIdx)];
export const ref = (typeIdx) => [0x64, ...sleb(typeIdx)];
