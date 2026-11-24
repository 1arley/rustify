//! Passive update notice: checks the latest public GitHub release and never
//! downloads or installs anything.

use std::fmt;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::library;

pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const RELEASES_PAGE_URL: &str = "https://github.com/1arley/rustify/releases";
const LATEST_RELEASE_API_URL: &str = "https://api.github.com/repos/1arley/rustify/releases/latest";
const RELEASES_PATH_PREFIX: &str = "/1arley/rustify/releases";
pub(crate) const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
const REQUEST_TIMEOUT_SECS: u64 = 6;
const STATE_FILE_NAME: &str = "update-check.json";

static UPDATE_STATE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn state_lock() -> &'static Mutex<()> {
    UPDATE_STATE_LOCK.get_or_init(|| Mutex::new(()))
}

fn default_enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct UpdateCheckState {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dismissed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_seen: Option<String>,
}

impl Default for UpdateCheckState {
    fn default() -> Self {
        Self {
            enabled: true,
            last_checked_at: None,
            dismissed_version: None,
            latest_seen: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub release_url: String,
    #[allow(dead_code)]
    pub published_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parses `vX.Y.Z` / `X.Y.Z`. Anything else (prerelease, build metadata,
/// missing or extra segments) is rejected.
pub(crate) fn normalize_version(tag: &str) -> Option<Version> {
    let trimmed = tag.trim();
    let trimmed = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    let mut parts = trimmed.split('.');
    let major = parse_part(parts.next()?)?;
    let minor = parse_part(parts.next()?)?;
    let patch = parse_part(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(Version {
        major,
        minor,
        patch,
    })
}

fn parse_part(part: &str) -> Option<u64> {
    if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

pub(crate) fn is_version_dismissed(state: &UpdateCheckState, version: &str) -> bool {
    let Some(dismissed) = state.dismissed_version.as_deref() else {
        return false;
    };
    match (normalize_version(dismissed), normalize_version(version)) {
        (Some(dismissed), Some(version)) => dismissed == version,
        _ => dismissed.trim() == version.trim(),
    }
}

pub(crate) fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn should_check(state: &UpdateCheckState, now: i64) -> bool {
    if !state.enabled {
        return false;
    }
    if let Some(last) = state.last_checked_at
        && now >= last
        && now.saturating_sub(last) < CHECK_INTERVAL_SECS
    {
        return false;
    }
    true
}

pub(crate) fn release_tag_url(version: &str) -> String {
    match normalize_version(version) {
        Some(version) => format!("https://github.com/1arley/rustify/releases/tag/v{version}"),
        None => RELEASES_PAGE_URL.to_owned(),
    }
}

/// The update to advertise: none when checks are disabled or this version was dismissed.
pub(crate) fn visible_update<'a>(
    state: &UpdateCheckState,
    available: &'a Option<UpdateInfo>,
) -> Option<&'a UpdateInfo> {
    if !state.enabled {
        return None;
    }
    available
        .as_ref()
        .filter(|info| !is_version_dismissed(state, &info.latest))
}

/// Update known from a previous check, if still newer and not dismissed.
pub(crate) fn cached_update(state: &UpdateCheckState, current: &str) -> Option<UpdateInfo> {
    if !state.enabled {
        return None;
    }
    let latest = state.latest_seen.as_deref()?;
    let latest_version = normalize_version(latest)?;
    let current_version = normalize_version(current)?;
    if latest_version <= current_version || is_version_dismissed(state, latest) {
        return None;
    }
    let latest = latest_version.to_string();
    Some(UpdateInfo {
        current: current_version.to_string(),
        release_url: release_tag_url(&latest),
        latest,
        published_at: None,
    })
}

pub(crate) fn load_state() -> Result<UpdateCheckState, String> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    load_state_at(&library::storage_path(STATE_FILE_NAME)?)
}

pub(crate) fn save_state(state: &UpdateCheckState) -> Result<(), String> {
    let _guard = state_lock().lock().unwrap_or_else(|e| e.into_inner());
    save_state_at(&library::storage_path(STATE_FILE_NAME)?, state)
}

fn load_state_at(path: &Path) -> Result<UpdateCheckState, String> {
    library::load_json_or_default(path)
}

fn save_state_at(path: &Path, state: &UpdateCheckState) -> Result<(), String> {
    library::save_json_atomic(path, state)
}

#[derive(Deserialize)]
struct GithubRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    published_at: Option<String>,
}

fn is_trusted_release_url(url: &Url) -> bool {
    let path = url.path();
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && (path == RELEASES_PATH_PREFIX || path.starts_with("/1arley/rustify/releases/tag/"))
}

fn validated_release_url(candidate: &str, latest: Version) -> String {
    match Url::parse(candidate.trim()) {
        Ok(url) if is_trusted_release_url(&url) => url.into(),
        _ => format!("https://github.com/1arley/rustify/releases/tag/v{latest}"),
    }
}

fn parse_release_update(release: GithubRelease, current: &str) -> Option<UpdateInfo> {
    if release.draft || release.prerelease {
        return None;
    }
    let latest = normalize_version(&release.tag_name)?;
    let current = normalize_version(current)?;
    if latest <= current {
        return None;
    }
    Some(UpdateInfo {
        current: current.to_string(),
        latest: latest.to_string(),
        release_url: validated_release_url(&release.html_url, latest),
        published_at: release.published_at,
    })
}

pub(crate) fn check_latest_release() -> Result<Option<UpdateInfo>, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(REQUEST_TIMEOUT_SECS)))
        .http_status_as_error(false)
        .build();
    let agent = ureq::Agent::new_with_config(config);
    let mut response = agent
        .get(LATEST_RELEASE_API_URL)
        .header("User-Agent", format!("rustify/{CURRENT_VERSION}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("Não foi possível consultar o GitHub: {error}"))?;
    let status = response.status().as_u16();
    match status {
        404 => Ok(None),
        403 | 429 => {
            Err("Limite temporário da API do GitHub. Tente novamente mais tarde.".to_owned())
        }
        status if status >= 400 => Err(format!("O GitHub respondeu com HTTP {status}.")),
        _ => {
            let release = response
                .body_mut()
                .read_json::<GithubRelease>()
                .map_err(|error| format!("Resposta de release inválida: {error}"))?;
            Ok(parse_release_update(release, CURRENT_VERSION))
        }
    }
}

pub(crate) fn open_release_page(url: &str) -> Result<(), String> {
    let safe = match Url::parse(url.trim()) {
        Ok(parsed) if is_trusted_release_url(&parsed) => String::from(parsed),
        _ => RELEASES_PAGE_URL.to_owned(),
    };
    open::that_detached(safe)
        .map_err(|error| format!("Não foi possível abrir a página da release: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, prerelease: bool, url: &str) -> GithubRelease {
        GithubRelease {
            tag_name: tag.to_owned(),
            html_url: url.to_owned(),
            draft,
            prerelease,
            published_at: Some("2026-11-23T00:00:00Z".to_owned()),
        }
    }

    #[test]
    fn normalize_version_parses_semver_and_rejects_prerelease_or_invalid() {
        let v020 = normalize_version("v0.2.0").unwrap();
        let v021 = normalize_version("0.2.1").unwrap();
        let v0100 = normalize_version("0.10.0").unwrap();
        let v100 = normalize_version("V1.0.0").unwrap();
        assert!(v020 < v021 && v021 < v0100 && v0100 < v100);
        assert_eq!(v0100.to_string(), "0.10.0");
        for invalid in [
            "v0.3.0-rc.1",
            "0.3.0-alpha",
            "v0.2",
            "0.2.0.1",
            "",
            "latest",
            "v..",
            "1.2.x",
        ] {
            assert!(normalize_version(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn should_check_respects_enabled_and_24h_cache() {
        let mut state = UpdateCheckState::default();
        assert!(should_check(&state, 1_000));
        state.last_checked_at = Some(1_000);
        assert!(!should_check(&state, 1_000 + CHECK_INTERVAL_SECS - 1));
        assert!(should_check(&state, 1_000 + CHECK_INTERVAL_SECS));
        state.enabled = false;
        assert!(!should_check(&state, 1_000 + CHECK_INTERVAL_SECS * 10));
    }

    #[test]
    fn parse_release_update_ignores_draft_prerelease_and_non_newer() {
        let good = "https://github.com/1arley/rustify/releases/tag/v0.3.0";
        assert!(parse_release_update(release("v0.3.0", true, false, good), "0.2.0").is_none());
        assert!(parse_release_update(release("v0.3.0", false, true, good), "0.2.0").is_none());
        assert!(parse_release_update(release("v0.2.0", false, false, good), "0.2.0").is_none());
        assert!(parse_release_update(release("v0.1.9", false, false, good), "0.2.0").is_none());
        assert!(
            parse_release_update(release("v0.3.0-rc.1", false, false, good), "0.2.0").is_none()
        );

        let info = parse_release_update(release("v0.3.0", false, false, good), "0.2.0").unwrap();
        assert_eq!(info.latest, "0.3.0");
        assert_eq!(info.current, "0.2.0");
        assert_eq!(info.release_url, good);

        let untrusted = parse_release_update(
            release("v0.3.0", false, false, "https://evil.example/x"),
            "0.2.0",
        )
        .unwrap();
        assert_eq!(
            untrusted.release_url,
            "https://github.com/1arley/rustify/releases/tag/v0.3.0"
        );
    }

    #[test]
    fn untrusted_release_urls_fall_back_to_the_tag_page() {
        let fallback = "https://github.com/1arley/rustify/releases/tag/v0.3.0";
        for url in [
            "https://github.com/1arley/rustify/releases/download/v0.3.0/evil.sh",
            "https://github.com/1arley/rustify/releases_other",
            "http://github.com/1arley/rustify/releases/tag/v0.3.0",
            "https://github.com:8443/1arley/rustify/releases/tag/v0.3.0",
        ] {
            let info = parse_release_update(release("v0.3.0", false, false, url), "0.2.0").unwrap();
            assert_eq!(info.release_url, fallback, "{url}");
        }
        let page =
            parse_release_update(release("v0.3.0", false, false, RELEASES_PAGE_URL), "0.2.0")
                .unwrap();
        assert_eq!(page.release_url, RELEASES_PAGE_URL);
    }

    #[test]
    fn visible_update_follows_enabled_and_per_version_dismissal() {
        let available: Option<UpdateInfo> = None;
        assert!(visible_update(&UpdateCheckState::default(), &available).is_none());

        let info = |latest: &str| UpdateInfo {
            current: "0.2.0".to_owned(),
            latest: latest.to_owned(),
            release_url: release_tag_url(latest),
            published_at: None,
        };
        let mut state = UpdateCheckState::default();
        let v3 = Some(info("0.3.0"));
        assert!(visible_update(&state, &v3).is_some());

        state.dismissed_version = Some("0.3.0".to_owned());
        assert!(visible_update(&state, &v3).is_none());
        assert!(visible_update(&state, &Some(info("0.4.0"))).is_some());

        state.dismissed_version = None;
        state.enabled = false;
        assert!(visible_update(&state, &v3).is_none());
    }

    #[test]
    fn state_persistence_and_dismiss_per_version_roundtrip() {
        assert!(UpdateCheckState::default().enabled);
        let empty: UpdateCheckState = serde_json::from_str("{}").unwrap();
        assert!(empty.enabled);

        let dir = std::env::temp_dir().join(format!("rustify-update-{}", rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("update-check.json");
        assert_eq!(load_state_at(&path).unwrap(), UpdateCheckState::default());

        let mut state = UpdateCheckState {
            last_checked_at: Some(42),
            latest_seen: Some("0.3.0".to_owned()),
            ..UpdateCheckState::default()
        };
        save_state_at(&path, &state).unwrap();
        state = load_state_at(&path).unwrap();
        assert_eq!(state.last_checked_at, Some(42));

        let info = cached_update(&state, "0.2.0").unwrap();
        assert_eq!(info.latest, "0.3.0");
        assert!(cached_update(&state, "0.3.0").is_none());

        state.dismissed_version = Some("v0.3.0".to_owned());
        assert!(is_version_dismissed(&state, "0.3.0"));
        assert!(cached_update(&state, "0.2.0").is_none());
        state.latest_seen = Some("0.4.0".to_owned());
        assert!(!is_version_dismissed(&state, "0.4.0"));
        assert!(cached_update(&state, "0.2.0").is_some());

        state.enabled = false;
        assert!(cached_update(&state, "0.2.0").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
