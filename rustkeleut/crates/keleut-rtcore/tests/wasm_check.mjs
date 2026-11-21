// rtcore の wasm32 版(rt.wasm)を Node で検査する。
//
// 使い方: rustkeleut/ で cargo test -p keleut-rtcore を実行した後に、次のコマンドを実行する。
//   node crates/keleut-rtcore/tests/wasm_check.mjs
// cargo のテストが <target>/rtcore/ に書いた入力と期待値を読む。
// <target> は、CARGO_TARGET_DIR があればその値、なければこのスクリプトから見た ../../../target である。
// 相対パスの CARGO_TARGET_DIR は、作業ディレクトリからの相対パスとして解決する。
// KRT_STAGE_DIR でディレクトリを直接指定してもよい。
// 読んだディレクトリと、テストが書いたビルドの情報(rt.wasm の FNV-1a、profile、ターゲットトリプル)を毎回表示する。
// ディレクトリにある rt.wasm の FNV-1a がビルドの情報と違えば、別のビルドのテストが書いた入力を読んでいるので、FAIL の行を表示して検査の失敗に数える。
//
// 検査する内容:
// 1. rt.wasm の import が 0 個で、必要な export がそろっている。
// 2. show(Float64): fltcmp の 1,998,964 件と境界値で、glibc で作った期待値(NaN は nan)との差分が 0 件である。
//    ビット列を i64 で渡す関数 krt_f64_show_bits と、f64 で渡す関数 krt_f64_show の両方を呼ぶ。
//    krt_f64_show_bits には、NaN の符号とペイロードがそのまま渡る。
// 3. libm: 1,000,000 件の入力で、x86_64 版の結果とビット列が一致する。
//    NaN どうしは一致とみなす。
// 4. バイト列の比較とシェイプ表: 乱数で作った入力で、バイト列の比較は Buffer.compare と、シェイプ表は JS で書いたモデルと一致する。
// 失敗した検査が 1 つでもあれば、残りの検査も行った後に終了コード 1 で終了する。
// 必要なファイル(rt.wasm、入力、期待値、ビルドの情報)がそろっていなければ、検査をせずに終了コード 2 で終了する。
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = fileURLToPath(new URL(".", import.meta.url));
const stage = process.env.KRT_STAGE_DIR
  ?? (process.env.CARGO_TARGET_DIR ? join(resolve(process.env.CARGO_TARGET_DIR), "rtcore") : join(here, "../../../target/rtcore"));
const need = ["rt.wasm", "fltcmp-inputs.bin", "fltcmp-expected.txt", "fltcmp-meta.json", "libm-inputs.bin", "libm-x86_64.bin", "libm-meta.json"];
console.log(`stage: ${stage}`);
for (const f of need) {
  if (!existsSync(join(stage, f))) {
    console.error(`${join(stage, f)} がない。先に rustkeleut/ で cargo test -p keleut-rtcore を実行する。`);
    process.exit(2);
  }
}
const read = (f) => readFileSync(join(stage, f));
const ab = (b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);

let failures = 0;
const fail = (msg) => { console.log(`FAIL: ${msg}`); failures++; };

// ---- 0. ビルドの情報 ----
const fnv1a64 = (b) => {
  let h = 0xcbf29ce484222325n;
  for (const x of b) h = ((h ^ BigInt(x)) * 0x100000001b3n) & 0xffffffffffffffffn;
  return h.toString(16).padStart(16, "0");
};
{
  const hash = fnv1a64(read("rt.wasm"));
  for (const f of ["fltcmp-meta.json", "libm-meta.json"]) {
    const meta = JSON.parse(read(f).toString());
    console.log(`${f}: ${JSON.stringify(meta)}`);
    if (meta.rt_wasm_fnv1a64 !== hash) {
      fail(`${f} は別の rt.wasm(FNV-1a ${meta.rt_wasm_fnv1a64})のビルド方法で書かれた。置き場の rt.wasm は ${hash} である。cargo test -p keleut-rtcore を同じビルド方法で実行し直す`);
    }
  }
}

// ---- 1. モジュール ----
const mod = new WebAssembly.Module(read("rt.wasm"));
const imports = WebAssembly.Module.imports(mod);
console.log(`rt.wasm: ${read("rt.wasm").length} bytes, imports: ${imports.length}`);
if (imports.length !== 0) fail(`rt.wasm imports ${JSON.stringify(imports)}`);
const { exports: ex } = new WebAssembly.Instance(mod, {});
for (const name of ["memory", "krt_sin", "krt_cos", "krt_exp", "krt_log", "krt_f64_show", "krt_f64_show_bits",
  "krt_scratch", "krt_bytes_compare", "krt_shapes_bytes", "krt_shapes_init", "krt_shapes_copy", "krt_shapes_count",
  "krt_shape_intern", "krt_shape_len", "krt_shape_labels", "krt_shape_find", "krt_shape_extend", "krt_shape_restrict",
  "__heap_base"]) {
  if (!(name in ex)) fail(`missing export ${name}`);
}
const mem = () => new Uint8Array(ex.memory.buffer);
const scratch = ex.krt_scratch();

