# Mitwirken

## Ablauf

1. Branch von `main` erstellen: `feat/<thema>`, `fix/<thema>`
2. Änderungen mit Tests
3. Vor dem Commit lokal prüfen:
   ```bash
   cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
   pnpm format && pnpm lint && pnpm check && pnpm test
   ```
4. Pull Request gegen `main`

## Commits

[Conventional Commits](https://www.conventionalcommits.org/de/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`.
Optional mit Bereich, z. B. `feat(capture): Wayland-Screenshot über Portal`.

## Grundsätze

- **Logik gehört in `crates/*`**, nicht nach `src-tauri`. So bleibt sie ohne GUI testbar.
- **Keine hartcodierten UI-Texte:** Neue Texte kommen in `src/lib/i18n/de.json` **und** `en.json` (ein Test prüft, dass beide dieselben Schlüssel haben).
- **Modell ändern:** Rust-Typen in `framenab-core/src/model.rs` anpassen und `pnpm bindings` ausführen. Die generierten Dateien in `src/lib/bindings/` werden mit committet.
- **Plattformcode** steckt hinter `CaptureBackend` (`framenab-capture`) und wird per `#[cfg(...)]` getrennt.
