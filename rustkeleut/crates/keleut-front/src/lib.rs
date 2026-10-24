//! RustKeleut のフロントエンドである(計画 261024-1 の 7 節)。
//!
//! M96 で、字句解析、構文解析と脱糖、`keleut dump tokens|ast` の印字を、
//! M97 で、型検査(`keleut check`)を足した。
//! どれも Diktor(参照インタプリタ)と同じ結果を出すことを目標にしている。
//!
//! | モジュール | Diktor の対応 | 中身 |
//! |---|---|---|
//! | [`lexer`] | lexer.ml | 字句、`{` の再分類、ASI |
//! | [`parser`] | driver.ml の §16.3、§16.3a | LR(1) の駆動、構文エラーの位置と文面、予約語の誤用 |
//! | `actions` | parser.mly | 規則の意味の動作と脱糖 |
//! | `tables` | parser.mly(menhir) | LR(1) の表(`tools/gen_tables.ml` が作る) |
//! | [`ast`] | syntax.ml | 脱糖した木 |
//! | [`dump`] | dump.ml | S 式の印字 |
//! | [`oformat`] | OCaml の `Format` | hov 箱の整形 |
//! | [`types`] | decls.ml、unify.ml、show.ml、exhaust.ml、elab.ml、driver.ml の型検査の手順 | 宣言表、単一化、型の表示、網羅性、精緻化(M97) |
//!
//! 構文解析は表で駆動する LR(1) の自動機械で、スタックはヒープの上の明示的な配列である。
//! スタックの段の数には限度を設けず、平らな長い列は Diktor と同じく受理する。
//! 作る木の深さを入れ子の計数器で数え、限度
//! ([`parser::DEFAULT_MAX_NEST`])を超えたら終了状態 2 の「処理系の限度」の診断で止まる。
//! 木をたどる後の段(印字、捨てること)は再帰するので、
//! [`on_stack`] で限度に見合うスタックを持つ専用のスレッドで走らせる(計画 6.11 節)。

pub mod actions;
pub mod ast;
pub mod dump;
pub mod escape;
pub mod lexer;
pub mod oformat;
pub mod parser;
pub mod pos;
#[rustfmt::skip]
#[allow(dead_code)]
mod tables;
pub mod token;
pub mod types;

pub use parser::{DEFAULT_MAX_NEST, ParseError};
pub use pos::{FileId, Files, Pos, Span};

/// 木の深さ 1 段あたりに見込むスタックのバイト数である。
///
/// 木をたどる再帰(S 式への変換と、木を捨てること)の 1 段より大きく取ってある。
pub const STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) { 8192 } else { 2048 };

/// 処理の限度である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// 構文解析で作る木の深さの上限。
    pub max_nest: usize,
    /// 専用のスレッドのスタックの大きさ(バイト)。
    pub stack_bytes: usize,
}

impl Limits {
    /// 深さの上限に足りるスタックを見積もった限度。
    pub fn for_nest(max_nest: usize) -> Limits {
        Limits {
            max_nest,
            stack_bytes: max_nest
                .saturating_mul(STACK_BYTES_PER_LEVEL)
                .saturating_add(32 << 20),
        }
    }

    /// スタックの大きさを決めた限度。深さの上限は、スタックに収まる深さまで下げる。
    pub fn with_stack(max_nest: usize, stack_bytes: usize) -> Limits {
        let margin = (stack_bytes / 4).clamp(64 << 10, 32 << 20);
        let fits = stack_bytes.saturating_sub(margin) / STACK_BYTES_PER_LEVEL;
        Limits {
            max_nest: max_nest.min(fits),
            stack_bytes,
        }
    }
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::for_nest(DEFAULT_MAX_NEST)
    }
}

/// `f` を大きさ `stack_bytes` のスタックを持つ専用のスレッドで走らせる。
/// スレッドを作れないときと、`f` が落ちたときは `Err` に文面を入れて返す。
pub fn on_stack<T: Send>(stack_bytes: usize, f: impl FnOnce() -> T + Send) -> Result<T, String> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("keleut-front".to_string())
            .stack_size(stack_bytes)
            .spawn_scoped(s, f)
            .map_err(|e| format!("フロントエンドのスレッドを作れません: {e}"))?;
        h.join()
            .map_err(|_| "フロントエンドの中で内部の誤りが起きました".to_string())
    })
}

/// ソースのトークンの列(ASI を適用した後、EOF まで)を返す。
pub fn tokens(src: &[u8], file: FileId) -> Result<Vec<lexer::Entry>, lexer::LexError> {
    lexer::Lexer::new(src, file).all_tokens()
}

/// ソースを構文解析する(Diktor の `parse_unit_with`)。
///
/// `file` は `files` に登録したファイルである。
/// import 文はファイルの先頭にだけ書ける形で受け、結果の [`ast::Program::imports`] に入る。
/// 呼ぶたびに新しい文脈を作るので、ノードの番号は 1 つのファイルの中でだけ重ならない。
/// 複数のファイルや REPL の入力で番号を重ねないときは [`parse_with`] を使う。
pub fn parse(
    src: &[u8],
    file: FileId,
    files: &Files,
    max_nest: usize,
) -> Result<ast::Program, ParseError> {
    parse_with(&mut actions::Ctx::new(max_nest), src, file, files)
}

/// 文脈 `cx` を使ってソースを構文解析する。
///
/// 限度は `cx` の限度である。
/// ノードの番号は `cx` が続きから振るので、同じ `cx` で解析した木どうしで番号は重ならない。
/// 限度で失敗した後の `cx` も、次の解析に使える。
pub fn parse_with(
    cx: &mut actions::Ctx,
    src: &[u8],
    file: FileId,
    files: &Files,
) -> Result<ast::Program, ParseError> {
    parser::parse_program(src, file, files, cx)
}

#[cfg(test)]
mod tests {
    /// 表が今の Diktor の文法から作ったものであることを確かめる。
    /// diktor のサブモジュールか `md5sum` が無ければ調べない。
    #[test]
    fn 表は今の_parser_mly_から作ってある() {
        let mly = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../diktor/lib/parser.mly"
        );
        if !std::path::Path::new(mly).exists() {
            return;
        }
        let Ok(o) = std::process::Command::new("md5sum").arg(mly).output() else {
            return;
        };
        assert!(o.status.success(), "md5sum が失敗しました");
        let text = String::from_utf8_lossy(&o.stdout);
        let digest = text.split_whitespace().next().unwrap_or("");
        assert_eq!(
            digest,
            crate::tables::PARSER_MLY_MD5,
            "diktor/lib/parser.mly が表を作った後に変わりました。\
             tools/gen_tables.ml の冒頭の手順で src/tables.rs を作り直し、\
             src/actions.rs の動作を新しい規則に合わせてください"
        );
    }
}
