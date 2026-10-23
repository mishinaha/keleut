const u = (n) => { const o = []; do { let b = n & 0x7f; n >>>= 7; if (n) b |= 0x80; o.push(b); } while (n); return o; };
const vec = (items) => [...u(items.length), ...items.flat()];
const sec = (id, body) => [id, ...u(body.length), ...body];
const str = (t) => [...u(t.length), ...Buffer.from(t)];
const bd = (code) => { const b = [0x00, ...code, 0x0b]; return [...u(b.length), ...b]; };
const bytes = new Uint8Array([0,0x61,0x73,0x6d,1,0,0,0,
  ...sec(1, vec([[0x60,1,0x7f,1,0x7f],[0x60,0,1,0x7f]])),
  ...sec(3, vec([[0],[1],[1]])),
  ...sec(7, vec([[...str("h"), 0, 1],[...str("dz"), 0, 2]])),
  ...sec(10, vec([
    bd([0x20,0, 0x41,1, 0x6a, 0x10,0, 0x41,1, 0x6a]),
    bd([0x02,0x40, 0x1f,0x40, 1, 0x02,0, 0x41,0, 0x10,0, 0x1a, 0x0b, 0x41,1, 0x0f, 0x0b, 0x41,2]),
    // divide by zero trap inside try_table catch_all
    bd([0x02,0x40, 0x1f,0x40, 1, 0x02,0, 0x41,1, 0x41,0, 0x6d, 0x1a, 0x0b, 0x41,1, 0x0f, 0x0b, 0x41,2])]))]);
const i = new WebAssembly.Instance(new WebAssembly.Module(bytes));
for (const f of ['h', 'dz']) { try { console.log(f, 'returned', i.exports[f](), '(2 = caught inside wasm)'); } catch (e) { console.log(f, 'escaped to JS:', e.constructor.name, e.message); } }
