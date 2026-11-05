// RustKeleut の WasmGC の後段の、Node 側の実行時(計画 261024-1 の 6.10 節)。
//
// `keleut build` と `keleut run --backend wasm` は、このファイルの後ろに `await start(META)` を足して
// `main.mjs` にし、`program.wasm` と `rt.wasm` の横に置く。
// META は、診断の文面の型紙と、値の印字に要る名前の表である(crate keleut-wasm の meta.rs)。
//
// 受け持つことは次のとおりである。
//
// - 起動: `rt.wasm` を先に作り、その線形メモリと関数を `program.wasm` に渡す。
//   環境変数 KELEUT_WASM_DIRECT が 1 なら、この Node のスタック(`keleut run` が RLIMIT_STACK と
//   `--stack-size` を上げて起動する)で走らせる。
//   そうでなければ、`Worker` の `resourceLimits.stackSizeMb` でスタックを取って走らせる(`node main.mjs` だけで走る形)。
//   Worker のスタックは META.bProg(プログラムの 1 段に見込むバイト数)で決め、STACK_CAP_KIB で切る
//   (k = 0 の大きさより小さくは切らない)。
//   環境変数 KELEUT_WASM_STATS があれば、選んだ経路と Worker のスタックの大きさを標準エラーに 1 行書く。
//   環境変数 KELEUT_WASM_RMDIR がこのファイルの場所を指すとき(`keleut run` の一時の場所)は、
//   2 つの `.wasm` を読んだ後でその場所を消す。
//   起動と読み込みの失敗、捕まえなかった例外は、内部エラーの 70 で終わる。
//   環境変数 KELEUT_IGNORED_SIGNALS に名前を挙げたシグナル(`keleut` が無視する扱いで起動されたもの)には、
//   何もしない処理を登録する(Node は引き継いだ SIG_IGN を既定の扱いに戻すため)。
//   V8 のヒープそのものが尽きたときは Node が SIGABRT で終わり、`keleut run` は 70 で終わる。
//   このとき cancel 節は走らず、バッファに残っていた標準出力は失われる(§15.5 からの既知の逸脱、P252)。
// - 標準出力: crate keleut-rt の out.rs と同じ規約(64 KiB のバッファ、満ちたときと終了の前の flush、
//   短い書き込みの繰り返し、EAGAIN を待って再試行、EBADF・EPIPE・ECONNRESET ではバッファを捨て、
//   ほかの誤りでは書けなかったバイトを残す)。
// - 診断: 実行時エラーの文面(値の印字を含む)を作り、終了状態を決める。
//   文面はインタプリタ(crate keleut-interp)と同じバイト列にする。
// - 形の表: rtcore の形の表を線形メモリに置き、満ちたら広げる。

import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { Worker, isMainThread, workerData } from 'node:worker_threads';

const NODE_MIN = [26, 9, 0];
// k = 0 のプログラムの 1 段に見込むバイト数と、スタックの大きさの上限(KiB)。
// crate keleut-wasm の B_WASM と STACK_CAP_KIB と同じ値である。
const B_WASM = 1536;
const STACK_CAP_KIB = 16777216;
const BUFFER_BYTES = 65536;

// 型紙の種類(meta.rs の kind)。
const K_RT = 0, K_DEPTH = 1, K_OOM = 2, K_IO = 3, K_UNHANDLED = 4, K_UNIMPL = 5, K_INTERNAL = 6;
// 型紙の後ろに足すもの(meta.rs の arg)。
const A_NONE = 0, A_SHOW = 1, A_F64 = 2, A_STR = 3, A_ARGS = 4;
// 状態語の種類(lib.rs の pk)。
const PK_ERROR = 4;
// 値の種類(rt.rs の vk)。
const VK = {
  I31: 1, BOX_I32: 2, I64: 3, F64: 4, STR: 5, UNIT: 6, BOOL: 7, DATA: 8, VARIANT: 9, RECORD: 10,
  CLOSURE: 11, METHOD: 12, EXTERNU: 13, REF: 14, ARRAY: 15, MARRAY: 16, DICT: 17,
};

