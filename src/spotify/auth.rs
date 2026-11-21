use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use super::{http_agent, status_error};

const KEYRING_SERVICE: &str = "rea-spotify";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const REDIRECT_BASE: &str = "http://127.0.0.1";
const REQUIRED_SCOPES: &str = "user-read-private user-library-read playlist-read-private";

pub(super) fn connect(client_id: &str) -> Result<(), String> {
    if let Some(token) = load_token(client_id)? {
        if has_required_scopes(&token.scope) && token.expires_at > unix_time().saturating_add(30) {
            return Ok(());
        }
        if has_required_scopes(&token.scope)
            && let Ok(refreshed) = refresh_token(client_id, &token.refresh_token, &token.scope)
        {
            return save_token(client_id, &refreshed);
        }
    }

    let token = authorize(client_id)?;
    save_token(client_id, &token)
}

pub(super) fn access_token(client_id: &str) -> Result<String, String> {
    let token = load_token(client_id)?.ok_or_else(|| {
        "Conecte sua conta Spotify antes de pesquisar. Use o botão Conectar Spotify.".to_owned()
    })?;
    if !has_required_scopes(&token.scope) {
        return Err("Reconecte sua conta para liberar biblioteca e playlists.".to_owned());
    }
    if token.expires_at > unix_time().saturating_add(30) {
        return Ok(token.access_token);
    }

    let refreshed = refresh_token(client_id, &token.refresh_token, &token.scope)?;
    let access_token = refreshed.access_token.clone();
    save_token(client_id, &refreshed)?;
    Ok(access_token)
}

pub(super) fn refresh_access_token(client_id: &str) -> Result<String, String> {
    let token =
        load_token(client_id)?.ok_or_else(|| "Conecte sua conta Spotify novamente.".to_owned())?;
    if !has_required_scopes(&token.scope) {
        return Err("Reconecte sua conta para liberar biblioteca e playlists.".to_owned());
    }
    let refreshed = refresh_token(client_id, &token.refresh_token, &token.scope)?;
    let access_token = refreshed.access_token.clone();
    save_token(client_id, &refreshed)?;
    Ok(access_token)
}

pub(super) fn clear_token(client_id: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, client_id)
        .map_err(|_| "Não foi possível acessar o chaveiro seguro do sistema.".to_owned())?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Não foi possível remover a sessão do chaveiro do sistema.".to_owned()),
    }
}

fn authorize(client_id: &str) -> Result<TokenRecord, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("Não foi possível iniciar o retorno OAuth local: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("Não foi possível iniciar o retorno OAuth local: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let redirect_uri = format!("{REDIRECT_BASE}:{port}/callback");

    let mut random = [0_u8; 32];
    rand::rng().fill_bytes(&mut random);
    let verifier = URL_SAFE_NO_PAD.encode(random);
    rand::rng().fill_bytes(&mut random);
    let state = URL_SAFE_NO_PAD.encode(random);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut authorization_url =
        Url::parse("https://accounts.spotify.com/authorize").map_err(|error| error.to_string())?;
    authorization_url
        .query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", REQUIRED_SCOPES)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", &challenge)
        .append_pair("state", &state);

    Command::new("xdg-open")
        .arg(authorization_url.as_str())
        .spawn()
        .map_err(|error| format!("Não foi possível abrir o navegador: {error}"))?;

    let code = authorization_code(listener, port, &state)?;
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("client_id", client_id)
        .append_pair("grant_type", "authorization_code")
        .append_pair("code", &code)
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("code_verifier", &verifier);
    request_token(&form.finish(), None, "")
}

fn authorization_code(
    listener: TcpListener,
    port: u16,
    expected_state: &str,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some((code, state, error)) = read_callback(&mut stream, port) {
                    if state.as_deref() != Some(expected_state) {
                        respond(&mut stream, "Resposta OAuth inválida", 400);
                        continue;
                    }
                    if let Some(error) = error {
                        respond(&mut stream, "Conexão cancelada", 200);
                        return Err(format!("A autorização Spotify não foi concluída: {error}"));
                    }
                    if let Some(code) = code {
                        respond(&mut stream, "Spotify conectado", 200);
                        return Ok(code);
                    }
                } else {
                    respond(&mut stream, "Retorno OAuth inválido", 400);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(format!("Falha no retorno OAuth: {error}")),
        }
    }
    Err("A autorização expirou após 5 minutos. Tente conectar novamente.".to_owned())
}

