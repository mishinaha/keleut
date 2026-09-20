# Repository Guidelines

## Project Structure & Module Organization

- `doc/sample.kel` is the Keleut language specification: its comments define syntax and semantics. `doc/log/` records design and status notes.
- `diktor/` is a Git submodule containing the OCaml bootstrap interpreter. Core modules live in `lib/`, the CLI entry point in `bin/main.ml`, and Cram tests in `test/*.t`. Keep implementation logic in the library.
- `diktor/lib/prelude.kel` provides the prelude; `diktor/test/sample/` holds specification fixtures and stubs.
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

## Coding Style & Naming Conventions

Follow existing OCaml code: two-space indentation, `snake_case` values and filenames, and capitalized modules and constructors. Preserve Japanese literate explanations and column-zero `(* ... *)` article blocks. Do not run `ocamlformat` or `dune fmt`; they break that layout despite the existing `.ocamlformat` file. Follow nearby `.kel` examples when editing the specification.

## Testing Guidelines

Add focused regression cases to descriptive `test/*.t` files such as `trailing_comma.t`. Cover successful behavior and relevant diagnostics or exit codes. No numeric coverage threshold is configured.

Review golden-output differences before `dune promote`; commit expectation updates separately from implementation changes. Never hand-edit `test/sample/sample.kel`: synchronize it from the parent specification using `test/sample/README.md`, updating provenance and affected source references together.

## Commit & Pull Request Guidelines

Use small logical commits with `scope: summary` subjects; history uses `spec:`, `diktor:`, `test:`, and `doc:`. Follow `.claude/rules/commit.md`: Japanese messages on `ja/` branches, English elsewhere; never bypass hooks or signing.

PR descriptions should explain behavior changes, cite affected specification sections or issues, and report validation commands and results. Commit submodule changes within `diktor/`, then record its updated revision in the parent repository.
