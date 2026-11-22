# Rustify

[English](README.md) | [Português (Brasil)](README.pt-BR.md)

Player de música desktop em Rust com GPUI e libmpv. Pesquise faixas no Audius ou no YouTube e ouça pelo player integrado; importe também playlists e biblioteca do Spotify.

## Funcionalidades

- Busca no Audius e no YouTube sem conectar uma conta Spotify.
- Player integrado com fila e controles de play/pause, avanço na faixa, volume, shuffle e repetição.
- Faixas curtidas e playlists importadas salvas localmente.
- Importação de links e URIs públicos do Spotify, arquivos JSON/CSV ou sincronização opcional de conta Spotify com OAuth PKCE.

> [!NOTE]
> O Rustify não reproduz streams do Spotify. Ele usa os metadados do Spotify para importar a biblioteca e procura correspondências no YouTube para reproduzir essas faixas. A disponibilidade depende de catálogos e serviços externos.

## Requisitos

- Rust estável com suporte à edition 2024.
- `libmpv` e codecs de áudio instalados no sistema.
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) no `PATH` para busca no YouTube e correspondência de faixas importadas do Spotify.
- Acesso à internet para consultar os catálogos e reproduzir streams.

## Executar

```sh
cargo run
```

Na tela de importação, cole um link ou URI público do Spotify, selecione arquivos exportados ou informe um Client ID do Spotify Developer Dashboard para sincronizar uma conta. São aceitos `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, pastas Spotify Account Data/MyData e CSVs do Exportify. A sincronização é opcional.

## Dados locais

Curtidas, playlists importadas e correspondências de faixas ficam em `$XDG_DATA_HOME/rustify` (ou `~/.local/share/rustify` quando `XDG_DATA_HOME` não está definido). A configuração e os tokens da conta Spotify ficam em `$XDG_CONFIG_HOME/rustify` (ou `~/.config/rustify`).

## Estrutura do código

- `src/app.rs`: janela, navegação e telas da aplicação.
- `src/catalog.rs`: busca Audius/YouTube e resolução de faixas importadas.
- `src/playback.rs` e `src/queue.rs`: reprodução via libmpv e fila.
- `src/library.rs`: persistência local da biblioteca.
- `src/spotify/`: integração de conta, API e importação de dados do Spotify.