fn read_callback(
    stream: &mut TcpStream,
    port: u16,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut line = String::new();
    let size = std::io::BufReader::new(stream.try_clone().ok()?)
        .take(4096)
        .read_line(&mut line)
        .ok()?;
    if size == 0 || size >= 4096 {
        return None;
    }
    let mut request_parts = line.split_whitespace();
    if request_parts.next()? != "GET" {
        return None;
    }
    let target = request_parts.next()?;
    let base = Url::parse(&format!("{REDIRECT_BASE}:{port}/")).ok()?;
    let url = base.join(target).ok()?;
    if url.host_str() != Some("127.0.0.1") || url.port() != Some(port) || url.path() != "/callback"
    {
        return None;
    }

    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    Some((code, state, error))
}

fn respond(stream: &mut TcpStream, title: &str, status: u16) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title><h2>{title}</h2><p>Você pode fechar esta aba e voltar ao aplicativo.</p>"
    );
    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn refresh_token(
    client_id: &str,
    refresh_token: &str,
    existing_scope: &str,
) -> Result<TokenRecord, String> {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("client_id", client_id)
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", refresh_token);
    request_token(&form.finish(), Some(refresh_token), existing_scope)
}

fn request_token(
    form: &str,
    existing_refresh_token: Option<&str>,
    existing_scope: &str,
) -> Result<TokenRecord, String> {
    let mut response = http_agent()
        .post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(form)
        .map_err(|error| format!("Não foi possível acessar a autenticação Spotify: {error}"))?;
    let status = response.status().as_u16();
    if status >= 400 {
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|value| value.to_str().ok());
        let fallback = status_error(status, retry_after);
        let body = response.body_mut().read_to_string().unwrap_or_default();
        let detail = serde_json::from_str::<TokenError>(&body)
            .ok()
            .and_then(|error| error.error_description.or(error.error));
        return Err(if status == 429 {
            fallback
        } else {
            detail.unwrap_or(fallback)
        });
    }

    let response = response
        .body_mut()
        .read_json::<TokenResponse>()
        .map_err(|error| format!("Resposta de autenticação inválida: {error}"))?;
    let refresh_token = response
        .refresh_token
        .or_else(|| existing_refresh_token.map(str::to_owned))
        .ok_or_else(|| "A resposta OAuth não trouxe um refresh token.".to_owned())?;
    Ok(TokenRecord {
        access_token: response.access_token,
        refresh_token,
        scope: response.scope.unwrap_or_else(|| existing_scope.to_owned()),
        expires_at: unix_time().saturating_add(response.expires_in),
    })
}

fn has_required_scopes(scope: &str) -> bool {
    let granted = scope.split_whitespace().collect::<Vec<_>>();
    REQUIRED_SCOPES
        .split_whitespace()
        .all(|required| granted.contains(&required))
}

fn load_token(client_id: &str) -> Result<Option<TokenRecord>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, client_id)
        .map_err(|_| "Não foi possível acessar o chaveiro seguro do sistema.".to_owned())?;
    match entry.get_password() {
        Ok(value) => serde_json::from_str(&value)
            .map(Some)
            .map_err(|_| "A sessão salva está inválida. Conecte a conta novamente.".to_owned()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Não foi possível acessar o chaveiro seguro do sistema.".to_owned()),
    }
}

fn save_token(client_id: &str, token: &TokenRecord) -> Result<(), String> {
    let value = serde_json::to_string(token)
        .map_err(|_| "Não foi possível preparar a sessão Spotify.".to_owned())?;
    let entry = keyring::Entry::new(KEYRING_SERVICE, client_id)
        .map_err(|_| "Não foi possível acessar o chaveiro seguro do sistema.".to_owned())?;
    entry
        .set_password(&value)
        .map_err(|_| "Não foi possível guardar a sessão no chaveiro seguro do sistema.".to_owned())
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[derive(Serialize, Deserialize)]
struct TokenRecord {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    #[serde(default)]
    scope: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    scope: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: Option<String>,
    error_description: Option<String>,
}
