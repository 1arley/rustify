<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="public/rustify-logo-dark.svg">
    <img src="public/rustify-logo.svg" alt="Rustify" width="320">
  </picture>
</p>

<p align="center">
  Player de música desktop nativo em Rust com GPUI e libmpv.<br>
  Pesquise, ouça e organize músicas, com ou sem conta Spotify.
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="README.pt-BR.md">Português (Brasil)</a>
</p>

<!-- Adicione uma captura ou GIF curto aqui, por exemplo:
<p align="center"><img src="public/screenshot.png" alt="Janela principal do Rustify" width="800"></p>
-->

## Por que o Rustify

- **Sem conta obrigatória.** Pesquise no Audius e no YouTube e comece a ouvir na hora.
- **Traga sua biblioteca.** Importe a partir de links do Spotify, exportações de dados ou sincronização opcional de conta.
- **Nativo e leve.** App em Rust com GPUI na interface e libmpv na reprodução, sem web view.
- **Local-first.** Playlists e faixas curtidas ficam na sua máquina e armazenam apenas metadados.

> [!NOTE]
> O Rustify não reproduz streams do Spotify. Ele usa os metadados do Spotify para importar a biblioteca e procura correspondências no YouTube para reproduzir essas faixas. A disponibilidade depende de catálogos e serviços externos.

## Conteúdo

- [Funcionalidades](#funcionalidades)
- [Início rápido](#início-rápido)
- [Requisitos](#requisitos)
- [Estado das plataformas](#estado-das-plataformas)
- [Importando sua música](#importando-sua-música)
- [Layout responsivo](#layout-responsivo)
- [Dados locais e privacidade](#dados-locais-e-privacidade)
- [Estrutura do código](#estrutura-do-código)

## Funcionalidades

**Ouvir**

- Busca no Audius e no YouTube sem conectar uma conta Spotify.
- Player integrado com fila e controles de play/pause, avanço na faixa, volume, shuffle e repetição.
- Restaura a última faixa e posição ao abrir, mantendo a reprodução pausada para você escolher quando tocar.

**Organizar**

- Criação de playlists locais e adição de faixas direto dos resultados da busca.
- Importação de playlists do YouTube só com metadados, sem baixar áudio.
- Curtidas guardam apenas metadados; o áudio começa a tocar quando a reprodução inicia.

**Importar**

- Links e URIs públicos do Spotify, exportações JSON/CSV ou sincronização opcional de conta via OAuth PKCE.
- Snapshot da conta separado da biblioteca local, com ação explícita para limpar imports antigos identificáveis.

**Interface**

- Layout responsivo com navegação compacta e mini-player em janelas estreitas.

## Início rápido

1. Instale os [requisitos](#requisitos) do seu sistema (`libmpv` e `yt-dlp`).
2. Clone o repositório e execute:

   ```sh
   cargo run
   ```

3. Pesquise uma faixa ou abra **Biblioteca → Importar** para trazer sua música.

## Requisitos

| Requisito | Por que é necessário |
| --- | --- |
| Rust estável com suporte à edition 2024 | Compilar o app |
| `libmpv` e codecs de áudio | Reprodução |
| [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) no `PATH` | Busca, importação de playlists do YouTube e correspondência de faixas importadas do Spotify |
| Acesso à internet | Consultar catálogos e reproduzir streams |

Para compilar localmente você também precisa dos **arquivos de desenvolvimento** do `libmpv` para o seu sistema:

| Sistema | Configuração |
| --- | --- |
| Linux | Instale o pacote de desenvolvimento `libmpv` da sua distribuição. |
| macOS | Instale `mpv` com o Homebrew. |
| Windows | O caminho de CI usa o target Rust `x86_64-pc-windows-gnu` com pacotes MSYS2 UCRT64. |

No Linux, a seleção de arquivos usa o portal de desktop XDG e recorre ao `zenity` se o portal não abrir o diálogo.

## Estado das plataformas

| Plataforma | Status |
| --- | --- |
| Linux x86_64 | Exercitado localmente |
| Windows x86_64 | Job nativo de CI configurado; primeira execução e teste interativo de áudio pendentes |
| macOS arm64 | Job nativo de CI configurado; primeira execução e teste interativo de áudio pendentes |

Os builds exigem `libmpv` nativo. O formato de distribuição e o empacotamento de codecs ainda não foram definidos.

## Importando sua música

Abra a tela de importação a partir da biblioteca. Você pode:

- Colar o **link de uma playlist do YouTube** para salvar seus metadados sem baixar áudio.
- Colar um **link ou URI público do Spotify**.
- Escolher **arquivos exportados** do seu computador.
- Informar um **Client ID do Spotify Developer Dashboard** para sincronizar uma conta (opcional).

Exportações suportadas:

| Origem | Arquivos |
| --- | --- |
| Exportação de dados do Spotify | `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json` ou a pasta Account Data / MyData |
| Exportify | Arquivos CSV |

## Layout responsivo

A navegação principal reúne **Início**, **Buscar** e **Biblioteca**. A importação fica na biblioteca; fila e Tocando agora ficam acessíveis pelo player.

| Largura da janela | Comportamento |
| --- | --- |
| 1280 px ou mais | Painel Tocando agora pode ser exibido ou recolhido |
| 1040 px ou mais | Controles completos do player |
| Abaixo de 1040 px | Player compacto |
| 900 px ou menos | Navegação lateral compacta |
| 620 px ou menos | Navegação vai para a parte inferior |

O layout foi conferido visualmente em 1440×900, 1280×800, 768×1024 e 390×844 com dados de demonstração.

## Dados locais e privacidade

O Rustify armazena seus dados nas pastas de configuração e dados locais do sistema operacional, resolvidas por meio de `dirs`. No Linux, respeita `XDG_CONFIG_HOME` e `XDG_DATA_HOME`; Windows e macOS usam as pastas padrão do usuário.

Consulte [PRIVACY.md](PRIVACY.md) para nomes dos arquivos, retenção, sincronização e remoção.

## Estrutura do código

| Caminho | Responsabilidade |
| --- | --- |
| `src/app.rs` | Janela, navegação e telas da aplicação |
| `src/catalog.rs` | Busca Audius/YouTube e resolução de faixas importadas |
| `src/playback.rs`, `src/queue.rs` | Reprodução via libmpv e fila |
| `src/library.rs` | Persistência local da biblioteca |
| `src/spotify/` | Integração de conta, API e importação de dados do Spotify |