# 測定用の一時の改変(リポジトリには入れていない)

HEAD 69febfd の rustkeleut/ を scratchpad に写し、次の 2 か所だけを変えて `cargo build --release -p keleut-cli` で組んだ。
環境変数 KELEUT_FREE_FRONT があれば drop、無ければ従来どおり forget する。

- crates/keleut-cli/src/check_cmd.rs:135 `std::mem::forget(c);`
  → `if std::env::var_os("KELEUT_FREE_FRONT").is_some() { drop(c) } else { std::mem::forget(c) }`
- crates/keleut-cli/src/emit_kir_cmd.rs:210 同じ置き換え
