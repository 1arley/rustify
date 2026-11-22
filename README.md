# Rustify

[English](README.md) | [Português (Brasil)](README.pt-BR.md)

A desktop music player built with Rust, GPUI, and libmpv. Search Audius or YouTube and listen in the integrated player, or import playlists and your library from Spotify.

## Features

- Search Audius and YouTube without connecting a Spotify account.
- Integrated player with queue, play/pause, seeking, volume, shuffle, and repeat controls.
- Locally saved liked tracks and imported playlists.
- Import public Spotify links and URIs, JSON/CSV exports, or optionally sync a Spotify account with OAuth PKCE.

> [!NOTE]
> Rustify does not play Spotify streams. It uses Spotify metadata to import your library and looks for matching tracks on YouTube for playback. Availability depends on external catalogs and services.

## Requirements

- Stable Rust with edition 2024 support.
- `libmpv` and audio codecs installed on the system.
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) on your `PATH` for YouTube search and matching imported Spotify tracks.
- Internet access to query catalogs and play streams.

## Run

```sh
cargo run
```

In the import screen, paste a public Spotify link or URI, choose exported files, or enter a Spotify Developer Dashboard Client ID to sync an account. Supported exports include `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, Spotify Account Data/MyData folders, and Exportify CSV files. Account sync is optional.

## Local data

Liked tracks, imported playlists, and track matches are stored in `$XDG_DATA_HOME/rustify` (or `~/.local/share/rustify` if `XDG_DATA_HOME` is unset). Spotify account configuration and tokens are stored in `$XDG_CONFIG_HOME/rustify` (or `~/.config/rustify`).

## Code layout

- `src/app.rs`: application window, navigation, and screens.
- `src/catalog.rs`: Audius/YouTube search and imported-track matching.
- `src/playback.rs` and `src/queue.rs`: libmpv playback and queue.
- `src/library.rs`: local library storage.
- `src/spotify/`: Spotify account integration, API, and data import.
