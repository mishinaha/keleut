#![allow(incomplete_features)]
fn f(n: u64) -> u64 { if n == 0 { 0 } else { become g(n - 1) } }
fn g(n: u64) -> u64 { if n == 0 { 1 } else { become f(n - 1) } }
fn main() { println!("{}", f(10)); }
