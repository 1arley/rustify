mod app;
mod catalog;
mod components;
mod design;
mod home;
mod library;
mod playback;
mod queue;
mod spotify;
#[cfg(test)]
mod ui_tests;
mod update;
mod youtube;

fn main() {
    // Make a bundled yt-dlp visible to libmpv's ytdl hook and child processes.
    if let Some(yt_dlp) = catalog::bundled_yt_dlp() {
        if let Some(dir) = yt_dlp.parent() {
            let current = std::env::var_os("PATH").unwrap_or_default();
            let mut paths: Vec<std::path::PathBuf> = std::env::split_paths(&current).collect();
            if !paths.iter().any(|p| p == dir) {
                paths.insert(0, dir.to_path_buf());
                if let Ok(joined) = std::env::join_paths(paths) {
                    // SAFETY: called at startup before worker threads are spawned.
                    unsafe { std::env::set_var("PATH", joined) };
                }
            }
        }
    }
    app::run();
}
