//! ホストに渡す表と、その JSON への書き出し。
//!
//! 表は、診断メッセージのテンプレートと、値の表示に必要な名前を持つ。
//! 実行時エラーのメッセージは、インタプリタ(`keleut-interp`)と同じバイト列にする。
//! RustKeleut のバックエンドの出力は、互いにバイト単位で一致しなければならないからである。
//! メッセージのうち値によらない部分はここで作り、値を表示する部分(`<値>`)だけをホストが追加する。

use std::collections::HashMap;

use keleut_lir::{GlobalKind, Program, RtError};

/// テンプレートの種類。
///
/// 番号は、ホストの定数 `K_RT`〜`K_INTERNAL` と同じである。
/// 各定数の説明の括弧の中は、その種類の診断で終了するときの終了コードである。
pub mod kind {
    /// 実行時エラー(終了コード 3)。
    pub const RT: u8 = 0;
    /// 論理的な深さが上限を超えた実行時エラー(終了コード 3)。
    pub const DEPTH: u8 = 1;
    /// メモリ不足(終了コード 3)。
    pub const OOM: u8 = 2;
    /// 標準出力への出力エラー(終了コード 74)。メッセージはホストが作る。
    pub const IO: u8 = 3;
    /// どのハンドラも処理しない操作(終了コード 3)。
    pub const UNHANDLED: u8 = 4;
    /// 未実装の機能(終了コード 4)。巻き戻さずに終了する。
    pub const UNIMPL: u8 = 5;
    /// 内部エラー(終了コード 70)。巻き戻さずに終了する。
    pub const INTERNAL: u8 = 6;
}

/// テンプレートの後ろにホストが追加するものの種類。
///
/// 番号は、ホストの定数 `A_NONE`〜`A_ARGS` と同じである。
pub mod arg {
    /// 何も追加しない。
    pub const NONE: u8 = 0;
    /// 値を文字列化して追加する。
    pub const SHOW: u8 = 1;
    /// `Float64` の値(ビット列で渡す)を文字列化して追加する。
    pub const F64: u8 = 2;
    /// 文字列のバイト列をそのまま追加する。
    pub const STR: u8 = 3;
    /// 引数の配列を `(a, b)` の形式で文字列化して追加する。
    pub const ARGS: u8 = 4;
}

/// 診断メッセージのテンプレートの表。
///
/// テンプレートは(種類 [`kind`]、追加するもの [`arg`]、メッセージ)の組で、表の添字がテンプレートの番号になる。
#[derive(Default)]
pub struct Templates {
    list: Vec<(u8, u8, String)>,
    map: HashMap<(u8, u8, String), u32>,
}

impl Templates {
    /// テンプレートを追加して番号を返す。
    /// 同じテンプレートがすでにあれば、その番号を返す。
    pub fn add(&mut self, kind: u8, arg: u8, text: impl Into<String>) -> u32 {
        let k = (kind, arg, text.into());
        if let Some(&i) = self.map.get(&k) {
            return i;
        }
        let i = self.list.len() as u32;
        self.list.push(k.clone());
        self.map.insert(k, i);
        i
    }

    /// 内部エラーのテンプレートを追加して番号を返す。
    pub fn internal(&mut self, text: &str) -> u32 {
        self.add(kind::INTERNAL, arg::NONE, text)
    }
}

/// シンボルから、単位のキー(最後の `/` まで)と末尾の `#n`(`n` は 10 進の数字の並び)を取り除いた名前を返す(IRSpec 3.1 節)。
///
/// `keleut-rt` の `display_of_symbol` と同じ規則である。
pub fn display_of_symbol(s: &str) -> &str {
    let s = match s.rfind('/') {
        Some(i) => &s[i + 1..],
        None => s,
    };
    if let Some(i) = s.rfind('#') {
        let d = &s[i + 1..];
        if !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()) {
            return &s[..i];
        }
    }
    s
}

/// ラベルやコンストラクタの名前から、表示に使う名前を返す(`keleut-rt` の `display_name` と同じ規則)。
///
/// `%` で始まる内部名は、最初の `.` より後ろの部分を返す。
pub fn display_name(name: &str) -> &str {
    if name.len() > 1 && name.starts_with('%') {
        match name.find('.') {
            Some(i) => &name[i + 1..],
            None => name,
        }
    } else {
        name
    }
}

/// 実行時エラー `k` のテンプレートを追加して番号を返す。
///
/// メッセージは、`keleut-interp` が同じ実行時エラーで書くものと同じである。
pub fn error_template(t: &mut Templates, prog: &Program, k: &RtError) -> u32 {
    use arg::*;
    use kind::*;
    match k {
        RtError::Hole => t.add(RT, NONE, "??? に到達しました"),
        RtError::Unbound(g) => t.add(
            RT,
            NONE,
            format!("未束縛の変数: {}", prog.globals[g.index()].display),
        ),
        RtError::PatternFail => t.add(RT, SHOW, "パターンに値が一致しません: "),
        RtError::MatchFail => t.add(RT, SHOW, "match のどの節にも一致しません: "),
        RtError::LitRange(s) => t.add(RT, NONE, format!("数値リテラルが範囲外です: {s}")),
        RtError::InstanceNotReady(i) => inst_not_ready(t, prog, i.index()),
        RtError::DivZero => t.add(RT, NONE, "ゼロ除算です"),
        RtError::F64ToIntNan(op) => t.add(
            RT,
            NONE,
            format!("{}: NaN は整数に変換できません", op.prim_name()),
        ),
        RtError::F64ToIntRange(op) => t.add(
            RT,
            F64,
            format!("{}: 変換結果が範囲外です: ", op.prim_name()),
        ),
        RtError::StrSubRange => t.add(RT, NONE, "__string_sub: 範囲外です"),
        RtError::Panic => t.add(RT, STR, "panic: "),
        RtError::ArrayIndex => t.add(RT, NONE, "配列の範囲外です"),
        RtError::MArrayNegLen => t.add(RT, NONE, "MutableArray.new: 長さが負です"),
        RtError::UnimplPrim(g) => unimpl_prim(t, prog, g.index()),
        RtError::OutOfMemory => t.add(OOM, NONE, ""),
    }
}

