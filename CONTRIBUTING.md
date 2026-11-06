# Contributing

## Workflow

1. Create a branch from `main`: `feat/<topic>`, `fix/<topic>`
2. Make your changes, with tests
3. Check locally before committing:
   ```bash
   cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
   pnpm format && pnpm lint && pnpm check && pnpm test
   ```
4. Open a pull request against `main`

## Commits

[Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`.
A scope is optional, e.g. `feat(capture): Wayland screenshot via portal`.

## Principles

- **Logic belongs in `crates/*`**, not in `src-tauri`, so it stays testable without a GUI.
- **No hard-coded UI strings:** new strings go into `src/lib/i18n/en.json` **and** `de.json` (a test checks that both have the same keys).
- **Changing the model:** edit the Rust types in `framenab-core/src/model.rs` and run `pnpm bindings`. The generated files in `src/lib/bindings/` are committed.
- **Platform code** lives behind `CaptureBackend` (`framenab-capture`) and is separated with `#[cfg(...)]`.
- **Language:** code, comments, docs and commit messages are in English.
