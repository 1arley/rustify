# Plano de prontidão do Rustify

**Execução prevista:** 2026-11-23

**Fluxo:** espiral orientada por riscos, entregue em etapas incrementais. Começar pela propriedade dos dados e pela viabilidade de `libmpv` nos sistemas-alvo.

## Escopo confirmado

- Não trabalhar no item 1 da avaliação anterior.
- Resolver o ciclo de dados da sincronização OAuth do Spotify.
- Preparar suporte desktop para Linux, Windows e macOS.
- Validar os fluxos reais do player e alinhar os documentos ao código.
- Adaptar o layout a janelas desktop e janelas estreitas com largura de celular.
- Usar as duas imagens enviadas como referência estrutural de desktop e celular. Não versionar as imagens nem reutilizar conteúdo pessoal ou assets delas.

**Interpretação para a etapa responsiva:** “celular” significa uma janela estreita de cerca de 390 px no app desktop. Este plano não inclui binários Android ou iOS.

## Sequência de execução

### 1. Registrar o estado inicial

- Rodar `cargo fmt --check`, `cargo check` e `cargo test` no Linux.
- Anotar dependências nativas presentes e resultado de cada comando antes de alterar o código.
- Fazer um smoke check da busca, reprodução, importação e abertura da fila na versão atual.

**Aceite:** há um resultado reproduzível de baseline; falhas preexistentes ficam registradas e separadas das mudanças do plano.

### 2. Isolar dados sincronizados da biblioteca local

Hoje a sincronização OAuth mistura curtidas e playlists com arquivos e links importados manualmente; a origem se perde. O novo fluxo deve manter o snapshot da conta separado e combiná-lo com a biblioteca local somente para apresentação.

