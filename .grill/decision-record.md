# Decision Record — Rustify: atualização para usuários e canais de pacote

## Contexto

- **Data**: 2026-11-23
- **Sessão grill**: `rustify-update-plan-20261123`
- **Escopo planejado**: Opção 1 (aviso de atualização no app) e Opção 3 (canais de gerenciador de pacote).
- **Orçamento informado**: `JEV_MAX_COST_USD=5`
- **Observação de execução**: chamadas ao JEV retornaram `provider-error`; rodadas 1-2 foram concluídas via fallback `jev-subagent`. A rodada 3 atingiu `token-limit` (`usedTokens=12996`, `maxTokens=20000`); Q6/Q7 foram confirmadas pelo usuário.

## Decisões

| ID | Decisão | Status | Proveniência | Settled |
|---|---|---|---:|:---:|
| Q1 | Check passivo e assíncrono no startup, comparando `CARGO_PKG_VERSION` com a última release do GitHub; exibir banner discreto e dispensável com link para a release; falhar silenciosamente em offline/rate limit; sem bloquear UI e sem baixar binário. | accept | jev-subagent | yes |
| Q2 | Primeira onda de pacotes: AUR primeiro (aderência Arch/Omarchy), depois Homebrew cask (macOS) e Winget (Windows), com automação integrada ao fluxo de release. | accept | jev-subagent | yes |
| Q3 | Checagem habilitada por padrão, com opt-out em configuração, cache da última verificação por 24h e persistência de “dispensar esta versão”. | accept | jev-subagent | yes |
| Q4 | Usar endpoint de última release do GitHub, comparar tags semver `vX.Y.Z` com `CARGO_PKG_VERSION`, e abrir a página da release no navegador; sem download/instalação no app. | accept | jev-subagent | yes |
| Q5 | AUR como pacote `-bin` baseado no artefato oficial de release Linux; Homebrew cask e Winget atualizados por manifests gerados automaticamente no workflow de release. | accept | jev-subagent | yes |
| Q6 | Persistir estado (`enabled`, `last_checked_at`, `dismissed_version`, `latest_seen`) no caminho de config/dados existente do Rustify, com ação manual “Verificar atualizações” e versão atual visível na UI/sobre. | accept | user | yes |
| Q7 | Automatizar no CI começando por AUR `-bin` (push de `PKGBUILD`/`.SRCINFO` para repo AUR no tag de release), depois manifests/PRs para Homebrew cask e Winget. Nome preferencial `rustify`; usar nome alternativo apenas se `rustify` já estiver ocupado no canal alvo. | amend | user | yes |

## Plano resultante

### Opção 1 — Aviso de atualização no app

1. **Detecção**: no startup, disparar task assíncrona que consulta a última release do GitHub. Não bloquear renderização/UI.
2. **Comparação**: normalizar tag `vX.Y.Z` e comparar com `CARGO_PKG_VERSION` por semver. Ignorar falhas de parse/rede/rate limit silenciosamente.
3. **Cache/dismiss**: armazenar `enabled`, `last_checked_at`, `dismissed_version`, `latest_seen` no config/dados existente. Pular rede se checado há menos de 24h, salvo ação manual.
4. **UX**: banner discreto e dispensável com texto de nova versão e ação para abrir release no navegador. Incluir controle manual “Verificar atualizações” e exibir versão atual em UI/sobre/config.
5. **Privacidade/segurança**: apenas GET público à API de releases; sem download/execução automática de binário; sem telemetria adicional.

### Opção 3 — Canais de gerenciador de pacote

1. **AUR (primeiro)**: publicar pacote `-bin` preferencialmente como `rustify`; se ocupado, usar nome alternativo consistente (ex.: `rustify-player-bin`) apenas após verificação de colisão. Instalar artefato oficial Linux da release.
2. **Homebrew cask (segundo)**: gerar/atualizar manifest/cask automaticamente a partir da release.
3. **Winget (terceiro)**: gerar/atualizar manifest e abrir PR/artefato automatizado no fluxo de release.
4. **CI**: no tag `v*`, após build/release, atualizar AUR (`PKGBUILD`, `.SRCINFO`, checksums) e gerar manifests Homebrew/Winget. Credenciais/segredos de publicação devem ficar em secrets do CI, com menor privilégio.

## Assunções

- As releases do GitHub continuarão usando tags semver `vX.Y.Z`.
- `CARGO_PKG_VERSION` representa a versão instalada/executada pelo usuário.
- O caminho de config/dados existente do Rustify pode armazenar estado pequeno de update-check sem migração complexa.
- O fluxo de release no GitHub Actions pode ser estendido para gerar/publicar manifests.
- O nome `rustify` pode estar ocupado em algum canal; colisão exige fallback por canal.

## Riscos em aberto

- **Rate limit da API GitHub**: requests anônimos podem ser limitados; cache de 24h e falha silenciosa mitigam.
- **Colisão de nomes**: `rustify` pode já existir no AUR/Homebrew/Winget; definir fallback consistente e documentar.
- **Automação de publicação**: push para AUR/PRs Winget requer tokens e manutenção; falhas de CI não devem bloquear release de binários.
- **Diferenças por canal**: usuários podem atualizar por canal diferente do esperado; banner deve apontar para release, sem presumir canal.
- **Semver/prerelease**: decidir se prereleases aparecem; recomendação implícita: usar apenas última release estável.

## Proveniência e auditoria

- Registro de auditoria: `.grill/rustify-update-plan-20261123.jsonl`
- Rodadas 1-2: `jev-subagent` (fallback por `provider-error`)
- Rodada 3: `user` (após `token-limit`)