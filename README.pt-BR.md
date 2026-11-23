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

### Linux (incluindo Omarchy / Arch)

```sh
# 1. Instale o libmpv (uma vez só)
sudo pacman -S mpv

# 2. Baixe a release Linux mais recente, extraia e:
cd rustify-linux-x86_64
./install.sh
```

Abra **Rustify** no menu de aplicativos. Pronto.

### Ubuntu / Debian

```sh
sudo apt install libmpv2
# Baixe e extraia a release Linux, depois:
cd rustify-linux-x86_64
./install.sh
```

### Fedora

```sh
sudo dnf install mpv-libs
# Baixe e extraia a release Linux, depois:
cd rustify-linux-x86_64
./install.sh
```

### macOS

```sh
brew install mpv
# Baixe e extraia a release macOS, depois:
./rustify
```

### Windows

1. Instale `mpv`/`libmpv` (por enquanto, recomendado o pacote MSYS2 UCRT64).
2. Baixe e extraia a release do Windows.
3. Clique duas vezes em `rustify.exe`.

> O `yt-dlp` já vem incluído no pacote da release. Você não precisa instalar.

## Se quiser só rodar sem instalar

Baixe a release da sua plataforma em [Releases](https://github.com/1arley/rustify/releases), extraia e execute o binário `rustify` dentro da pasta. Mantenha o `yt-dlp` na mesma pasta do `rustify`.

## Compilar a partir do código (desenvolvedores)

```sh
# Instale os arquivos de desenvolvimento do libmpv + yt-dlp no seu sistema, depois:
cargo build --release
./target/release/rustify
```

Para adicionar a entrada/ícone no menu a partir do código:

```sh
./scripts/install.sh
```

## O que você precisa

| Coisa | Precisa instalar? |
| --- | --- |
| `libmpv` | **Sim**, uma vez. Comandos acima. |
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
| `src/spotify/` | Integração de conta, API e importação de dados do Spotify |