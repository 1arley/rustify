# Rustify

[English](README.md) | [Português (Brasil)](README.pt-BR.md)

A desktop music player built with Rust, GPUI, and libmpv. Search Audius or YouTube and listen in the integrated player, or import playlists and your library from Spotify.

## Features

- Search Audius and YouTube without connecting a Spotify account.
- Integrated player with queue, play/pause, seeking, volume, shuffle, and repeat controls.
- Create local playlists, add tracks from search, and import YouTube playlists as metadata.
- Liked tracks store metadata only; audio starts streaming when playback begins.
- Restore the last track and position on launch, paused so playback starts when you choose.
- Import public Spotify links and URIs, JSON/CSV exports, or optionally sync a Spotify account with OAuth PKCE.
- Separate local library and account-sync snapshots, with an explicit cleanup action for identifiable legacy imports.
- Responsive desktop layout with a compact navigation and mini-player for narrow windows.

> [!NOTE]
> Rustify does not play Spotify streams. It uses Spotify metadata to import your library and looks for matching tracks on YouTube for playback. Availability depends on external catalogs and services.

## Requirements

- Stable Rust with edition 2024 support.
- `libmpv` and audio codecs installed on the system.
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) on your `PATH` for YouTube search, playlist import, and matching imported Spotify tracks.
- Internet access to query catalogs and play streams.

## Platform status

Linux x86_64 is the only platform exercised locally so far. Native CI jobs are configured for Linux x86_64, Windows x86_64, and macOS arm64, but their first runs and interactive audio checks are still pending. Builds require a native `libmpv`; the distribution format and bundled codecs have not been selected.

For local builds, install `libmpv` and its development files for your OS. The Windows CI path uses the Rust `x86_64-pc-windows-gnu` target with MSYS2 UCRT64 packages. On macOS, Homebrew provides `mpv`; on Linux, use your distribution's `libmpv` development package. Linux file selection uses the XDG desktop portal and falls back to `zenity` when the portal cannot open a dialog.

## Run

```sh
cargo run
```

In the import screen, paste a YouTube playlist link to save its metadata without downloading audio. You can also paste a public Spotify link or URI, choose exported files, or enter a Spotify Developer Dashboard Client ID to sync an account. Supported exports include `Playlist1.json`, `YourLibrary.json`, `StreamingHistory*.json`, Spotify Account Data/MyData folders, and Exportify CSV files. Account sync is optional.

## Local data

The app uses the operating system's user config and local data directories through `dirs`. Linux respects `XDG_CONFIG_HOME` and `XDG_DATA_HOME`; Windows and macOS use their standard user folders. See [PRIVACY.md](PRIVACY.md) for filenames, retention, sync, and removal behavior.

## Responsive layout

At 1100 px and wider the app shows the library sidebar, content, Now Playing panel, and full player controls together. Below 1100 px the Now Playing panel collapses and the player becomes compact; below 900 px the sidebar narrows; at 620 px and below navigation moves to the bottom. The 1100 px layout has been visually checked; the other planned sizes still need review.

## Code layout

- `src/app.rs`: application window, navigation, and screens.
- `src/catalog.rs`: Audius/YouTube search and imported-track matching.
- `src/playback.rs` and `src/queue.rs`: libmpv playback and queue.
- `src/library.rs`: local library storage.
- `src/spotify/`: Spotify account integration, API, and data import.