/// 初期化の前のインスタンス `i` からメソッドを選んだときの実行時エラー(IRSpec 付録 B の 6 番)のテンプレートを追加して番号を返す。
pub fn inst_not_ready(t: &mut Templates, prog: &Program, i: usize) -> u32 {
    t.add(
        kind::RT,
        arg::NONE,
        format!(
            "{} のインスタンスは宣言より前の位置では使えません(実行はまだ実体を持ちません。インスタンス宣言を使用より前に置いてください)",
            prog.instances[i].display
        ),
    )
}

/// 実装のないプリミティブ(グローバル `g`)を呼んだときの実行時エラーのテンプレートを追加して番号を返す。
///
/// IRSpec 付録 B の 17 番と 18 番に相当する。
/// 表示名と実装名が違うとき(18 番)は、メッセージに実装名も書く。
pub fn unimpl_prim(t: &mut Templates, prog: &Program, g: usize) -> u32 {
    let gl = &prog.globals[g];
    let name = match &gl.kind {
        GlobalKind::Extern { name, .. } => name.as_str(),
        _ => gl.display.as_str(),
    };
    let text = if prog.error_number(&RtError::UnimplPrim(keleut_lir::GlobalId(g as u32))) == 18 {
        format!(
            "未実装のプリミティブ: {}(実装名 {name} が見つかりません)",
            gl.display
        )
    } else {
        format!("未実装のプリミティブ: {}", gl.display)
    };
    t.add(kind::RT, arg::NONE, text)
}

/// `s` を JSON の文字列リテラルにして `out` に追記する。
fn js(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `xs` の各要素を `f` で書き、JSON の配列にして `out` に追記する。
fn js_list<T>(out: &mut String, xs: &[T], f: impl Fn(&mut String, &T)) {
    out.push('[');
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        f(out, x);
    }
    out.push(']');
}

/// ホストに渡す表を、JSON のオブジェクト 1 つとして返す。
///
/// `b_prog` はプログラムの 1 段に必要なバイト数 `B_prog`、`k` はその計算に使った `cancel` 節の数である([`crate::b_prog`])。
pub fn meta_json(prog: &Program, t: &Templates, max_depth: u64, b_prog: u64, k: u32) -> String {
    let mut o = String::new();
    o.push_str("{\"maxDepth\":");
    o.push_str(&max_depth.to_string());
    // プログラムの 1 段に必要なバイト数 B_prog と、その計算に使った cancel 節の数 k を書く(crate::b_prog と crate::clause_path が求める値)。
    // B_prog は、ホストが Worker の経路でスタックサイズを決めるのに使う。
    o.push_str(",\"bProg\":");
    o.push_str(&b_prog.to_string());
    o.push_str(",\"cancelNest\":");
    o.push_str(&k.to_string());
    o.push_str(",\"templates\":");
    js_list(&mut o, &t.list, |o, (k, a, s)| {
        o.push_str(&format!("[{k},{a},"));
        js(o, s);
        o.push(']');
    });
    // labels はラベルの表示名、items はラベルが組のラベル(`_item`)なら 1 になるフラグである。
    o.push_str(",\"labels\":");
    js_list(&mut o, &prog.labels, |o, l| js(o, display_name(l)));
    o.push_str(",\"items\":");
    js_list(&mut o, &prog.labels, |o, l| {
        o.push_str(if l == "_item" { "1" } else { "0" })
    });
    o.push_str(",\"ctors\":");
    js_list(&mut o, &prog.types, |o, ty| {
        js_list(o, &ty.ctors, |o, c| js(o, display_name(&c.name)))
    });
    // 関数値を `<prim …>` と表示するときの名前を、関数ごとに持つ。
    // 名前を持つのは、組み込みの値の関数と extern の本体の関数だけである。
    let mut prim: Vec<Option<String>> = vec![None; prog.funcs.len()];
    for g in &prog.globals {
        match &g.kind {
            GlobalKind::Builtin { func }
            | GlobalKind::Extern {
                func: Some(func), ..
            } => {
                prim[func.index()] = Some(g.display.clone());
            }
            _ => {}
        }
    }
    o.push_str(",\"prim\":");
    js_list(&mut o, &prim, |o, p| match p {
        Some(s) => js(o, s),
        None => o.push_str("null"),
    });
    o.push_str(",\"methods\":");
    js_list(&mut o, &prog.classes, |o, c| {
        js_list(o, &c.methods, |o, m| {
            js(o, &format!("<prim {}.{}>", display_of_symbol(&c.sym), m))
        })
    });
    o.push_str(",\"shapes\":");
    js_list(&mut o, &prog.shapes, |o, s| {
        js_list(o, s, |o, l| o.push_str(&l.0.to_string()))
    });
    o.push_str(",\"globals\":");
    js_list(&mut o, &prog.globals, |o, g| js(o, &g.display));
    o.push('}');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名前の表示() {
        assert_eq!(display_of_symbol("main/M.__no_such"), "M.__no_such");
        assert_eq!(display_of_symbol("main/__no_such#1"), "__no_such");
        assert_eq!(display_name("%3.Foo"), "Foo");
        let mut s = String::new();
        js(&mut s, "a\"\n\u{1}あ");
        assert_eq!(s, "\"a\\\"\\n\\u0001あ\"");
    }
}
