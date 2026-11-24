# Plano de implementação — Opção 1 + Opção 3

Baseado em `.grill/decision-record.md` (confirmado pelo usuário).

## Objetivo

1. **Opção 1**: avisar no app quando houver nova release do Rustify, sem auto-update.
2. **Opção 3**: publicar em canais de pacote, começando por AUR, depois Homebrew cask e Winget, com automação de release.

## Workstream A — Aviso de atualização no app

### A1. Modelo e persistência

**Arquivos**: `src/library.rs` (expor helpers), `src/update.rs` (novo), `src/main.rs` (mod).

- Expor helpers genéricos em `src/library.rs` como `pub(crate)`:
  - `pub(crate) fn storage_path(file_name: &str) -> Result<PathBuf, String>`
  - `pub(crate) fn load_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String>`
  - `pub(crate) fn save_json_atomic<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), String>`
  - Ajustar mensagens de erro para neutras (“armazenamento local”) onde fizer sentido, ou manter e aceitar contexto de biblioteca.
- Criar `src/update.rs` com:
  - `UpdateCheckState { enabled: bool, last_checked_at: Option<i64>, dismissed_version: Option<String>, latest_seen: Option<String> }` (serde default).
  - `UpdateInfo { current: String, latest: String, release_url: String, published_at: Option<String> }`.
  - `load_state()/save_state()` usando `storage_path("update-check.json")`.
  - `should_check(state, now)` respeitando `enabled` e cache de 24h.
  - `normalize_version(tag: &str) -> Option<semver-like>`: aceitar `vX.Y.Z`/`X.Y.Z`, comparar numericamente (major/minor/patch). Sem adicionar dependência se parser simples bastar.
  - `check_latest_release() -> Result<Option<UpdateInfo>, String>`: GET `https://api.github.com/repos/1arley/rustify/releases/latest` via `ureq`, timeout curto, User-Agent `rustify/<version>`. Falha silenciosa no chamador.
  - Filtrar prerelease/draft (`draft == true || prerelease == true` => ignorar).

### A2. Integração no app

**Arquivos**: `src/app.rs`.

- Adicionar estado em `Home`:
  - `update_state: UpdateCheckState`
  - `update_available: Option<UpdateInfo>`
  - `update_checking: bool`
  - `update_banner_dismissed_session: bool` (opcional)
- No `Home::new`/startup: carregar estado local. Se `should_check`, disparar `cx.spawn` + `background_executor` para checar. Ao receber resultado, atualizar `latest_seen`, `last_checked_at`, salvar estado, setar `update_available` se versão mais nova e não dispensada.
- Adicionar comandos/UI actions:
  - `CheckForUpdates` manual: ignora cache 24h, mas respeita `enabled`? Decisão: ação manual deve funcionar mesmo com cache; pode sinalizar “verificando”.
  - `DismissUpdateVersion`: grava `dismissed_version = latest`.
  - `ToggleUpdateChecks`: alterna `enabled` e salva.
  - `OpenReleasePage`: `open::that(release_url)`.
- Banner discreto no topo do conteúdo principal (ou abaixo do header): “Nova versão X.Y.Z disponível” + botões `Abrir release` e `Dispensar`. Só mostrar quando `update_available.is_some()` e versão não dispensada.
- Exibir versão atual (`env!("CARGO_PKG_VERSION")`) e controle de update em área visível: página Import/Config existente ou novo bloco “Sobre/Atualizações” na sidebar/config. Manter mudança pequena; preferir seção em Import ou modal simples.

### A3. Testes

**Arquivos**: `src/update.rs` (unit tests), possivelmente `src/ui_tests.rs`.

- Unit tests: parse/compare semver (`v0.2.0` < `v0.2.1`, ignora sufixo inesperado com cuidado), cache 24h, dismiss por versão, prerelease ignorado.
- Teste de UI/harness: validar que banner aparece quando `update_available` setado (se houver seam); caso contrário, cobrir lógica em unit tests e smoke manual.
- `cargo test` e `cargo check`.

## Workstream B — Canais de pacote

