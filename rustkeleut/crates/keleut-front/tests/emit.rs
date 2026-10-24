//! KIR の書き出し(`types::emit`)の試験である。
//!
//! 環境変数 `KELEUT_FRONT_DIKTOR` で本物の diktor を与えたときは、
//! `KELEUT_FRONT_CORPUS`(既定は `conformance/corpus`)の全件で、
//! 書き出したテキストとバイナリを `diktor --emit-ir-text` と `--emit-ir` の出力とバイト単位で比べる。
//! 旗はプレリュードありと `--no-prelude` の 2 通りで、Diktor が 0 で終わる組だけを比べる。

use keleut_front::types::check::{CheckOptions, check_limits, check_program, on_check_stack};
use keleut_front::types::emit::program_of_checked;
use keleut_front::types::load::{SourceFs, StdFs};

/// 作業のディレクトリを `0` とみなすファイルシステムである。
struct DirFs(std::path::PathBuf);

impl DirFs {
    fn at(&self, p: &str) -> String {
        if p.starts_with('/') {
            p.to_string()
        } else {
            self.0.join(p).to_string_lossy().into_owned()
        }
    }
}

impl SourceFs for DirFs {
    fn read(&mut self, p: &str) -> Result<Vec<u8>, String> {
        let q = self.at(p);
        StdFs.read(&q)
    }
    fn exists(&mut self, p: &str) -> bool {
        let q = self.at(p);
        StdFs.exists(&q)
    }
    fn realpath(&mut self, p: &str) -> Result<String, String> {
        let q = self.at(p);
        StdFs.realpath(&q)
    }
}

/// `dir` を作業のディレクトリとして `files` を書き出す。型検査か鍵で止まれば `None`。
fn emit(dir: std::path::PathBuf, files: Vec<String>, no_prelude: bool) -> Option<kir::Program> {
    let opts = CheckOptions {
        no_prelude,
        prelude: None,
        files,
        import_path: Vec::new(),
        max_nest: keleut_front::DEFAULT_MAX_NEST,
    };
    let limits = check_limits(keleut_front::DEFAULT_MAX_NEST, None).unwrap();
    on_check_stack(limits, move || {
        let mut fs = DirFs(dir);
        let mut c = check_program(&opts, &mut fs).ok()?;
        if c.output.error.is_some() || c.output.panic.is_some() {
            return None;
        }
        let keys = c.unit_keys(&mut fs, &opts).ok()?;
        Some(program_of_checked(&mut c, &keys, &opts).expect("書き出しの内部の誤り"))
    })
    .unwrap()
}

#[test]
fn 書いた_kir_は検証を通り往復する() {
    let d = std::env::temp_dir().join(format!("keleut-front-emit-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(
        d.join("a.kel"),
        "effect E = { ask: () => Int32 }\n\
         let f(x: Int32): Int32 @ {E} = x + perform ask()\n\
         let g = fn(y) => f(y) handle { case ask() => resume(2) case return(v) => v * 2 }\n\
         let rec loop(n: Int32): Int32 = if n == 0 then 0 else loop(n - 1)\n\
         type class Sh[A] { val sh: (A) => String }\n\
         type instance Sh[Int32] { let sh(x) = show(x) }\n\
         let (p, q) = (1, 2.5)\n\
         echoln(sh(g(p)) + show(q) + show(loop(3)))\n",
    )
    .unwrap();
    let p = emit(d.clone(), vec!["a.kel".into()], false).expect("型検査に通る");
    kir::verify(&p).unwrap();
    let bin = kir::write_kir(&p).unwrap();
    let text = kir::write_kirt(&p).unwrap();
    assert_eq!(kir::write_kirt(&kir::load(&bin).unwrap()).unwrap(), text);
    assert_eq!(kir::write_kir(&kir::load(&text).unwrap()).unwrap(), bin);
    std::fs::remove_dir_all(&d).unwrap();
}

/// 本物の diktor が与えられたら、コーパスの全件で書き出しを比べる。
#[test]
fn コーパスで_diktor_と同じ_kir_を書く() {
    let Some(diktor) = std::env::var_os("KELEUT_FRONT_DIKTOR") else {
        return;
    };
    let corpus = std::env::var_os("KELEUT_FRONT_CORPUS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/corpus")
        });
    let mut files = Vec::new();
    let mut stack = vec![corpus];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "kel") {
                files.push(p);
            }
        }
    }
    files.sort();
    let out = std::env::temp_dir().join(format!("keleut-front-emit-c-{}", std::process::id()));
    std::fs::create_dir_all(&out).unwrap();
    let (kirt, kirb) = (out.join("d.kirt"), out.join("d.kir"));
    let mut diffs = Vec::new();
    let mut compared = 0;
    for p in &files {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        let dir = p.parent().unwrap().to_path_buf();
        for no_prelude in [false, true] {
            let mut cmd = std::process::Command::new(&diktor);
            cmd.current_dir(&dir)
                .arg("--emit-ir-text")
                .arg(&kirt)
                .arg("--emit-ir")
                .arg(&kirb);
            if no_prelude {
                cmd.arg("--no-prelude");
            }
            let o = cmd.arg(&name).output().unwrap();
            let got = emit(dir.clone(), vec![name.clone()], no_prelude);
            match (o.status.code(), got) {
                (Some(0), Some(prog)) => {
                    compared += 1;
                    let text = kir::write_kirt(&prog).unwrap();
                    let bin = kir::write_kir(&prog).unwrap();
                    if text != std::fs::read(&kirt).unwrap() || bin != std::fs::read(&kirb).unwrap()
                    {
                        diffs.push(format!("{} {no_prelude}", p.display()));
                    }
                }
                (Some(0), None) => diffs.push(format!("{} {no_prelude}: 書けない", p.display())),
                _ => {}
            }
        }
    }
    std::fs::remove_dir_all(&out).unwrap();
    assert!(compared > 500, "比べた数が少ない: {compared}");
    assert!(diffs.is_empty(), "Diktor と違う: {diffs:?}");
}
