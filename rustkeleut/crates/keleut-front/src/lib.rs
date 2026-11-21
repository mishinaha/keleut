//! RustKeleut のフロントエンドである。
//!
//! 字句解析、構文解析と脱糖、トークンと AST の出力(`keleut dump tokens` と `keleut dump ast`)、型検査(`keleut check`)、KIR の書き出し(`keleut emit-kir`)を行う。
//! どの処理も、Diktor(参照インタプリタ)と同じ結果を出すことを目標にしている。
//!
//! | モジュール | Diktor の対応 | 内容 |
//! |---|---|---|
//! | [`lexer`] | lexer.ml | 字句解析、`{` の再分類、ASI |
//! | [`parser`] | driver.ml の §16.3、§16.3a | LR(1) のドライバ、構文エラーの位置とメッセージ、予約語の誤用 |
//! | [`actions`] | parser.mly | 規則の意味動作と脱糖 |
//! | `tables` | parser.mly(menhir) | LR(1) の表(`tools/gen_tables.ml` が作る) |
//! | [`ast`] | syntax.ml | 脱糖した木 |
//! | [`dump`] | dump.ml | S 式の出力 |
//! | [`oformat`] | OCaml の `Format` | hov ボックスによる整形 |
//! | [`escape`] | OCaml の `String.escaped` | 文字列のエスケープ |
//! | [`types`] | decls.ml、unify.ml、show.ml、exhaust.ml、elab.ml、driver.ml の型検査の手順、ir_write.ml | 宣言表、単一化、型の表示、網羅性、精緻化、KIR の書き出し |
//!
//! 構文解析は、表で駆動する LR(1) のオートマトンで行う。
//! オートマトンのスタックは、ヒープ上に確保した配列である。
//! スタックの段数には上限を設けないので、入れ子にならない長い並びは Diktor と同じく受理する。
//! 意味動作が作る木の深さは、各ノードの [`ast::Meta`] に記録する。
//! 深さが上限(既定値は [`parser::DEFAULT_MAX_NEST`])を超えたら、構文解析は `処理系の限度` の診断(終了コード 2)を返して失敗する。
//! 構文解析より後の段階(S 式への変換、木のドロップ)は木を再帰でたどるので、[`on_stack`] を使い、深さの上限に必要なスタックを持つ専用のスレッドで実行する。

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

/// 木の深さ 1 段あたりに見積もるスタックのバイト数である。
///
/// 木をたどる再帰(S 式への変換と、木のドロップ)が 1 段で使う量より大きくしてある。
pub const STACK_BYTES_PER_LEVEL: usize = if cfg!(debug_assertions) { 8192 } else { 2048 };

/// フロントエンドの処理の上限(木の深さとスタックサイズ)である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// 構文解析で作る木の深さの上限。
    pub max_nest: usize,
    /// 専用のスレッドのスタックサイズ(バイト)。
    pub stack_bytes: usize,
}

impl Limits {
    /// 深さの上限 `max_nest` に必要なスタックサイズを見積もって、上限の組を作る。
    ///
    /// 見積もりは、1 段あたり [`STACK_BYTES_PER_LEVEL`] バイトと、深さによらない 32 MiB の和である。
    pub fn for_nest(max_nest: usize) -> Limits {
        Limits {
            max_nest,
            stack_bytes: max_nest
                .saturating_mul(STACK_BYTES_PER_LEVEL)
                .saturating_add(32 << 20),
        }
    }

    /// スタックサイズ `stack_bytes` を指定して、上限の組を作る。
    ///
    /// 深さの上限は、スタックに収まる深さまで下げる。
    /// 収まる深さは、スタックの 4 分の 1(64 KiB 以上、32 MiB 以下)を余裕として除いた残りを、1 段あたり [`STACK_BYTES_PER_LEVEL`] バイトで割った値である。
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

/// `f` を、スタックサイズが `stack_bytes` バイトの専用のスレッドで実行する。
/// スレッドを作れないときと、`f` がパニックしたときは、`Err` にメッセージを入れて返す。
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

/// ソースのトークン列を返す。
/// ASI を適用した後のトークン列で、末尾の EOF のトークンも含む。
pub fn tokens(src: &[u8], file: FileId) -> Result<Vec<lexer::Entry>, lexer::LexError> {
    lexer::Lexer::new(src, file).all_tokens()
}

/// ソースを構文解析する(Diktor の `parse_unit_with` に対応する)。
///
/// `file` は `files` に登録したファイルである。
/// import 文は、ファイルの先頭に書いたものだけを受理し、結果の [`ast::Program::imports`] に入れる。
/// 呼び出すたびに新しいコンテキストを作るので、ノードの番号が重ならないのは 1 つのファイルの中だけである。
/// 複数のファイルや REPL の入力の間で番号が重ならないようにするには、[`parse_with`] を使う。
pub fn parse(
    src: &[u8],
    file: FileId,
    files: &Files,
    max_nest: usize,
) -> Result<ast::Program, ParseError> {
    parse_with(&mut actions::Ctx::new(max_nest), src, file, files)
}

/// コンテキスト `cx` を使ってソースを構文解析する。
///
/// 木の深さの上限は、`cx` が持つ上限([`actions::Ctx::limit`])である。
/// ノードの番号は `cx` が前回の続きから振るので、同じ `cx` で解析した木どうしで番号は重ならない。
/// 深さの上限を超えて失敗した後の `cx` も、次の解析に使える。
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
    /// LR(1) の表が、現在の Diktor の文法から作ったものであることを確かめる。
    /// diktor のサブモジュールか `md5sum` がなければ、何も調べない。
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
