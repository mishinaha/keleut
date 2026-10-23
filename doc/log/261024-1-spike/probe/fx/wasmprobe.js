const u = (n) => { const o = []; do { let b = n & 0x7f; n >>>= 7; if (n) b |= 0x80; o.push(b); } while (n); return o; };
const s = (n) => { const o = []; while (true) { let b = n & 0x7f; n >>= 7; if ((n === 0 && !(b & 0x40)) || (n === -1 && (b & 0x40))) { o.push(b); return o; } o.push(b | 0x80); } };
const vec = (items) => [...u(items.length), ...items.flat()];
const sec = (id, body) => [id, ...u(body.length), ...body];
const str = (t) => [...u(t.length), ...Buffer.from(t)];
const mod = (...secs) => new Uint8Array([0,0x61,0x73,0x6d,1,0,0,0, ...secs.flat()]);
const body = (code) => { const b = [0x00, ...code, 0x0b]; return [...u(b.length), ...b]; };
const results = {};
// 1. tail call: f(n, acc) = n==0 ? acc : return_call f(n-1, acc+1)
const tail = mod(
  sec(1, vec([[0x60, 2, 0x7f, 0x7f, 1, 0x7f]])),
  sec(3, vec([[0]])),
  sec(7, vec([[...str("f"), 0, 0]])),
  sec(10, vec([body([0x20,0, 0x45, 0x04,0x7f, 0x20,1, 0x05, 0x20,0, 0x41,1, 0x6b, 0x20,1, 0x41,1, 0x6a, 0x12,0, 0x0b])])));
try { const i = new WebAssembly.Instance(new WebAssembly.Module(tail)); results.tail_call = i.exports.f(50000000, 0); } catch (e) { results.tail_call = 'ERR ' + e.message; }
// 2. GC struct
const gc = mod(
  sec(1, vec([[0x5f, 1, 0x7f, 1], [0x60, 0, 1, 0x7f]])),
  sec(3, vec([[1]])),
  sec(7, vec([[...str("g"), 0, 0]])),
  sec(10, vec([body([0x41, ...s(42), 0xfb,0x00,0x00, 0xfb,0x02,0x00,0x00])])));
try { results.gc_struct = new WebAssembly.Instance(new WebAssembly.Module(gc)).exports.g(); } catch (e) { results.gc_struct = 'ERR ' + e.message; }
// 3. exnref try_table + throw
const eh = mod(
  sec(1, vec([[0x60, 1, 0x7f, 0], [0x60, 0, 1, 0x7f]])),
  sec(3, vec([[1]])),
  sec(13, vec([[0x00, 0]])),
  sec(7, vec([[...str("h"), 0, 0]])),
  sec(10, vec([body([0x02,0x7f, 0x1f,0x40, 1, 0x00,0,0, 0x41,7, 0x08,0, 0x0b, 0x00, 0x0b])])));
try { results.exnref_try_table = new WebAssembly.Instance(new WebAssembly.Module(eh)).exports.h(); } catch (e) { results.exnref_try_table = 'ERR ' + e.message; }
// 4. WasmFX: cont type declaration
const fx = mod(sec(1, vec([[0x60, 0, 0], [0x5d, 0]])));
results.wasmfx_cont_type_validates = WebAssembly.validate(fx);
try { new WebAssembly.Module(fx); } catch (e) { results.wasmfx_error = e.message; }
results.jspi = { Suspending: typeof WebAssembly.Suspending, promising: typeof WebAssembly.promising };
console.log(JSON.stringify(results, null, 1));