const enc = new TextEncoder();

// OS の誤りの文(glibc の strerror と同じ綴り。Rust の io::Error の印字に合わせる)。
// x86_64 の Linux の glibc の表を、名前(Node の err.code)ごとに写したもの。
const STRERROR = {
  EPERM: 'Operation not permitted',
  ENOENT: 'No such file or directory',
  ESRCH: 'No such process',
  EINTR: 'Interrupted system call',
  EIO: 'Input/output error',
  ENXIO: 'No such device or address',
  E2BIG: 'Argument list too long',
  ENOEXEC: 'Exec format error',
  EBADF: 'Bad file descriptor',
  ECHILD: 'No child processes',
  EAGAIN: 'Resource temporarily unavailable',
  EWOULDBLOCK: 'Resource temporarily unavailable',
  ENOMEM: 'Cannot allocate memory',
  EACCES: 'Permission denied',
  EFAULT: 'Bad address',
  ENOTBLK: 'Block device required',
  EBUSY: 'Device or resource busy',
  EEXIST: 'File exists',
  EXDEV: 'Invalid cross-device link',
  ENODEV: 'No such device',
  ENOTDIR: 'Not a directory',
  EISDIR: 'Is a directory',
  EINVAL: 'Invalid argument',
  ENFILE: 'Too many open files in system',
  EMFILE: 'Too many open files',
  ENOTTY: 'Inappropriate ioctl for device',
  ETXTBSY: 'Text file busy',
  EFBIG: 'File too large',
  ENOSPC: 'No space left on device',
  ESPIPE: 'Illegal seek',
  EROFS: 'Read-only file system',
  EMLINK: 'Too many links',
  EPIPE: 'Broken pipe',
  EDOM: 'Numerical argument out of domain',
  ERANGE: 'Numerical result out of range',
  EDEADLK: 'Resource deadlock avoided',
  EDEADLOCK: 'Resource deadlock avoided',
  ENAMETOOLONG: 'File name too long',
  ENOLCK: 'No locks available',
  ENOSYS: 'Function not implemented',
  ENOTEMPTY: 'Directory not empty',
  ELOOP: 'Too many levels of symbolic links',
  ENOMSG: 'No message of desired type',
  EIDRM: 'Identifier removed',
  ECHRNG: 'Channel number out of range',
  EL2NSYNC: 'Level 2 not synchronized',
  EL3HLT: 'Level 3 halted',
  EL3RST: 'Level 3 reset',
  ELNRNG: 'Link number out of range',
  EUNATCH: 'Protocol driver not attached',
  ENOCSI: 'No CSI structure available',
  EL2HLT: 'Level 2 halted',
  EBADE: 'Invalid exchange',
  EBADR: 'Invalid request descriptor',
  EXFULL: 'Exchange full',
  ENOANO: 'No anode',
  EBADRQC: 'Invalid request code',
  EBADSLT: 'Invalid slot',
  EBFONT: 'Bad font file format',
  ENOSTR: 'Device not a stream',
  ENODATA: 'No data available',
  ETIME: 'Timer expired',
  ENOSR: 'Out of streams resources',
  ENONET: 'Machine is not on the network',
  ENOPKG: 'Package not installed',
  EREMOTE: 'Object is remote',
  ENOLINK: 'Link has been severed',
  EADV: 'Advertise error',
  ESRMNT: 'Srmount error',
  ECOMM: 'Communication error on send',
  EPROTO: 'Protocol error',
  EMULTIHOP: 'Multihop attempted',
  EDOTDOT: 'RFS specific error',
  EBADMSG: 'Bad message',
  EOVERFLOW: 'Value too large for defined data type',
  ENOTUNIQ: 'Name not unique on network',
  EBADFD: 'File descriptor in bad state',
  EREMCHG: 'Remote address changed',
  ELIBACC: 'Can not access a needed shared library',
  ELIBBAD: 'Accessing a corrupted shared library',
  ELIBSCN: '.lib section in a.out corrupted',
  ELIBMAX: 'Attempting to link in too many shared libraries',
  ELIBEXEC: 'Cannot exec a shared library directly',
  EILSEQ: 'Invalid or incomplete multibyte or wide character',
  ERESTART: 'Interrupted system call should be restarted',
  ESTRPIPE: 'Streams pipe error',
  EUSERS: 'Too many users',
  ENOTSOCK: 'Socket operation on non-socket',
  EDESTADDRREQ: 'Destination address required',
  EMSGSIZE: 'Message too long',
  EPROTOTYPE: 'Protocol wrong type for socket',
  ENOPROTOOPT: 'Protocol not available',
  EPROTONOSUPPORT: 'Protocol not supported',
  ESOCKTNOSUPPORT: 'Socket type not supported',
  ENOTSUP: 'Operation not supported',
  EOPNOTSUPP: 'Operation not supported',
  EPFNOSUPPORT: 'Protocol family not supported',
  EAFNOSUPPORT: 'Address family not supported by protocol',
  EADDRINUSE: 'Address already in use',
  EADDRNOTAVAIL: 'Cannot assign requested address',
  ENETDOWN: 'Network is down',
  ENETUNREACH: 'Network is unreachable',
  ENETRESET: 'Network dropped connection on reset',
  ECONNABORTED: 'Software caused connection abort',
  ECONNRESET: 'Connection reset by peer',
  ENOBUFS: 'No buffer space available',
  EISCONN: 'Transport endpoint is already connected',
  ENOTCONN: 'Transport endpoint is not connected',
  ESHUTDOWN: 'Cannot send after transport endpoint shutdown',
  ETOOMANYREFS: 'Too many references: cannot splice',
  ETIMEDOUT: 'Connection timed out',
  ECONNREFUSED: 'Connection refused',
  EHOSTDOWN: 'Host is down',
  EHOSTUNREACH: 'No route to host',
  EALREADY: 'Operation already in progress',
  EINPROGRESS: 'Operation now in progress',
  ESTALE: 'Stale file handle',
  EUCLEAN: 'Structure needs cleaning',
  ENOTNAM: 'Not a XENIX named type file',
  ENAVAIL: 'No XENIX semaphores available',
  EISNAM: 'Is a named type file',
  EREMOTEIO: 'Remote I/O error',
  EDQUOT: 'Disk quota exceeded',
  ENOMEDIUM: 'No medium found',
  EMEDIUMTYPE: 'Wrong medium type',
  ECANCELED: 'Operation canceled',
  ENOKEY: 'Required key not available',
  EKEYEXPIRED: 'Key has expired',
  EKEYREVOKED: 'Key has been revoked',
  EKEYREJECTED: 'Key was rejected by service',
  EOWNERDEAD: 'Owner died',
  ENOTRECOVERABLE: 'State not recoverable',
  ERFKILL: 'Operation not possible due to RF-kill',
  EHWPOISON: 'Memory page has hardware error',
};