// ---- 2. show(Float64) ----
{
  const meta = JSON.parse(read("fltcmp-meta.json").toString());
  const bits = new BigUint64Array(ab(read("fltcmp-inputs.bin")));
  const expected = read("fltcmp-expected.txt");
  if (bits.length !== meta.fltcmp + meta.boundary) fail("fltcmp-inputs.bin の件数が meta と違う");
  if (meta.fltcmp !== 1998964) fail(`fltcmp の件数が 1,998,964 でない(${meta.fltcmp})`);
  const f64 = new Float64Array(bits.buffer);
  const dec = new TextDecoder();
  const groups = [["fltcmp", 0, meta.fltcmp], ["boundary", meta.fltcmp, bits.length]];
  let off = 0;
  for (const [label, lo, hi] of groups) {
    let diffBits = 0, diffF64 = 0, nans = 0, shown = 0;
    for (let i = lo; i < hi; i++) {
      const end = expected.indexOf(10, off);
      const e = expected.subarray(off, end);
      off = end + 1;
      const isNaN = (bits[i] & 0x7fffffffffffffffn) > 0x7ff0000000000000n;
      if (isNaN) nans++;
      // ビット列で渡す関数。
      let n = ex.krt_f64_show_bits(bits[i], scratch);
      let m = mem();
      let same = n === e.length && m[scratch + n] === 0;
      for (let k = 0; same && k < n; k++) same = m[scratch + k] === e[k];
      if (!same) {
        diffBits++;
        if (shown++ < 10) console.log(`  ${label} 0x${bits[i].toString(16)}: glibc ${dec.decode(e)} wasm32 ${dec.decode(m.subarray(scratch, scratch + n))}`);
      }
      // f64 で渡す関数。
      // NaN は JS の Number を経由するのでペイロードは保たれないが、どの NaN も nan になる。
      n = ex.krt_f64_show(f64[i], scratch);
      m = mem();
      same = n === e.length;
      for (let k = 0; same && k < n; k++) same = m[scratch + k] === e[k];
      if (!same) diffF64++;
    }
    console.log(`wasm32 ${label}: ${hi - lo} cases (NaN ${nans}), ${diffBits} differ from glibc via bits, ${diffF64} via f64`);
    if (diffBits + diffF64 !== 0) fail(`show ${label}`);
  }
}

// ---- 3. libm ----
{
  const inputs = new Float64Array(ab(read("libm-inputs.bin")));
  const native = new BigUint64Array(ab(read("libm-x86_64.bin")));
  const n = inputs.length;
  if (native.length !== 4 * n) fail("libm-x86_64.bin のサイズが違う");
  const tmp = new Float64Array(1);
  const tmpBits = new BigUint64Array(tmp.buffer);
  const isNaNBits = (b) => (b & 0x7fffffffffffffffn) > 0x7ff0000000000000n;
  for (const [j, name] of ["sin", "cos", "exp", "log"].entries()) {
    const f = ex[`krt_${name}`];
    let diff = 0, nanPayload = 0;
    for (let i = 0; i < n; i++) {
      tmp[0] = f(inputs[i]);
      const w = tmpBits[0], x = native[j * n + i];
      if (w === x) continue;
      if (isNaNBits(w) && isNaNBits(x)) { nanPayload++; continue; }
      if (diff < 5) console.log(`  ${name}(${inputs[i]}): x86_64 0x${x.toString(16)} wasm32 0x${w.toString(16)}`);
      diff++;
    }
    console.log(`wasm32 libm ${name}: ${n} cases, ${diff} differ from x86_64 (NaN differing only in payload or sign: ${nanPayload})`);
    if (diff !== 0) fail(`libm ${name}`);
  }
}

// 乱数(xorshift64)。
let st = 0x9E3779B97F4A7C15n;
const rnd = (n) => {
  st ^= (st << 13n) & 0xffffffffffffffffn;
  st ^= st >> 7n;
  st ^= (st << 17n) & 0xffffffffffffffffn;
  return Number(st % BigInt(n));
};

