<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="public/rustify-logo-dark.svg">
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

<!-- Add a screenshot or short GIF here, e.g.:
<p align="center"><img src="public/screenshot.png" alt="Rustify main window" width="800"></p>
-->

## Why Rustify

- **No account required.** Search Audius and YouTube and start listening right away.
- **Bring your library.** Import from Spotify links, data exports, or an optional account sync.
- **Native and lightweight.** A Rust app with GPUI for the interface and libmpv for playback, not a web view.
- **Local-first.** Playlists and liked tracks live on your machine and store metadata only.

> [!NOTE]
> Rustify does not play Spotify streams. It uses Spotify metadata to import your library and looks for matching tracks on YouTube for playback. Availability depends on external catalogs and services.

## Contents

- [Features](#features)
- [Quick start](#quick-start)
- [Requirements](#requirements)
- [Platform status](#platform-status)
- [Importing your music](#importing-your-music)
- [Responsive layout](#responsive-layout)
- [Local data and privacy](#local-data-and-privacy)
- [Code layout](#code-layout)

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

## Quick start

1. Install the [requirements](#requirements) for your OS (`libmpv` and `yt-dlp`).
2. Clone the repository and run it:

   ```sh
   cargo run
   ```

3. Search for a track, or open **Library → Import** to bring in your music.

## Requirements

| Requirement | Why it is needed |
| --- | --- |
| Stable Rust with edition 2024 support | Building the app |
| `libmpv` and audio codecs | Playback |
| [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) on your `PATH` | YouTube search, playlist import, and matching imported Spotify tracks |
| Internet access | Querying catalogs and playing streams |

For local builds you also need the `libmpv` **development files** for your OS:

| OS | Setup |
| --- | --- |
| Linux | Install your distribution's `libmpv` development package. |
| macOS | Install `mpv` with Homebrew. |
| Windows | The CI path uses the `x86_64-pc-windows-gnu` Rust target with MSYS2 UCRT64 packages. |

On Linux, file selection uses the XDG desktop portal and falls back to `zenity` when the portal cannot open a dialog.

## Platform status

| Platform | Status |
| --- | --- |
| Linux x86_64 | Exercised locally |
| Windows x86_64 | Native CI job configured; first run and interactive audio check pending |
| macOS arm64 | Native CI job configured; first run and interactive audio check pending |

Builds require a native `libmpv`. The distribution format and bundled codecs have not been selected yet.

## Importing your music

Open the import screen from the library. You can:

- Paste a **YouTube playlist link** to save its metadata without downloading audio.
- Paste a **public Spotify link or URI**.
- Choose **exported files** from your computer.
- Enter a **Spotify Developer Dashboard Client ID** to sync an account (optional).

Supported exports:

| Source | Files |
| --- | --- |
| Spotify data export | `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, or the Account Data / MyData folder |
| Exportify | CSV files |

## Responsive layout

The main navigation is **Home**, **Search**, and **Library**. Import lives in the library; the queue and Now Playing are available from the player.

| Window width | Behavior |
| --- | --- |
| 1280 px and wider | Now Playing side panel can be shown or hidden |
| 1040 px and wider | Full player controls |
| Below 1040 px | Compact player |
| 900 px and below | Narrower sidebar |
| 620 px and below | Navigation moves to the bottom |

The layout was visually checked at 1440×900, 1280×800, 768×1024, and 390×844 with demo data.

## Local data and privacy

Rustify stores its data in the operating system's user config and local data directories, resolved through `dirs`. Linux respects `XDG_CONFIG_HOME` and `XDG_DATA_HOME`; Windows and macOS use their standard user folders.

See [PRIVACY.md](PRIVACY.md) for filenames, retention, sync, and removal behavior.

## Code layout

| Path | Responsibility |
| --- | --- |
| `src/app.rs` | Application window, navigation, and screens |
| `src/catalog.rs` | Audius/YouTube search and imported-track matching |
| `src/playback.rs`, `src/queue.rs` | libmpv playback and queue |
| `src/library.rs` | Local library storage |
| `src/spotify/` | Spotify account integration, API, and data import |