// 表に無い誤りは、glibc が知らない番号に返す文と同じにする(libuv の errno は負の数)。
function strerror(e) {
  const s = STRERROR[e.code];
  if (s !== undefined) return s;
  if (typeof e.errno === 'number') return `Unknown error ${Math.abs(e.errno)}`;
  return String(e.message ?? e);
}

function bytesOf(parts) {
  const chunks = parts.map((p) => (typeof p === 'string' ? enc.encode(p) : p));
  let n = 0;
  for (const c of chunks) n += c.length;
  const out = new Uint8Array(n);
  let at = 0;
  for (const c of chunks) {
    out.set(c, at);
    at += c.length;
  }
  return out;
}

const sleeper = new Int32Array(new SharedArrayBuffer(4));

function sleepMs(ms) {
  Atomics.wait(sleeper, 0, 0, ms);
}

// 記述子 fd に data を書く(標準エラー用。誤りは無視する)。
function writeQuiet(fd, data) {
  let written = 0;
  while (written < data.length) {
    try {
      const n = fs.writeSync(fd, data, written, data.length - written);
      if (n <= 0) return;
      written += n;
    } catch (e) {
      if (e.code === 'EINTR') continue;
      if (e.code === 'EAGAIN') {
        sleepMs(1);
        continue;
      }
      return;
    }
  }
}

