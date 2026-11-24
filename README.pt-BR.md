<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="public/rustify-logo-dark-transparent.svg">
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

## Use agora

| Canal | Comando |
| --- | --- |
| Arch / Omarchy (AUR) | `yay -S rustify-bin` ou `paru -S rustify-bin` |
| macOS (Homebrew) | `brew install --cask 1arley/tap/rustify` |
| Windows (Winget) | `winget install 1arley.Rustify` |
| Qualquer plataforma | Baixe em [Releases](https://github.com/1arley/rustify/releases) (instruções abaixo) |
| Quem usa Rust | `cargo install rustify-player` |

> Os canais de pacote são publicados automaticamente após cada release e podem levar um tempo para aparecer. Veja [packaging/README.md](packaging/README.md).

### Linux (incluindo Omarchy / Arch)

```sh
# 1. Baixe a release Linux mais recente, extraia
# 2. Rode:
cd rustify-linux-x86_64
./install.sh
```

Abra **Rustify** no menu de aplicativos. Pronto.

> A release Linux inclui `yt-dlp` e a maioria das bibliotecas do `libmpv`. Se não abrir na sua distro, instale: `sudo pacman -S mpv`.

### Ubuntu / Debian

```sh
# Baixe e extraia a release Linux, depois:
cd rustify-linux-x86_64
./install.sh
```

Se não abrir, instale: `sudo apt install libmpv2`

### Fedora

```sh
# Baixe e extraia a release Linux, depois:
cd rustify-linux-x86_64
./install.sh
```

Se não abrir, instale: `sudo dnf install mpv-libs`

### macOS

```sh
brew install mpv
# Baixe e extraia a release macOS, depois:
./rustify
```

### Windows

1. Baixe a release do Windows em [Releases](https://github.com/1arley/rustify/releases).
2. Extraia o zip.
3. Clique duas vezes em `rustify.exe`.

> `libmpv` e `yt-dlp` já vêm incluídos na release do Windows. Não precisa instalar nada extra.

## Se quiser só rodar sem instalar

Baixe a release da sua plataforma em [Releases](https://github.com/1arley/rustify/releases), extraia e execute o binário `rustify` dentro da pasta. Mantenha o `yt-dlp` na mesma pasta do `rustify`.

## Compilar a partir do código (desenvolvedores)

```sh
# Instale os arquivos de desenvolvimento do libmpv + yt-dlp no seu sistema, depois:
cargo build --release
./target/release/rustify
```

Ou instale pelo crates.io (mesmas dependências de sistema necessárias):

```sh
cargo install rustify-player
```

Para adicionar a entrada/ícone no menu a partir do código:

```sh
./scripts/install.sh              # copia o binário
./scripts/install.sh --symlink    # cria um link para target/release/rustify (útil em builds de desenvolvimento)
```

## Atualizar

- **Gerenciador de pacotes:** `yay -Syu`, `brew upgrade --cask rustify` ou `winget upgrade 1arley.Rustify`.
- **Arquivo da release:** baixe o novo arquivo em [Releases](https://github.com/1arley/rustify/releases) e rode `./install.sh` de novo (ou substitua a pasta).
- **No app:** o Rustify consulta a última release pública no GitHub ao abrir (no máximo uma vez a cada 24 horas) e mostra um banner dispensável quando existe uma versão mais nova. Ele nunca baixa nem instala nada sozinho. Em **Biblioteca → Sobre e atualizações** (ou a versão no rodapé da barra lateral) você verifica manualmente, vê a versão atual ou desliga a verificação.

## O que você precisa

| Coisa | Precisa instalar? |
| --- | --- |
| `libmpv` | **Windows:** não (incluso). **Linux:** normalmente não (incluso), instale se necessário. **macOS:** sim (`brew install mpv`). |
| `yt-dlp` | **Não**, já vem no pacote da release. |
| Rust / Cargo | Só se for compilar do código. |

## Por que o Rustify

- **Sem conta obrigatória.** Pesquise no Audius e no YouTube e comece a ouvir na hora.
- **Traga sua biblioteca.** Importe a partir de links do Spotify, exportações de dados ou sincronização opcional de conta.
- **Nativo e leve.** App em Rust com GPUI na interface e libmpv na reprodução, sem web view.
- **Local-first.** Playlists e faixas curtidas ficam na sua máquina e armazenam apenas metadados.

> [!NOTE]
> O Rustify não reproduz streams do Spotify. Ele usa os metadados do Spotify para importar a biblioteca e procura correspondências no YouTube para reproduzir essas faixas. A disponibilidade depende de catálogos e serviços externos.

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

## Importando sua música

Abra **Biblioteca → Importar**. Você pode:

- Colar o **link de uma playlist do YouTube** para salvar seus metadados sem baixar áudio.
- Colar um **link ou URI público do Spotify**.
- Escolher **arquivos exportados** do seu computador.
- Informar um **Client ID do Spotify Developer Dashboard** para sincronizar uma conta (opcional).

Exportações suportadas: `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, pastas Spotify Account Data/MyData e arquivos CSV do Exportify.

## Dados locais e privacidade

O Rustify armazena seus dados nas pastas de configuração e dados locais do sistema operacional, resolvidas por meio de `dirs`. No Linux, respeita `XDG_CONFIG_HOME` e `XDG_DATA_HOME`; Windows e macOS usam as pastas padrão do usuário.

Consulte [PRIVACY.md](PRIVACY.md) para nomes dos arquivos, retenção, sincronização e remoção.

## Estado das plataformas

| Plataforma | Status |
| --- | --- |
| Linux x86_64 | Exercitado localmente |
| Windows x86_64 | Job nativo de CI configurado; primeira execução e teste interativo de áudio pendentes |
| macOS arm64 | Job nativo de CI configurado; primeira execução e teste interativo de áudio pendentes |

## Estrutura do código

| Caminho | Responsabilidade |
| --- | --- |
| `src/app.rs` | Janela, navegação e telas da aplicação |
| `src/catalog.rs` | Busca Audius/YouTube e resolução de faixas importadas |
| `src/playback.rs`, `src/queue.rs` | Reprodução via libmpv e fila |
| `src/library.rs` | Persistência local da biblioteca |
| `src/update.rs` | Aviso passivo de atualização (última release no GitHub, cache de 24 h) |
| `src/spotify/` | Integração de conta, API e importação de dados do Spotify |
## Licença

[MIT](LICENSE)
