<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="public/rustify-logo-dark-transparent.svg">
    <img src="public/rustify-logo.svg" alt="Rustify" width="320">
  </picture>
</p>

<p align="center">
  A native desktop music player built with Rust, GPUI, and libmpv.<br>
  Search, stream, and organize music, with or without a Spotify account.
</p>

<p align="center">
  <a href="README.md">English</a> ·
  <a href="README.pt-BR.md">Português (Brasil)</a>
</p>

## Use it now

| Channel | Command |
| --- | --- |
| Arch / Omarchy (AUR) | `yay -S rustify-bin` or `paru -S rustify-bin` |
| macOS (Homebrew) | `brew install --cask 1arley/tap/rustify` |
| Windows (Winget) | `winget install 1arley.Rustify` |
| Any platform | Download from [Releases](https://github.com/1arley/rustify/releases) (instructions below) |
| Rust users | `cargo install rustify-player` |

> Package channels are published automatically after each release and may take a little while to appear. See [packaging/README.md](packaging/README.md).

### Linux (including Omarchy / Arch)

```sh
# 1. Download the latest Linux release, extract it
# 2. Run:
cd rustify-linux-x86_64
./install.sh
```

Open **Rustify** from your app menu. Done.

> The Linux release bundles `yt-dlp` and most `libmpv` libraries. If it fails to start on your distro, install mpv (`sudo pacman -S mpv` on Arch).

### Ubuntu / Debian

```sh
# Download and extract the Linux release, then:
cd rustify-linux-x86_64
./install.sh
```

If it fails to start, install: `sudo apt install libmpv2`

### Fedora

```sh
# Download and extract the Linux release, then:
cd rustify-linux-x86_64
./install.sh
```

If it fails to start, install: `sudo dnf install mpv-libs`

### macOS

```sh
brew install mpv
# Download and extract the macOS release, then:
./rustify
```

### Windows

1. Download the Windows release from [Releases](https://github.com/1arley/rustify/releases).
2. Extract the zip.
3. Double-click `rustify.exe`.

> `libmpv` and `yt-dlp` are bundled in the Windows release. No extra installs needed.

## If you just want to run it without installing

Download the release for your platform from [Releases](https://github.com/1arley/rustify/releases), extract, and run the `rustify` binary inside the folder. Keep `yt-dlp` in the same folder as `rustify`.

## Build from source (developers)

```sh
# Install libmpv dev files + yt-dlp for your OS, then:
cargo build --release
./target/release/rustify
```

Or install from crates.io (same system dependencies required):

```sh
cargo install rustify-player
```

To add the menu entry/icon from a source checkout:

```sh
./scripts/install.sh              # copy the binary
./scripts/install.sh --symlink    # link target/release/rustify instead (handy for dev builds)
```

## Updating

- **Package manager:** `yay -Syu`, `brew upgrade --cask rustify` or `winget upgrade 1arley.Rustify`.
- **Release archive:** download the new archive from [Releases](https://github.com/1arley/rustify/releases) and run `./install.sh` again (or replace the folder).
- **In the app:** Rustify checks the latest public GitHub release at startup (at most once every 24 hours) and shows a dismissible banner when a newer version exists. It never downloads or installs anything by itself. Use **Library → Import → Sobre e atualizações** to check manually, see the current version, or turn the check off.

## What you need

| Thing | Do you need to install it? |
| --- | --- |
| `libmpv` | **Windows:** no (bundled). **Linux:** usually no (bundled), install if needed. **macOS:** yes (`brew install mpv`). |
| `yt-dlp` | **No**, bundled in release packages. |
| Rust / Cargo | Only if building from source. |

## Why Rustify

- **No account required.** Search Audius and YouTube and start listening right away.
- **Bring your library.** Import from Spotify links, data exports, or an optional account sync.
- **Native and lightweight.** A Rust app with GPUI for the interface and libmpv for playback, not a web view.
- **Local-first.** Playlists and liked tracks live on your machine and store metadata only.

> [!NOTE]
> Rustify does not play Spotify streams. It uses Spotify metadata to import your library and looks for matching tracks on YouTube for playback. Availability depends on external catalogs and services.

## Features

**Listen**

- Search Audius and YouTube without connecting a Spotify account.
- Integrated player with queue, play/pause, seeking, volume, shuffle, and repeat.
- Restores the last track and position on launch, paused, so playback starts when you choose.

**Organize**

- Create local playlists and add tracks straight from search results.
- Import YouTube playlists as metadata, with no audio downloaded.
- Liked tracks store metadata only; audio streams when playback begins.

**Import**

- Public Spotify links and URIs, JSON/CSV exports, or optional account sync via OAuth PKCE.
- Local library and account-sync snapshots are kept separate, with an explicit cleanup action for identifiable legacy imports.

**Interface**

- Responsive desktop layout with compact navigation and a mini-player for narrow windows.

## Importing your music

Open **Library → Import**. You can:

- Paste a **YouTube playlist link** to save its metadata without downloading audio.
- Paste a **public Spotify link or URI**.
- Choose **exported files** from your computer.
- Enter a **Spotify Developer Dashboard Client ID** to sync an account (optional).

Supported exports: `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, Spotify Account Data/MyData folders, and Exportify CSV files.

## Local data and privacy

Rustify stores its data in the operating system's user config and local data directories, resolved through `dirs`. Linux respects `XDG_CONFIG_HOME` and `XDG_DATA_HOME`; Windows and macOS use their standard user folders.

See [PRIVACY.md](PRIVACY.md) for filenames, retention, sync, and removal behavior.

## Platform status

| Platform | Status |
| --- | --- |
| Linux x86_64 | Exercised locally |
| Windows x86_64 | Native CI job configured; first run and interactive audio check pending |
| macOS arm64 | Native CI job configured; first run and interactive audio check pending |

## Code layout

| Path | Responsibility |
| --- | --- |
| `src/app.rs` | Application window, navigation, and screens |
| `src/catalog.rs` | Audius/YouTube search and imported-track matching |
| `src/playback.rs`, `src/queue.rs` | libmpv playback and queue |
| `src/library.rs` | Local library storage |
| `src/update.rs` | Passive update notice (latest GitHub release, 24 h cache) |
| `src/spotify/` | Spotify account integration, API, and data import |
## License

[MIT](LICENSE)