// 標準出力(out.rs の Out と同じ規約)。
class Out {
  constructor(closed) {
    this.closed = closed;
    this.buf = new Uint8Array(2 * BUFFER_BYTES);
    this.len = 0;
  }

  push(data) {
    if (this.len + data.length > this.buf.length) {
      const nb = new Uint8Array(Math.max(this.buf.length * 2, this.len + data.length));
      nb.set(this.buf.subarray(0, this.len));
      this.buf = nb;
    }
    this.buf.set(data, this.len);
    this.len += data.length;
  }

  // data を書けるだけ書く。失敗したら { error, written, discard } を返す。
  writeAll(data) {
    let written = 0;
    let waits = 0;
    while (written < data.length) {
      let n;
      try {
        if (this.closed) {
          const e = new Error('closed');
          e.code = 'EBADF';
          throw e;
        }
        n = fs.writeSync(1, data, written, data.length - written);
      } catch (e) {
        if (e.code === 'EINTR') continue;
        if (e.code === 'EAGAIN') {
          waits += 1;
          sleepMs(Math.min(waits, 50));
          continue;
        }
        const discard = e.code === 'EBADF' || e.code === 'EPIPE' || e.code === 'ECONNRESET';
        return { error: strerror(e), written, discard };
      }
      if (n === 0) return { error: 'write returned 0', written, discard: false };
      written += n;
      waits = 0;
    }
    return null;
  }

  // バッファを書き出す。失敗したら文を返す。
  flush() {
    if (this.len === 0) return null;
    const data = this.buf.slice(0, this.len);
    const f = this.writeAll(data);
    if (f === null) {
      this.len = 0;
      return null;
    }
    if (f.discard) {
      this.len = 0;
    } else {
      const rest = data.subarray(f.written);
      this.len = 0;
      this.push(rest);
    }
    return f.error;
  }

  // Console.write の文字列を足す。失敗したら文を返す。
  write(data) {
    if (this.len + data.length < BUFFER_BYTES) {
      this.push(data);
      return null;
    }
    const room = BUFFER_BYTES - this.len;
    this.push(data.subarray(0, room));
    const e = this.flush();
    if (e !== null) return e;
    const rest = data.subarray(room);
    const big = rest.length - (rest.length % BUFFER_BYTES);
    if (big > 0) {
      const f = this.writeAll(rest.subarray(0, big));
      if (f !== null) {
        if (!f.discard) {
          const left = rest.subarray(f.written, big);
          this.push(left.subarray(0, Math.min(left.length, BUFFER_BYTES)));
        }
        return f.error;
      }
    }
    this.push(rest.subarray(big));
    if (this.len >= BUFFER_BYTES) {
      const e2 = this.flush();
      if (e2 !== null) return e2;
    }
    return null;
  }

  // 標準出力を flush してから(失敗は無視する)、msg を標準エラーに書く。
  eprint(msg) {
    this.flush();
    writeQuiet(2, msg);
  }
}

function versionOk() {
  const v = process.versions.node.split('.').map((x) => parseInt(x, 10));
  for (let i = 0; i < 3; i++) {
    if ((v[i] ?? 0) > NODE_MIN[i]) return true;
    if ((v[i] ?? 0) < NODE_MIN[i]) return false;
  }
  return true;
}

// OCaml の String.escaped と同じ規則でバイト列を書く(crate keleut-rt の escape_into)。
function escapeBytes(b) {
  let s = '';
  for (const c of b) {
    if (c === 0x22) s += '\\"';
    else if (c === 0x5c) s += '\\\\';
    else if (c === 0x0a) s += '\\n';
    else if (c === 0x09) s += '\\t';
    else if (c === 0x0d) s += '\\r';
    else if (c === 0x08) s += '\\b';
    else if (c >= 0x20 && c <= 0x7e) s += String.fromCharCode(c);
    else s += '\\' + String(c).padStart(3, '0');
  }
  return s;
}