// ---- 4a. バイト列の比較 ----
{
  let diff = 0;
  const N = 100000;
  for (let i = 0; i < N; i++) {
    const la = rnd(6), lb = rnd(6);
    const a = Uint8Array.from({ length: la }, () => [0, 1, 0x7f, 0x80, 0xff][rnd(5)]);
    const b = rnd(4) === 0 ? a.slice(0, lb) : Uint8Array.from({ length: lb }, () => [0, 1, 0x7f, 0x80, 0xff][rnd(5)]);
    const m = mem();
    m.set(a, scratch);
    m.set(b, scratch + 64);
    const got = ex.krt_bytes_compare(scratch, a.length, scratch + 64, b.length);
    if (got !== Buffer.compare(Buffer.from(a), Buffer.from(b))) diff++;
  }
  console.log(`wasm32 bytes_compare: ${N} cases, ${diff} differ from Buffer.compare`);
  if (diff !== 0) fail("bytes_compare");
}

// ---- 4b. シェイプ表 ----
{
  const NONE = 0xffffffff, FULL = 0xfffffffe;
  const heap = (ex.__heap_base.value + 7) & ~7;
  let base = heap, capS = 1, capL = 0;
  const ensure = (end) => {
    const have = ex.memory.buffer.byteLength;
    if (end > have) ex.memory.grow(Math.ceil((end - have) / 65536));
  };
  ensure(base + ex.krt_shapes_bytes(capS, capL));
  let t = ex.krt_shapes_init(base, capS, capL);
  const grow = () => {
    const nS = capS * 2, nL = Math.max(capL, 16) * 2;
    const nb = (base + ex.krt_shapes_bytes(capS, capL) + 7) & ~7;
    ensure(nb + ex.krt_shapes_bytes(nS, nL));
    const nt = ex.krt_shapes_init(nb, nS, nL);
    if (ex.krt_shapes_copy(nt, t) !== 0) fail("krt_shapes_copy");
    base = nb; t = nt; capS = nS; capL = nL;
  };
  const call = (f) => { for (;;) { const r = f() >>> 0; if (r !== FULL) return r; grow(); } };
  const model = [[]];
  const ids = new Map([["", 0]]);
  const intern = (l) => { const k = l.join(","); if (!ids.has(k)) { ids.set(k, model.length); model.push(l); } return ids.get(k); };
  const labelsOf = (id) => {
    const n = ex.krt_shape_len(t, id) >>> 0, p = ex.krt_shape_labels(t, id) >>> 0;
    return Array.from(new Uint32Array(ex.memory.buffer, p, n));
  };
  const posPtr = scratch, labPtr = scratch + 64;
  let diff = 0;
  const N = 20000;
  for (let i = 0; i < N; i++) {
    const id = rnd(model.length), label = rnd(10);
    const op = rnd(4);
    if (op === 0) {
      const written = Array.from({ length: rnd(6) }, () => rnd(10));
      new Uint32Array(ex.memory.buffer, labPtr, written.length).set(written);
      const got = call(() => ex.krt_shape_intern(t, labPtr, written.length, 0));
      if (got !== intern([...written].sort((a, b) => a - b))) diff++;
    } else if (op === 1) {
      const got = call(() => ex.krt_shape_extend(t, id, label, posPtr));
      const l = [...model[id]];
      let lb = 0;
      while (lb < l.length && l[lb] < label) lb++;
      l.splice(lb, 0, label);
      if (got !== intern(l) || new Uint32Array(ex.memory.buffer, posPtr, 1)[0] !== lb) diff++;
    } else if (op === 2) {
      const got = call(() => ex.krt_shape_restrict(t, id, label, posPtr));
      const i0 = model[id].indexOf(label);
      if (i0 < 0) { if (got !== NONE) diff++; }
      else {
        const l = [...model[id]];
        l.splice(i0, 1);
        if (got !== intern(l) || new Uint32Array(ex.memory.buffer, posPtr, 1)[0] !== i0) diff++;
      }
    } else {
      const f = ex.krt_shape_find(t, id, label) >>> 0;
      const i0 = model[id].indexOf(label);
      if (f !== (i0 < 0 ? NONE : i0) || labelsOf(id).join(",") !== model[id].join(",")) diff++;
    }
    if (ex.krt_shapes_count(t) !== model.length) diff++;
  }
  console.log(`wasm32 shapes: ${N} operations, ${model.length} shapes, ${diff} differ from the model`);
  if (diff !== 0) fail("shapes");
}

console.log(failures === 0 ? "differences: 0" : `failures: ${failures}`);
process.exit(failures === 0 ? 0 : 1);
