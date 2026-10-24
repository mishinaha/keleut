# Repository Guidelines

## Project Structure & Module Organization

- `doc/sample.kel` is the Keleut language specification: its comments define syntax and semantics. `doc/log/` records design and status notes.
- `diktor/` is a Git submodule containing Diktor, the OCaml reference interpreter of Keleut. It serves as the test oracle for RustKeleut, the main implementation, and as the reference implementation of KIR. Core modules live in `lib/`, the CLI entry point in `bin/main.ml`, and Cram tests in `test/*.t`. Keep implementation logic in the library.
- `diktor/lib/prelude.kel` provides the prelude; `diktor/test/sample/` holds specification fixtures and stubs.
- `rustkeleut/` is the Cargo workspace of RustKeleut, the main implementation (edition 2024, resolver 3, `rust-version = "1.98"`, members `crates/*`; versions are pinned in `Cargo.toml`). `crates/kir` reads, verifies and writes KIR (binary `.kir` and text `.kirt`, byte-compatible with Diktor's `--emit-ir` and `--emit-ir-text`; spec in `doc/IRSpec.md`); `crates/keleut-rtcore` builds `rtcore/` (freestanding C) for x86_64 and wasm32; `crates/keleut-cli` is the `keleut` binary; `crates/kconform` is the differential test runner. `conformance/` holds `known-diffs.toml`, the generated `corpus/` (`.kel` files extracted from `diktor/test/*.t`), `kir-bad/` (malformed KIR that must be rejected with exit 65) and the other cram-form conformance tests.
- `reference/` contains ignored local reference material, including `MiniLang.scala` when available; a fresh clone does not supply it.

## Build, Test, and Development Commands

Initialize the interpreter from the repository root:

```sh
git submodule update --init --recursive
cd diktor
```

Use an opam switch with OCaml >= 5.2. Run these commands inside `diktor/`:

- `opam install dune menhir sedlex` — install build dependencies (Dune >= 3.0).
- `dune build` — compile the interpreter and library.
- `dune runtest` — run the Cram golden tests.
- `dune exec diktor -- path/to/program.kel` — execute a Keleut file; add `--type-check` before the path to check types only.
- `awk -f tools/weave.awk lib/unify.ml > /tmp/unify.md` — render a literate source chapter as Markdown.
- `python3 tools/litwrap.py lib/unify.ml` — wrap long lines in literate article blocks after Japanese commas.
- `python3 tools/litcheck.py` — after comment-only edits, confirm that the code tokens still match `HEAD` and that the comments keep the literate conventions (no `"` or unpaired `(*`, block indentation, line width).

Run these commands inside `rustkeleut/` (no rustup is needed; only the x86_64 standard library):

- `cargo build --release`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`, `cargo build --no-default-features` — the checks every change must pass.
- `cargo test -p kir` — KIR reader, writer and verifier tests; set `KIR_CORPUS_DIR` or `KIR_DIKTOR` for the optional corpus and Diktor cross-checks.
- `target/release/keleut kir verify|dis|asm FILE`, `target/release/keleut kir tables` — KIR tools: `verify` reads and verifies (exit 65 when invalid), `dis` prints canonical text, `asm FILE.kirt -o OUT.kir` writes canonical binary, `tables` prints the builtin tables like `diktor --dump-ir-tables`. `--max-nest N` and `--native-stack BYTES` change the nesting limit and the stack of the KIR thread (a stack too small for the limit lowers the limit).
- `target/release/kconform --pairs dk ../diktor/test/*.t` — run Diktor's cram tests through the PATH shim and compare each command with the recorded expectation (add `--check-shim` to fail when a `diktor` call bypasses the shim).
- `target/release/kconform expect conformance/kir-bad/*.t` — run conformance tests against their recorded expectations only; `--promote` rewrites them with the actual results (review the diff).
- `target/release/kconform kir-roundtrip` — on every corpus program, compare `keleut kir dis` and `asm` with Diktor's text and binary output and check the text → binary → text round trip.
- `target/release/kconform extract-corpus [--check]` — regenerate (or check) `conformance/corpus/` after `diktor/test` changes.
- kconform finds the real Diktor through `--diktor`, then `KELEUT_DIKTOR`, then `../diktor/_build/install/default/bin/diktor`, and the `keleut` next to itself, so build both with `cargo build --release` first.

## Coding Style & Naming Conventions

Follow existing OCaml code: two-space indentation, `snake_case` values and filenames, and capitalized modules and constructors. Preserve Japanese literate explanations and column-zero `(* ... *)` article blocks. Do not run `ocamlformat` or `dune fmt`; they break that layout despite the existing `.ocamlformat` file. In `rustkeleut/`, write rustdoc comments in Japanese, and format with `cargo fmt`; the literate layout does not apply there. Follow nearby `.kel` examples when editing the specification.

## Testing Guidelines

Add focused regression cases to descriptive `test/*.t` files such as `trailing_comma.t`. Cover successful behavior and relevant diagnostics or exit codes. No numeric coverage threshold is configured.

RustKeleut conformance tests are cram-form `.t` files under `rustkeleut/conformance/`, with their `.kel`, `.kir` or `.kirt` inputs beside them; they call `keleut` directly, and `kconform expect` compares them with the recorded output. Never change `diktor/test` expectations for RustKeleut's sake; record unavoidable differences in `conformance/known-diffs.toml` with a ledger number.

Review golden-output differences before `dune promote`; commit expectation updates separately from implementation changes. Never hand-edit `test/sample/sample.kel`: synchronize it from the parent specification using `test/sample/README.md`, updating provenance and affected source references together.

## Commit & Pull Request Guidelines

Use small logical commits with `scope: summary` subjects; history uses `spec:`, `diktor:`, `test:`, and `doc:`. Follow `.claude/rules/commit.md`: Japanese messages on `ja/` branches, English elsewhere; never bypass hooks or signing.

PR descriptions should explain behavior changes, cite affected specification sections or issues, and report validation commands and results. Commit submodule changes within `diktor/`, then record its updated revision in the parent repository.