// keleut run が作った一時の場所なら消す(2 つの .wasm を読んだ後。keleut が殺されても残らない)。
function removeTempDir(dir) {
  const want = process.env.KELEUT_WASM_RMDIR;
  if (want === undefined || want === '') return;
  const here = fileURLToPath(dir).replace(/\/+$/, '');
  if (here !== want.replace(/\/+$/, '')) return;
  try {
    fs.rmSync(here, { recursive: true, force: true });
  } catch (e) {
    // 消せなくても keleut が後で消す。
  }
}

function runProgram(dir, META, exit) {
  const out = new Out(process.env.KELEUT_STDOUT_CLOSED === '1');
  const L = BigInt(META.maxDepth);
  let rtMod, progMod, rt;
  try {
    const rtBytes = fs.readFileSync(new URL('./rt.wasm', dir));
    const progBytes = fs.readFileSync(new URL('./program.wasm', dir));
    removeTempDir(dir);
    rtMod = new WebAssembly.Module(rtBytes);
    progMod = new WebAssembly.Module(progBytes);
    rt = new WebAssembly.Instance(rtMod, {}).exports;
  } catch (e) {
    out.eprint(enc.encode(`keleut: 内部エラー: モジュールを読み込めません: ${e}\n`));
    exit(70);
    return;
  }
  const mem = rt.memory;
  let top = (rt.__heap_base.value + 15) & ~15;

  // 線形メモリの上に n バイトの領域を取る(解放はしない)。
  function alloc(n) {
    const p = top;
    const end = p + ((n + 15) & ~15);
    const need = end - mem.buffer.byteLength;
    if (need > 0) {
      try {
        mem.grow(Math.ceil(need / 65536));
      } catch (e) {
        out.eprint(enc.encode('keleut: 内部エラー: 線形メモリを広げられません\n'));
        exit(70);
      }
    }
    top = end;
    return p;
  }

  const scratch = rt.krt_scratch();
  // 形の表。
  let capShapes = 1024;
  let capLabels = 16384;
  function newTable(cs, cl) {
    const bytes = rt.krt_shapes_bytes(cs, cl);
    const p = alloc(bytes + 8);
    const at = (p + 7) & ~7;
    const t = rt.krt_shapes_init(at, cs, cl);
    if (t === 0) {
      out.eprint(enc.encode('keleut: 内部エラー: 形の表を作れません\n'));
      exit(70);
    }
    return t;
  }
  {
    let total = 0;
    for (const s of META.shapes) total += s.length;
    while (total + 1 > capLabels || META.shapes.length + 1 > capShapes) {
      capLabels *= 2;
      capShapes *= 2;
    }
  }
  const table = newTable(capShapes, capLabels);
  for (let i = 1; i < META.shapes.length; i++) {
    const s = META.shapes[i];
    const p = alloc(4 * s.length + 4);
    new Uint32Array(mem.buffer, p, s.length).set(s);
    const id = rt.krt_shape_intern(table, p, s.length, 0);
    if (id !== i) {
      out.eprint(enc.encode(`keleut: 内部エラー: 形の表の番号が合いません(LIR の ${i} 番が実行時の ${id} 番)\n`));
      exit(70);
    }
  }
  const shapes = new WebAssembly.Global({ value: 'i32', mutable: true }, table);

  let stage = 0;
  let stageCap = 0;
  function buf(n) {
    if (n > stageCap) {
      stageCap = Math.max(n, 2 * stageCap, 65536);
      stage = alloc(stageCap);
    }
    return stage;
  }

  // 診断の文面(番号から)。
  const msgs = new Map();
  let nextMsg = 1;
  let X = null;

  function strBytes(v) {
    const n = X.v_n(v);
    const p = X.s_out(v);
    return new Uint8Array(mem.buffer, p, n).slice();
  }

  function f64show(bits) {
    const p = scratch + 1024;
    const n = rt.krt_f64_show_bits(bits, p);
    return new Uint8Array(mem.buffer, p, n).slice();
  }

  function shapeLabels(shape) {
    const n = rt.krt_shape_len(shapes.value, shape);
    const p = rt.krt_shape_labels(shapes.value, shape);
    return Array.from(new Uint32Array(mem.buffer, p, n));
  }

  // 値の印字(crate keleut-rt の Rt::show と同じ)。深い値でも JS のスタックを使わないよう、作業の列でたどる。
  // レコードのフィールドは Rt::show と同じく正準の配置の順に書く。付録 B はラベルの違うフィールドどうしの順を読み手に任せるが、
  // 後段の群の中では出力が一致しなければならない(D499)ので、この順を Rt::show と変えてはならない。
  function show(v0) {
    const parts = [];
    const work = [{ v: v0 }];
    const seq = (open, xs, close) => {
      work.push({ t: close });
      for (let i = xs.length - 1; i >= 0; i--) {
        work.push({ v: xs[i] });
        if (i > 0) work.push({ t: ', ' });
      }
      work.push({ t: open });
    };
    while (work.length > 0) {
      const it = work.pop();
      if (it.t !== undefined) {
        parts.push(it.t);
        continue;
      }
      const w = it.v;
      if (typeof w === 'number') {
        parts.push(String(w));
        continue;
      }
      const k = X.v_kind(w);
      const elems = () => {
        const n = X.v_n(w);
        const xs = [];
        for (let i = 0; i < n; i++) xs.push(X.v_get(w, i));
        return xs;
      };
      switch (k) {
        case VK.BOX_I32:
          parts.push(String(X.v_a(w)));
          break;
        case VK.I64:
          parts.push(X.v_i64(w).toString());
          break;
        case VK.F64:
          parts.push(f64show(X.v_i64(w)));
          break;
        case VK.STR:
          parts.push('"' + escapeBytes(strBytes(w)) + '"');
          break;
        case VK.UNIT:
          parts.push('()');
          break;
        case VK.BOOL:
          parts.push(X.v_a(w) ? 'true' : 'false');
          break;
        case VK.DATA: {
          const name = META.ctors[X.v_a(w)][X.v_b(w)];
          const xs = elems();
          if (xs.length === 0) parts.push(name);
          else seq(name + '(', xs, ')');
          break;
        }
        case VK.VARIANT: {
          const l = META.labels[X.v_a(w)];
          const p = X.v_get(w, 0);
          if (typeof p !== 'number' && X.v_kind(p) === VK.UNIT) parts.push('#' + l);
          else seq('#' + l + '(', [p], ')');
          break;
        }
        case VK.RECORD: {
          const labels = shapeLabels(X.v_a(w));
          const xs = elems();
          const tuple = labels.every((l) => META.items[l] === 1);
          if (tuple) {
            seq('(', xs, xs.length === 1 ? ',)' : ')');
          } else {
            work.push({ t: '}' });
            for (let i = xs.length - 1; i >= 0; i--) {
              work.push({ v: xs[i] });
              work.push({ t: (i > 0 ? ', ' : '') + META.labels[labels[i]] + ' = ' });
            }
            work.push({ t: '{' });
          }
          break;
        }
        case VK.CLOSURE: {
          const p = META.prim[X.v_a(w)];
          parts.push(p === null || p === undefined ? '<fn>' : `<prim ${p}>`);
          break;
        }
        case VK.METHOD:
          parts.push(META.methods[X.v_a(w)][X.v_b(w)]);
          break;
        case VK.EXTERNU:
          parts.push(`<prim ${META.globals[X.v_a(w)]}>`);
          break;
        case VK.REF:
          parts.push('<ref>');
          break;
        case VK.ARRAY:
          seq('[', elems(), ']');
          break;
        case VK.MARRAY:
          seq('<mutable [', elems(), ']>');
          break;
        case VK.DICT:
          parts.push('<dict>');
          break;
        default:
          parts.push('<?>');
      }
    }
    return bytesOf(parts);
  }

  function argsText(arr) {
    const n = X.v_n(arr);
    const parts = ['('];
    for (let i = 0; i < n; i++) {
      if (i > 0) parts.push(', ');
      parts.push(show(X.v_get(arr, i)));
    }
    if (n === 1) parts.push(',');
    parts.push(')');
    return bytesOf(parts);
  }

  function message(m) {
    switch (m.kind) {
      case K_RT:
        return bytesOf(['実行時エラー: ', m.text, '\n']);
      case K_DEPTH:
        return bytesOf([`実行時エラー: 論理的な深さが限度 ${L} を超えました\n`]);
      case K_OOM:
        return bytesOf(['実行時エラー: メモリ不足です\n']);
      case K_IO:
        return bytesOf(['keleut: 標準出力に書き出せません: ', m.text, '\n']);
      case K_UNHANDLED:
        return bytesOf(['未処理のエフェクト操作: ', m.text, '\n']);
      default:
        return bytesOf(['keleut: 内部エラー: 知らない種類の誤り\n']);
    }
  }

  function exitCode(m) {
    return m.kind === K_IO ? 74 : 3;
  }

  function suppressedText(m) {
    switch (m.kind) {
      case K_RT:
        return m.text;
      case K_DEPTH:
        return bytesOf([`論理的な深さが限度 ${L} を超えました`]);
      case K_OOM:
        return bytesOf(['メモリ不足です']);
      case K_IO:
        return bytesOf(['標準出力に書き出せません: ', m.text]);
      case K_UNHANDLED:
        return bytesOf(['未処理のエフェクト操作: ', m.text]);
      default:
        return bytesOf(['']);
    }
  }

  function fatal(kind, text) {
    if (kind === K_UNIMPL) {
      out.eprint(bytesOf(['keleut: 未実装: ', text, '\n']));
      exit(4);
    }
    out.eprint(bytesOf(['keleut: 内部エラー: ', text, '\n']));
    exit(70);
  }

  const host = {
    write(ptr, len) {
      const data = new Uint8Array(mem.buffer, ptr, len).slice();
      const e = out.write(data);
      if (e === null) return 0;
      const id = nextMsg++;
      msgs.set(id, { kind: K_IO, text: enc.encode(e) });
      return id;
    },
    buf,
    err(t, v, x) {
      const [kind, a, text] = META.templates[t];
      let tail = new Uint8Array(0);
      if (a === A_SHOW) tail = show(v);
      else if (a === A_F64) tail = f64show(x);
      else if (a === A_STR) tail = strBytes(v);
      else if (a === A_ARGS) tail = argsText(v);
      const id = nextMsg++;
      msgs.set(id, { kind, text: bytesOf([text, tail]) });
      return id;
    },
    fatal(t) {
      const [kind, , text] = META.templates[t];
      fatal(kind, text);
    },
    suppress(kind, msg) {
      let text;
      if (kind === PK_ERROR && msgs.has(msg)) {
        text = suppressedText(msgs.get(msg));
        msgs.delete(msg);
      } else {
        text = enc.encode('cancel 節から操作の巻き戻しで脱出しようとしました');
      }
      const e = out.flush();
      if (e !== null) {
        out.eprint(bytesOf(['keleut: 標準出力に書き出せません: ', e, '\n']));
        exit(74);
      }
      writeQuiet(2, bytesOf(['cancel 節で例外が抑制されました: ', text, '\n']));
    },
    shapes_grow() {
      capShapes *= 2;
      capLabels *= 2;
      const t2 = newTable(capShapes, capLabels);
      if (rt.krt_shapes_copy(t2, shapes.value) !== 0) return 1;
      shapes.value = t2;
      return 0;
    },
    shapes,
    scratch: new WebAssembly.Global({ value: 'i32', mutable: false }, scratch),
  };

  try {
    X = new WebAssembly.Instance(progMod, { rt, host }).exports;
  } catch (e) {
    out.eprint(enc.encode(`keleut: 内部エラー: モジュールを読み込めません: ${e}\n`));
    exit(70);
    return;
  }
  let code;
  try {
    code = X.main(L);
  } catch (e) {
    out.eprint(enc.encode(`keleut: 内部エラー: ${e}\n`));
    exit(70);
    return;
  }
  if (code === 0) {
    const e = out.flush();
    if (e !== null) {
      out.eprint(bytesOf(['keleut: 標準出力に書き出せません: ', e, '\n']));
      exit(74);
    }
    exit(0);
  }
  const kind = X.p_kind.value;
  if (kind === PK_ERROR && msgs.has(X.p_msg.value)) {
    const m = msgs.get(X.p_msg.value);
    out.eprint(message(m));
    exit(exitCode(m));
  }
  out.eprint(enc.encode(`keleut: 内部エラー: 最上位に届いた状態語: ${kind}\n`));
  exit(70);
}