### B1. Nomenclatura e verificação de colisão

- **Preferencial**: `rustify` em todos os canais.
- **Fallback**: somente se nome já ocupado no canal alvo; usar nome alternativo consistente, ex. `rustify-player-bin` (AUR) / `Rustify.Player` (Winget id) / cask `rustify-player` conforme convenção. Registrar decisão por canal no PR/docs.
- Antes de publicar: verificar manualmente colisão (`aur.archlinux.org`, `brew search`, `winget search`). Automatizar check em CI como passo não destrutivo.

### B2. AUR (primeira onda)

**Arquivos novos**: `packaging/aur/PKGBUILD.tmpl` (ou `packaging/aur/rustify-bin/PKGBUILD`), `packaging/aur/.SRCINFO.tmpl` opcional, docs.

- Criar pacote `-bin` usando artefato oficial `rustify-linux-x86_64.tar.gz` da release.
- PKGBUILD: `pkgname=rustify-bin` ou `rustify` conforme disponibilidade; `provides/conflicts` para nome alternativo; instala binário, desktop, ícones, licenças. `depends=('libmpv' 'gcc-libs' ...)` conforme bundle; validar com `ldd` do release.
- CI: job `publish-aur` no `release.yml` (ou workflow separado) acionado em tag `v*` após release publicada. Passos:
  - baixar checksums/asset
  - renderizar PKGBUILD com versão/sha256
  - gerar `.SRCINFO` via `makepkg --printsrcinfo`
  - push para AUR via SSH (`AUR_SSH_KEY` secret)
- Não bloquear publicação da release GitHub se AUR falhar; marcar job como aviso ou permitir retry.

### B3. Homebrew cask (segunda onda)

**Arquivos novos**: `packaging/homebrew/Casks/rustify.rb.tmpl` ou workflow que atualiza tap externo.

- Preferir tap próprio: `1arley/homebrew-tap`. CI abre PR/commit no tap com nova versão/sha256.
- Cask instala app/bin conforme artefato macOS; nome `rustify` se disponível.
- Segredo: token com permissão mínima para tap repo.

### B4. Winget (terceira onda)

**Arquivos novos**: `packaging/winget/` templates/manifests ou uso de `wingetcreate`.

- Usar `wingetcreate update` ou gerar manifests YAML no CI e abrir PR para `microsoft/winget-pkgs`.
- Identificador preferencial: `1arley.Rustify` ou conforme disponibilidade; nome exibido `Rustify`.
- Segredo: `WINGET_GITHUB_TOKEN` para fork/PR.

### B5. Documentação e release

**Arquivos**: `README.md`, `README.pt-BR.md`, `scripts/install.sh` (já tem `--symlink`), `.github/workflows/release.yml`.

- Atualizar README com canais: Release/manual, AUR, Homebrew, Winget, cargo install.
- Adicionar seção “Atualizar”: pelo canal de pacote ou baixando release novo; app mostra aviso quando há nova versão.
- Release checklist: verificar nomes, checksums, manifests, AUR push.

## Sequência sugerida

1. **A1-A2**: lógica de update-check + persistência + banner/ação manual.
2. **A3**: testes unitários e smoke.
3. **B1-B2**: AUR `-bin` + automação CI (maior valor para público atual).
4. **B3**: Homebrew tap/cask.
5. **B4**: Winget manifests/PR.
6. **B5**: docs e verificação final.

## Verificação

- `cargo check`
- `cargo test`
- `cargo test ui_tests` (harness)
- Smoke manual: simular `latest` maior, dismiss, cache 24h, opt-out, ação manual, abrir release.
- CI dry-run: validar render de PKGBUILD/manifests com versão de teste sem publicar.

## Riscos / cuidados

- Não transformar aviso em auto-update/download silencioso.
- Falha de rede/GitHub não pode afetar startup ou playback.
- Publicação em pacotes não deve bloquear release binária.
- Verificar colisão de nome antes do primeiro publish; documentar fallback.
- Respeitar privacidade: apenas request público de release; sem dados do usuário.