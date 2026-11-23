# Rustify

[English](README.md) | [Português (Brasil)](README.pt-BR.md)

Player de música desktop em Rust com GPUI e libmpv. Pesquise faixas no Audius ou no YouTube e ouça pelo player integrado; importe também playlists e biblioteca do Spotify.

## Funcionalidades

- Busca no Audius e no YouTube sem conectar uma conta Spotify.
- Player integrado com fila e controles de play/pause, avanço na faixa, volume, shuffle e repetição.
- Criação de playlists locais, adição de faixas pela busca e importação de playlists do YouTube só com metadados.
- Curtidas guardam apenas metadados; o áudio começa a tocar enquanto o player recebe o stream.
- Restauração da última faixa e posição ao abrir, mantendo a reprodução pausada.
- Importação de links e URIs públicos do Spotify, arquivos JSON/CSV ou sincronização opcional de conta Spotify com OAuth PKCE.
- Snapshot da conta separado da biblioteca local, com ação explícita para limpar imports antigos identificáveis.
- Layout responsivo com navegação compacta e mini-player em janelas estreitas.

> [!NOTE]
> O Rustify não reproduz streams do Spotify. Ele usa os metadados do Spotify para importar a biblioteca e procura correspondências no YouTube para reproduzir essas faixas. A disponibilidade depende de catálogos e serviços externos.

## Requisitos

- Rust estável com suporte à edition 2024.
- `libmpv` e codecs de áudio instalados no sistema.
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) no `PATH` para busca, importação de playlists do YouTube e correspondência de faixas importadas do Spotify.
- Acesso à internet para consultar os catálogos e reproduzir streams.

## Estado das plataformas

Linux x86_64 é a única plataforma exercitada localmente até agora. Há jobs nativos de CI configurados para Linux x86_64, Windows x86_64 e macOS arm64, mas a primeira execução deles e os testes interativos de áudio ainda estão pendentes. Os builds exigem `libmpv` nativo; o formato de distribuição e o empacotamento de codecs ainda não foram definidos.

Para compilar localmente, instale `libmpv` e seus arquivos de desenvolvimento para o sistema. O caminho de CI do Windows usa o target Rust `x86_64-pc-windows-gnu` com pacotes MSYS2 UCRT64. No macOS, o Homebrew fornece `mpv`; no Linux, use o pacote de desenvolvimento `libmpv` da distribuição. A seleção de arquivos no Linux usa o portal de desktop XDG e recorre ao `zenity` se o portal não abrir o diálogo.

## Executar

```sh
cargo run
```

Na tela de importação, cole o link de uma playlist do YouTube para salvar até 500 faixas sem baixar áudio. Você também pode colar um link ou URI público do Spotify, selecionar arquivos exportados ou informar um Client ID do Spotify Developer Dashboard para sincronizar uma conta. São aceitos `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, pastas Spotify Account Data/MyData e CSVs do Exportify. A sincronização é opcional.

## Dados locais

O app usa as pastas de configuração e dados locais do sistema operacional por meio de `dirs`. No Linux, respeita `XDG_CONFIG_HOME` e `XDG_DATA_HOME`; Windows e macOS usam as pastas padrão do usuário. Consulte [PRIVACY.md](PRIVACY.md) para nomes dos arquivos, retenção, sincronização e remoção.

## Layout responsivo

A navegação principal reúne Início, Buscar e Biblioteca. A importação fica na biblioteca; fila e Tocando agora ficam acessíveis pelo player. Em janelas de 1280 px ou mais, o painel Tocando agora pode ser exibido ou recolhido. Os controles completos do player aparecem a partir de 1040 px; abaixo disso o player fica compacto. Em 900 px ou menos a navegação lateral fica compacta; em 620 px ou menos a navegação vai para a parte inferior. O layout foi conferido visualmente em 1440×900, 1280×800, 768×1024 e 390×844 com dados de demonstração.

## Estrutura do código

- `src/app.rs`: janela, navegação e telas da aplicação.
- `src/catalog.rs`: busca Audius/YouTube e resolução de faixas importadas.
- `src/playback.rs` e `src/queue.rs`: reprodução via libmpv e fila.
- `src/library.rs`: persistência local da biblioteca.
- `src/spotify/`: integração de conta, API e importação de dados do Spotify.
