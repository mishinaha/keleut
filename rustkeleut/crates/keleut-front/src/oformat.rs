//! OCaml の `Format` の整形の写しである(OCaml 5.2 の stdlib/format.ml)。
//!
//! Diktor の `--dump-ast` は、S 式を `Format` の hov 箱と区切りで印字する。
//! 改行と字下げの位置を同じにするため、`Format` の待ち行列と走査スタックの算法をそのまま写す。
//! 写すのは、箱(`pp_open_box_gen`)、文字列、区切り(`pp_print_break`)、箱を閉じること、
//! 改行つきの吐き出し(`pp_print_newline`、書式の `@.`)だけである。
//! 余白は 78、最大の字下げは 68(`formatter_of_out_channel` の既定)、箱の数の上限は無い。

use std::collections::VecDeque;

const INFINITY: i64 = 1_000_000_010;

/// 箱の種類である。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxType {
    H,
    V,
    Hv,
    Hov,
    B,
    Fits,
}

#[derive(Clone, Debug)]
enum Token {
    Text(String),
    Break {
        fits: (String, i64, String),
        breaks: (String, i64, String),
    },
    Begin(i64, BoxType),
    End,
}

struct Elem {
    size: i64,
    token: Token,
    length: i64,
}

struct ScanElem {
    left_total: i64,
    /// 待ち行列の要素の通し番号(`None` は番兵)。
    elem: Option<u64>,
    /// 要素が区切りか(偽なら箱の始まり)。
    is_break: bool,
}

struct FormatElem {
    box_type: BoxType,
    width: i64,
}

/// 整形器である。出力はバイト列に溜める。
pub struct Formatter {
    scan_stack: Vec<ScanElem>,
    format_stack: Vec<FormatElem>,
    margin: i64,
    max_indent: i64,
    space_left: i64,
    current_indent: i64,
    is_new_line: bool,
    left_total: i64,
    right_total: i64,
    curr_depth: i64,
    queue: VecDeque<Elem>,
    /// `queue` の先頭の要素の通し番号。
    queue_base: u64,
    /// 整形した出力。
    pub out: Vec<u8>,
}

impl Default for Formatter {
    fn default() -> Formatter {
        Formatter::new()
    }
}

impl Formatter {
    /// `formatter_of_out_channel` と同じ既定の整形器。
    pub fn new() -> Formatter {
        let margin = 78;
        let min_space_left = 10;
        let mut f = Formatter {
            scan_stack: Vec::new(),
            format_stack: Vec::new(),
            margin,
            max_indent: margin - min_space_left,
            space_left: margin,
            current_indent: 0,
            is_new_line: true,
            left_total: 1,
            right_total: 1,
            curr_depth: 1,
            queue: VecDeque::new(),
            queue_base: 0,
            out: Vec::new(),
        };
        // 初期の状態は、仮の箱を 1 つ持つ。
        f.queue.push_back(Elem {
            size: -1,
            token: Token::Begin(0, BoxType::Hov),
            length: 0,
        });
        f.initialize_scan_stack();
        f.scan_stack.push(ScanElem {
            left_total: 1,
            elem: Some(0),
            is_break: false,
        });
        f
    }

    fn elem_mut(&mut self, seq: u64) -> Option<&mut Elem> {
        if seq < self.queue_base {
            return None;
        }
        self.queue.get_mut((seq - self.queue_base) as usize)
    }

    fn enqueue(&mut self, e: Elem) -> u64 {
        self.right_total += e.length;
        self.queue.push_back(e);
        self.queue_base + self.queue.len() as u64 - 1
    }

    fn output_str(&mut self, s: &str) {
        self.out.extend_from_slice(s.as_bytes());
    }

    fn output_spaces(&mut self, n: i64) {
        for _ in 0..n.max(0) {
            self.out.push(b' ');
        }
    }

    fn format_pp_text(&mut self, size: i64, text: &str) {
        self.space_left -= size;
        self.output_str(text);
        self.is_new_line = false;
    }

    fn format_string(&mut self, s: &str) {
        if !s.is_empty() {
            self.format_pp_text(s.len() as i64, s);
        }
    }

    fn break_new_line(&mut self, before: &str, offset: i64, after: &str, width: i64) {
        self.format_string(before);
        self.out.push(b'\n');
        self.is_new_line = true;
        let indent = self.margin - width + offset;
        let real_indent = self.max_indent.min(indent);
        self.current_indent = real_indent;
        self.space_left = self.margin - self.current_indent;
        let ci = self.current_indent;
        self.output_spaces(ci);
        self.format_string(after);
    }

    fn break_line(&mut self, width: i64) {
        self.break_new_line("", 0, "", width);
    }

    fn break_same_line(&mut self, before: &str, width: i64, after: &str) {
        self.format_string(before);
        self.space_left -= width;
        self.output_spaces(width);
        self.format_string(after);
    }

    fn force_break_line(&mut self) {
        match self.format_stack.last() {
            None => self.out.push(b'\n'),
            Some(&FormatElem { box_type, width }) => {
                if width > self.space_left {
                    match box_type {
                        BoxType::Fits | BoxType::H => {}
                        _ => self.break_line(width),
                    }
                }
            }
        }
    }