- Gravar o snapshot da conta separadamente e substituí-lo atomicamente somente após uma sincronização completa.
- Ao desconectar, remover o snapshot e os vínculos de faixa pertencentes à conta, além do token OAuth; preservar curtidas locais e imports de arquivos/links.
- Em falha de sincronização, manter o snapshot anterior. Em falha de remoção, mostrar erro claro e não informar que os dados foram apagados.
- Para dados de versões anteriores, cuja origem não pode ser reconstruída com segurança, preservar por padrão e oferecer uma ação explícita para remover itens antigos ligados ao Spotify.
- Criar `PRIVACY.md` com scopes, dados lidos, arquivos locais usados, retenção, desconexão e remoção de dados.
- Conferir o fluxo contra a [Spotify Developer Policy](https://developer.spotify.com/policy), sem declarar conformidade legal como resultado automático dos testes.

**Aceite:** testes cobrem sincronização repetida, falha sem perda do snapshot anterior, desconexão que apaga dados da conta e preserva biblioteca manual, e tratamento explícito dos dados legados.

### 3. Definir a matriz de sistemas e remover dependências Linux-only

- Confirmar arquitetura dos builds antes de configurar CI. Matriz inicial proposta: Linux x86_64, Windows x86_64 e macOS arm64; adicionar Linux arm64 ou macOS x86_64 se forem requisitos do lançamento.
- Substituir `zenity`/`kdialog` pelo seletor nativo multiplataforma.
- Substituir `xdg-open` por abertura de URL multiplataforma.
- Resolver diretórios de configuração, dados e Downloads por convenções de cada sistema, mantendo XDG no Linux.
- Preferir as crates `open` e `dirs` já presentes na árvore de dependências como dependências diretas; avaliar `rfd` para o seletor nativo, sem criar uma abstração própria.
- Fornecer `libmpv` e codecs em build e runtime para cada pacote; escolher um formato de distribuição por sistema após validar o custo de empacotamento.
- Adicionar CI nativa para Linux, Windows e macOS. Uma plataforma só é anunciada como suportada depois de compilar, testar, abrir a interface e reproduzir áudio em runner nativo.

**Aceite:** caminhos de dados, seletor de arquivo, abertura do navegador e carregamento de libmpv funcionam na matriz escolhida; cada plataforma tem instruções ou pacote reproduzível.

### 4. Tornar a interface responsiva

Usar a imagem desktop como referência para sidebar, conteúdo central, painel Now Playing à direita e controles fixos do player. Usar a imagem de celular como referência para filtros compactos, atalhos em duas colunas, prateleiras horizontais, mini-player e navegação inferior.

| Janela de validação | Comportamento esperado |
|---|---|
| 1920 × 1080 | Sidebar, conteúdo e painel Now Playing podem coexistir; player inferior completo |
| 1280 × 800 | Conteúdo mantém largura útil; painel lateral pode recolher |
| 768 × 1024 | Navegação lateral compacta; conteúdo e controles sem sobreposição |
| 390 × 844 | Sem sidebar permanente; navegação inferior, atalhos em duas colunas, cards/prateleiras compactos e mini-player acima da navegação |

- Definir breakpoints a partir de conteúdo mínimo, não somente de valores medidos na captura.
- Converter o painel Now Playing em painel/tela acessível pela navegação na janela estreita.
- Garantir que busca, reprodução, fila e importação continuem acessíveis em todas as larguras.
- Evitar rolagem horizontal acidental, texto sobreposto e controles pequenos; revisar teclado, foco e alvos de toque na largura de celular.
- Usar dados de demonstração para capturas e revisão visual, sem persistir conteúdo pessoal das imagens de referência.

**Aceite:** capturas das quatro larguras mostram layout legível e sem clipping; os fluxos principais continuam operáveis em 390 px e 768 px.

### 5. Validar o produto na matriz

**Testes automatizados:** manter testes locais determinísticos no CI; manter testes que exigem serviços externos separados e explicitamente opt-in.

**Fluxos manuais por plataforma:**

- Busca e reprodução Audius e YouTube.
- Importação de link Spotify, arquivo JSON/CSV e sincronização OAuth.
- Atualização, desconexão e remoção dos dados sincronizados.
- Play/pause/retomar, seek, volume, próxima/anterior, edição de fila, shuffle, repeat e EOF.
- Reinício e restauração da biblioteca local; falhas de rede, token expirado, arquivo inválido, yt-dlp ausente e libmpv/codec ausente.
- Redimensionamento progressivo entre as quatro larguras da etapa responsiva.

**Aceite:** suíte local passa; Linux tem smoke check de interface e áudio; Windows e macOS têm smoke check em runners nativos. Integrações indisponíveis ficam registradas como não verificadas, sem declarar suporte para elas.

### 6. Reconciliar a documentação

Atualizar os documentos depois de validar o código, distinguindo **implementado**, **testado** e **ainda não verificado**:

- `implementation-roadmap.md`
- `implementation-architecture.md`
- `interaction-matrix.md`
- `player-state-machine.md`
- `api-capability-matrix.md`
- `environment-audit.md`
- `technical-findings.md`
- `visual-design-spec.md`
- `README.md` e `README.pt-BR.md`

Manter `spotify-screen-inventory.md` como referência histórica. Remover afirmações antigas de que Spotify está inativo ou que a fila editável não existe. Registrar a matriz de sistemas, diretórios de dados, fluxo de exclusão e breakpoints efetivamente validados.

**Aceite:** nenhum documento de implementação contradiz `main.rs` ou a UI atual; cada item de suporte tem evidência associada.

## Gate de beta

O beta está pronto quando o ciclo OAuth respeita a exclusão de dados sem apagar a biblioteca manual, os fluxos principais passam no Linux, Windows e macOS, os quatro tamanhos de janela são operáveis, e os documentos refletem os resultados reais. Se um runner ou pacote nativo não estiver disponível amanhã, registrar esse sistema como pendente em vez de declarar suporte.

## Riscos e limites do plano

- `libmpv` e codecs são a maior dependência para builds e pacotes nos três sistemas.
- Dados antigos foram mesclados sem rastreamento de origem; a remoção precisa de uma ação explícita para evitar apagar imports manuais.
- A largura de celular neste plano é responsividade da janela desktop, não suporte a Android/iOS.
