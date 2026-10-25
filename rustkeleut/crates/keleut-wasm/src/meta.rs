//! ホストに渡す表(診断の文面の型紙と、値の印字に要る名前)と、その JSON。
//!
//! 実行時エラーの文面は、インタプリタ(`keleut-interp`)と同じバイト列にする(後段の間の完全な一致。D499)。
//! 文面のうち値に依らない部分はここで作り、値を印字する部分(`<値>`)だけをホストが足す。

use std::collections::HashMap;

use keleut_lir::{GlobalKind, Program, RtError};

/// 型紙の種類(ホストの `KIND` と同じ番号)。
pub mod kind {
    /// 実行時エラー(3)。
    pub const RT: u8 = 0;
    /// 論理的な深さの限度(3)。
    pub const DEPTH: u8 = 1;
    /// メモリの不足(3)。
    pub const OOM: u8 = 2;
    /// 標準出力の失敗(74)。文面はホストが作る。
    pub const IO: u8 = 3;
    /// どのハンドラも処理しない操作(3)。
    pub const UNHANDLED: u8 = 4;
    /// 未実装(4。巻き戻さずに止まる)。
    pub const UNIMPL: u8 = 5;
    /// 内部の誤り(70。巻き戻さずに止まる)。
    pub const INTERNAL: u8 = 6;
}

/// 型紙の後ろに足すもの。
pub mod arg {
    /// 何も足さない。
    pub const NONE: u8 = 0;
    /// 値を印字する。
    pub const SHOW: u8 = 1;
    /// `Float64` を印字する(ビット列)。
    pub const F64: u8 = 2;
    /// 文字列のバイト列をそのまま足す。
    pub const STR: u8 = 3;
    /// 引数の配列を `(a, b)` の形で印字する。
    pub const ARGS: u8 = 4;
}

/// 診断の型紙の表。
#[derive(Default)]
pub struct Templates {
    list: Vec<(u8, u8, String)>,
    map: HashMap<(u8, u8, String), u32>,
}

impl Templates {
    /// 型紙を足して番号を返す。同じものは 1 つにまとめる。
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

    /// 内部の誤り。
    pub fn internal(&mut self, text: &str) -> u32 {
        self.add(kind::INTERNAL, arg::NONE, text)
    }
}

/// 記号から単位の鍵(最後の `/` まで)と末尾の `#n` を除いた名前(IRSpec 3.1 節。`keleut-rt` の `display_of_symbol` と同じ)。
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

/// ラベルやコンストラクタの名前の表示(`keleut-rt` の `display_name` と同じ)。
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

/// 実行時エラーの型紙(`keleut-interp` の `error` と同じ文面)。
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

/// 付録 B の 6 番。
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

/// 付録 B の 17 番と 18 番。
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

/// JSON の文字列。
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

/// ホストに渡す表(JSON の 1 つのオブジェクト)。
pub fn meta_json(prog: &Program, t: &Templates, max_depth: u64) -> String {
    let mut o = String::new();
    o.push_str("{\"maxDepth\":");
    o.push_str(&max_depth.to_string());
    o.push_str(",\"templates\":");
    js_list(&mut o, &t.list, |o, (k, a, s)| {
        o.push_str(&format!("[{k},{a},"));
        js(o, s);
        o.push(']');
    });
    // ラベルの表示と、組のラベル(`_item`)か。
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
    // 関数ごとの `<prim …>` の名前(組み込みの値と extern の本体だけ)。
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
