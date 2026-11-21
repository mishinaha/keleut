//! バイトコードの符号化のテスト。
//!
//! `crates/kir/tests/fixtures/` と `crates/keleut-cli/tests/fixtures/run/` にある KIR のすべての関数を符号化し、次のことを確かめる。
//!
//! - 命令のワード数を先頭から順に足していくと、ワードの並びの終わりにちょうど達する。
//! - 再開点の表の各項目が、呼び出しの命令の先頭を指す。
//! - 再開点の表の各項目が持つ、呼び出しの後に続ける位置が、命令の先頭かワードの並びの終わりである(位置を持たない項目を除く)。

use keleut_interp::bytecode::{self, op};
use keleut_lir::FuncId;
use std::path::PathBuf;

fn kirs() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut v = Vec::new();
    for d in ["../kir/tests/fixtures", "../keleut-cli/tests/fixtures/run"] {
        for e in std::fs::read_dir(root.join(d)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().and_then(|x| x.to_str()) == Some("kir") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

#[test]
fn すべての関数の命令の列をたどれる() {
    let files = kirs();
    assert!(files.len() >= 30, "{}", files.len());
    let mut funcs = 0;
    for path in files {
        let k = kir::load(&std::fs::read(&path).unwrap()).unwrap();
        let lp = keleut_lir::lower(&k).unwrap();
        let rt = keleut_rt::Rt::new(&lp, 100).unwrap();
        for i in 0..lp.funcs.len() {
            let code = bytecode::encode(&lp, &rt, FuncId(i as u32))
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let mut starts = std::collections::HashSet::new();
            let mut pc = 0;
            while pc < code.words.len() {
                starts.insert(pc as u32);
                let n = bytecode::inst_len(&code.words, pc);
                assert!(n >= 1);
                pc += n;
            }
            assert_eq!(pc, code.words.len(), "{}: 関数 {i}", path.display());
            for r in &code.resume {
                assert!(starts.contains(&r.call_pc), "{}: 関数 {i}", path.display());
                let o = (code.words[r.call_pc as usize] & 0xff) as u8;
                assert!(
                    matches!(
                        o,
                        op::CALL_DIRECT
                            | op::CALL_VALUE
                            | op::CALL_METHOD
                            | op::DICT_APP
                            | op::TAIL_VALUE
                            | op::TAIL_METHOD
                            | op::FX
                    ),
                    "{}: 関数 {i} の再開点が呼び出しを指さない",
                    path.display()
                );
                if o == op::FX {
                    // エフェクトの命令を指す再開点は、呼び出しの一種である命令(perform と、番兵を受け取る呼び出し)を指す。
                    let sub = (code.words[r.call_pc as usize] >> 8) as usize;
                    assert!(code.fx[sub].is_call(), "{}: 関数 {i}", path.display());
                }
                if r.pc_after != u32::MAX {
                    assert!(
                        starts.contains(&r.pc_after) || r.pc_after as usize == code.words.len()
                    );
                }
            }
            assert!(!bytecode::disasm(&code).is_empty());
            funcs += 1;
        }
    }
    assert!(funcs > 1000, "{funcs}");
}
