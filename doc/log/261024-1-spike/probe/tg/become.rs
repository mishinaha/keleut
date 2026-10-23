fn f(n: u64) -> u64 { if n == 0 { 0 } else { become f(n - 1) } }
fn main() { println!("{}", f(10)); }