    fn format_pp_token(&mut self, size: i64, token: Token) {
        match token {
            Token::Text(s) => self.format_pp_text(size, &s),
            Token::Begin(off, ty) => {
                let insertion_point = self.margin - self.space_left;
                if insertion_point > self.max_indent {
                    self.force_break_line();
                }
                let width = self.space_left - off;
                let box_type = match ty {
                    BoxType::V => BoxType::V,
                    _ => {
                        if size > self.space_left {
                            ty
                        } else {
                            BoxType::Fits
                        }
                    }
                };
                self.format_stack.push(FormatElem { box_type, width });
            }
            Token::End => {
                self.format_stack.pop();
            }
            Token::Break { fits, breaks } => {
                let Some(&FormatElem { box_type, width }) = self.format_stack.last() else {
                    return;
                };
                let (before, off, after) = &breaks;
                let blen = before.len() as i64;
                match box_type {
                    BoxType::Hov => {
                        if size + blen > self.space_left {
                            self.break_new_line(before, *off, after, width);
                        } else {
                            self.break_same_line(&fits.0, fits.1, &fits.2);
                        }
                    }
                    BoxType::B => {
                        if self.is_new_line {
                            self.break_same_line(&fits.0, fits.1, &fits.2);
                        } else if size + blen > self.space_left
                            || self.current_indent > self.margin - width + off
                        {
                            self.break_new_line(before, *off, after, width);
                        } else {
                            self.break_same_line(&fits.0, fits.1, &fits.2);
                        }
                    }
                    BoxType::Hv | BoxType::V => self.break_new_line(before, *off, after, width),
                    BoxType::Fits | BoxType::H => self.break_same_line(&fits.0, fits.1, &fits.2),
                }
            }
        }
    }

    fn advance_left(&mut self) {
        while let Some(front) = self.queue.front() {
            let pending_count = self.right_total - self.left_total;
            if !(front.size >= 0 || pending_count >= self.space_left) {
                return;
            }
            let e = self.queue.pop_front().expect("覗いた要素");
            self.queue_base += 1;
            let size = if e.size >= 0 { e.size } else { INFINITY };
            self.format_pp_token(size, e.token);
            self.left_total += e.length;
        }
    }

    fn enqueue_advance(&mut self, e: Elem) {
        self.enqueue(e);
        self.advance_left();
    }

    fn initialize_scan_stack(&mut self) {
        self.scan_stack.clear();
        self.scan_stack.push(ScanElem {
            left_total: -1,
            elem: None,
            is_break: false,
        });
    }

    fn set_size(&mut self, ty: bool) {
        let Some(top) = self.scan_stack.last() else {
            return;
        };
        let (left_total, seq, is_break) = (top.left_total, top.elem, top.is_break);
        if left_total < self.left_total {
            self.initialize_scan_stack();
            return;
        }
        let right_total = self.right_total;
        let Some(seq) = seq else {
            return;
        };
        if is_break == ty {
            // 既に印字した要素なら、大きさを書いても効かない(OCaml でも同じ)。
            if let Some(e) = self.elem_mut(seq) {
                e.size += right_total;
            }
            self.scan_stack.pop();
        }
    }

    fn scan_push(&mut self, b: bool, e: Elem) {
        let is_break = matches!(e.token, Token::Break { .. });
        let seq = self.enqueue(e);
        if b {
            self.set_size(true);
        }
        self.scan_stack.push(ScanElem {
            left_total: self.right_total,
            elem: Some(seq),
            is_break,
        });
    }

    /// 箱を開く(`pp_open_box_gen`)。
    pub fn open_box(&mut self, indent: i64, ty: BoxType) {
        self.curr_depth += 1;
        let size = -self.right_total;
        self.scan_push(
            false,
            Elem {
                size,
                token: Token::Begin(indent, ty),
                length: 0,
            },
        );
    }

    /// 箱を閉じる(`pp_close_box`)。
    pub fn close_box(&mut self) {
        if self.curr_depth > 1 {
            self.enqueue(Elem {
                size: 0,
                token: Token::End,
                length: 0,
            });
            self.set_size(true);
            self.set_size(false);
            self.curr_depth -= 1;
        }
    }

    /// 文字列を印字する(`pp_print_string`)。幅はバイト数である。
    pub fn string(&mut self, s: &str) {
        let n = s.len() as i64;
        self.enqueue_advance(Elem {
            size: n,
            token: Token::Text(s.to_string()),
            length: n,
        });
    }

    /// 区切りを置く(`pp_print_break width offset`)。
    pub fn brk(&mut self, width: i64, offset: i64) {
        let size = -self.right_total;
        self.scan_push(
            true,
            Elem {
                size,
                token: Token::Break {
                    fits: (String::new(), width, String::new()),
                    breaks: (String::new(), offset, String::new()),
                },
                length: width,
            },
        );
    }

    fn rinit(&mut self) {
        self.left_total = 1;
        self.right_total = 1;
        self.queue.clear();
        self.queue_base = 0;
        self.initialize_scan_stack();
        self.format_stack.clear();
        self.current_indent = 0;
        self.curr_depth = 0;
        self.space_left = self.margin;
        self.open_box(0, BoxType::Hov);
    }

    /// 待ち行列を吐き出し、改行を書く(`pp_print_newline`)。
    pub fn newline(&mut self) {
        while self.curr_depth > 1 {
            self.close_box();
        }
        self.right_total = INFINITY;
        self.advance_left();
        self.out.push(b'\n');
        self.rinit();
    }

    /// 溜めた出力を取り出す。
    pub fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.out)
    }
}