// 起動する。main.mjs の最後の行が呼ぶ。
export async function start(META) {
  const dir = new URL('./', import.meta.url);
  const exit = (c) => process.exit(c);
  // ホストの中で捕まえなかった例外は、Node の終了状態 1(型の誤りの番号)にせず、内部エラーの 70 にする。
  const fail = (e) => {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: ${e}\n`));
    exit(70);
  };
  process.on('uncaughtException', fail);
  process.on('unhandledRejection', fail);
  if (!isMainThread) {
    runProgram(dir, workerData.META, exit);
    return;
  }
  // keleut が無視する扱いで起動されたシグナルは、ここでも無視する。
  for (const sig of (process.env.KELEUT_IGNORED_SIGNALS ?? '').split(',')) {
    if (sig === 'SIGINT' || sig === 'SIGTERM' || sig === 'SIGHUP') process.on(sig, () => {});
  }
  if (!versionOk()) {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: Node ${NODE_MIN.join('.')} 以上が要ります(この Node は ${process.versions.node})\n`));
    exit(70);
  }
  // 環境変数 KELEUT_WASM_STATS があれば、選んだ経路と Worker のスタックの大きさを標準エラーに 1 行書く(測定と試験用)。
  const stats = process.env.KELEUT_WASM_STATS !== undefined;
  if (process.env.KELEUT_WASM_DIRECT === '1') {
    if (stats) writeQuiet(2, enc.encode('keleut-host: path=direct\n'));
    runProgram(dir, META, exit);
    return;
  }
  // RLIMIT_STACK と --stack-size を上げずに起動されたので、Worker のスタックで走らせる(D545)。
  // 大きさは (L + R) × B_prog + 32 MiB で、keleut run の --stack-size と同じ式である(crate keleut-wasm の stack_size_kib)。
  // B_prog はプログラムの 1 段に見込むバイト数で、META.bProg にある(crate keleut-wasm の b_prog)。
  // STACK_CAP_KIB を超えたら切るが、B_WASM で求めた k = 0 の大きさより小さくはしない。
  const L = META.maxDepth;
  const R = Math.max(1024, Math.floor(L / 64));
  const need = (b) => Math.ceil(((L + R) * b + 32 * 1024 * 1024) / 1024);
  let stackKiB = need(META.bProg);
  if (stackKiB > STACK_CAP_KIB) stackKiB = Math.max(STACK_CAP_KIB, need(B_WASM));
  const stackSizeMb = Math.ceil(stackKiB / 1024) + 64;
  if (stats) writeQuiet(2, enc.encode(`keleut-host: path=worker stackSizeMb=${stackSizeMb}\n`));
  let w;
  try {
    w = new Worker(new URL(import.meta.url), {
      workerData: { META },
      resourceLimits: { stackSizeMb },
      stdout: false,
      stderr: false,
    });
  } catch (e) {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: 実行のスレッドを作れません(${stackSizeMb} MiB のスタック): ${e.message ?? e}\n`));
    exit(70);
    return;
  }
  w.on('error', (e) => {
    writeQuiet(2, enc.encode(`keleut: 内部エラー: ${e}\n`));
    exit(70);
  });
  w.on('exit', (c) => exit(c));
}
