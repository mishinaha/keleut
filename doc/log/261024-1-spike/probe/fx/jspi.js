const u = (n) => { const o = []; do { let b = n & 0x7f; n >>>= 7; if (n) b |= 0x80; o.push(b); } while (n); return o; };
const vec = (items) => [...u(items.length), ...items.flat()];
const sec = (id, body) => [id, ...u(body.length), ...body];
const str = (t) => [...u(t.length), ...Buffer.from(t)];
const b = [1, 1, 0x7f, 0x03,0x40, 0x20,1, 0x10,0, 0x6a, 0x21,1, 0x20,0, 0x41,1, 0x6b, 0x22,0, 0x0d,0, 0x0b, 0x20,1, 0x0b];
const bytes = new Uint8Array([0,0x61,0x73,0x6d,1,0,0,0,
  ...sec(1, vec([[0x60,0,1,0x7f],[0x60,1,0x7f,1,0x7f]])),
  ...sec(2, vec([[...str("e"), ...str("s"), 0x00, 0]])),
  ...sec(3, vec([[1]])),
  ...sec(7, vec([[...str("run"), 0, 1]])),
  ...sec(10, vec([[...u(b.length), ...b]]))]);
const m = new WebAssembly.Module(bytes);
(async () => {
  for (const mode of ['sync', 'jspi']) {
    const s = mode === 'sync' ? () => 1 : new WebAssembly.Suspending(async () => 1);
    const inst = new WebAssembly.Instance(m, { e: { s } });
    const run = mode === 'sync' ? inst.exports.run : WebAssembly.promising(inst.exports.run);
    const N = 1000000;
    const t0 = process.hrtime.bigint();
    const r = await run(N);
    const t1 = process.hrtime.bigint();
    console.log(mode, 'result', r, 'ns/switch', Number(t1 - t0) / N);
  }
})();
