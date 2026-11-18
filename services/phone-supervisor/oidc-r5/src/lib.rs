use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde::Deserialize;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;
use zeroize::Zeroizing;

const ISSUER: &str = "https://keycloak.p238.test:8443/realms/phone02-p238b";
const DISCOVERY: &str = "https://keycloak.p238.test:8443/realms/phone02-p238b/.well-known/openid-configuration";
const TOKEN_PATH: &str = "/realms/phone02-p238b/protocol/openid-connect/token";
const JWKS_PATH: &str = "/realms/phone02-p238b/protocol/openid-connect/certs";
const CLIENT_ID: &str = "phone02-supervisor-p238b";
const SCOPE: &str = "team_room:connect";
const AUDIENCE: &str = "team-room-api";
const MAX_RESPONSE: usize = 65536;
const MAX_TOKEN: usize = 16384;

#[derive(Deserialize)]
struct Discovery { issuer: String, token_endpoint: String, jwks_uri: String }

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(deserialize_with = "deserialize_zeroizing_string")]
    access_token: Zeroizing<String>,
    token_type: String,
    expires_in: u64,
    scope: String,
}

fn deserialize_zeroizing_string<'de, D>(d: D) -> Result<Zeroizing<String>, D::Error>
where D: serde::Deserializer<'de> {
    String::deserialize(d).map(Zeroizing::new)
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: serde_json::Value,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    jti: String,
    scope: String,
    azp: Option<String>,
}

fn client() -> Result<Client, i32> {
    Client::builder()
        .use_native_tls()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| 11)
}

fn valid_url(url: &str, path: &str, allow_probe_port: bool) -> bool {
    let Ok(u) = Url::parse(url) else { return false; };
    u.scheme() == "https"
        && u.host_str() == Some("keycloak.p238.test")
        && u.port_or_known_default() == Some(if allow_probe_port { 8444 } else { 8443 })
        && u.path() == path
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
}

fn read_body(mut response: Response) -> Result<Zeroizing<Vec<u8>>, i32> {
    let status = response.status().as_u16();
    if status < 200 || status >= 300 { return Err(20 + status as i32); }
    let declared = response.content_length().unwrap_or(0);
    if declared as usize > MAX_RESPONSE { return Err(31); }
    let mut body = Zeroizing::new(Vec::new());
    let mut limited = response.take((MAX_RESPONSE + 1) as u64);
    limited.read_to_end(&mut *body).map_err(|_| 32)?;
    if body.len() > MAX_RESPONSE { return Err(31); }
    Ok(body)
}

fn get_discovery(c: &Client) -> Result<Discovery, i32> {
    if !valid_url(DISCOVERY, "/realms/phone02-p238b/.well-known/openid-configuration", false) { return Err(12); }
    let response = c.get(DISCOVERY).header("Accept", "application/json").send().map_err(|_| 13)?;
    let body = read_body(response)?;
    let d: Discovery = serde_json::from_slice(&body).map_err(|_| 14)?;
    if d.issuer != ISSUER { return Err(15); }
    if !valid_url(&d.token_endpoint, TOKEN_PATH, false) { return Err(16); }
    if !valid_url(&d.jwks_uri, JWKS_PATH, false) { return Err(17); }
    Ok(d)
}

struct ZeroizingReader { bytes: Zeroizing<Vec<u8>>, position: usize }
impl Read for ZeroizingReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let remaining = self.bytes.len().saturating_sub(self.position);
        let count = remaining.min(out.len());
        if count == 0 { return Ok(0); }
        out[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

fn form_body(client_id: &str, secret: &str, scope: &str) -> Zeroizing<String> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("grant_type", "client_credentials");
    serializer.append_pair("client_id", client_id);
    serializer.append_pair("client_secret", secret);
    serializer.append_pair("scope", scope);
    Zeroizing::new(serializer.finish())
}

fn post_token(c: &Client, endpoint: &str, client_id: &str, secret: &str, scope: &str) -> Result<(u16, Zeroizing<Vec<u8>>), i32> {
    if !valid_url(endpoint, TOKEN_PATH, false) { return Err(18); }
    let body = form_body(client_id, secret, scope);
    let request_bytes = Zeroizing::new(body.as_bytes().to_vec());
    let request_body = reqwest::blocking::Body::new(ZeroizingReader { bytes: request_bytes, position: 0 });
    let response = c.post(endpoint)
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(request_body)
        .send().map_err(|_| 19)?;
    let status = response.status().as_u16();
    let mut bytes = Zeroizing::new(Vec::new());
    let mut limited = response.take((MAX_RESPONSE + 1) as u64);
    limited.read_to_end(&mut *bytes).map_err(|_| 21)?;
    if bytes.len() > MAX_RESPONSE { return Err(22); }
    Ok((status, bytes))
}

fn verify_token(c: &Client, token: &str, expires_in: u64, response_scope: &str, requested_scope: &str) -> Result<(), i32> {
    if token.is_empty() || token.len() > MAX_TOKEN || expires_in == 0 || expires_in > 3600 { return Err(40); }
    if !response_scope.split_whitespace().any(|s| s == requested_scope) { return Err(41); }
    let header = decode_header(token).map_err(|_| 42)?;
    if header.alg != Algorithm::RS256 { return Err(43); }
    let kid = header.kid.ok_or(44)?;
    let discovery = get_discovery(c)?;
    let response = c.get(&discovery.jwks_uri).header("Accept", "application/json").send().map_err(|_| 45)?;
    let jwks_body = read_body(response)?;
    let jwks: serde_json::Value = serde_json::from_slice(&jwks_body).map_err(|_| 46)?;
    let keys = jwks.get("keys").and_then(|v| v.as_array()).ok_or(47)?;
    let key = keys.iter().find(|k| {
        k.get("kid").and_then(|v| v.as_str()) == Some(kid.as_str())
            && k.get("kty").and_then(|v| v.as_str()) == Some("RSA")
            && k.get("use").and_then(|v| v.as_str()).map(|v| v == "sig").unwrap_or(true)
            && k.get("alg").and_then(|v| v.as_str()).map(|v| v == "RS256").unwrap_or(true)
    }).ok_or(48)?;
    let n = key.get("n").and_then(|v| v.as_str()).ok_or(49)?;
    let e = key.get("e").and_then(|v| v.as_str()).ok_or(50)?;
    let decoding_key = DecodingKey::from_rsa_components(n, e).map_err(|_| 51)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[AUDIENCE]);
    validation.leeway = 0;
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.required_spec_claims = ["iss", "sub", "aud", "exp", "iat", "jti"].iter().map(|s| s.to_string()).collect();
    let data = decode::<Claims>(token, &decoding_key, &validation).map_err(|_| 52)?;
    let claims = data.claims;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| 53)?.as_secs();
    if claims.iss != ISSUER || claims.sub.is_empty() || claims.jti.is_empty() { return Err(54); }
    if claims.azp.as_deref() != Some(CLIENT_ID) { return Err(55); }
    if !claims.scope.split_whitespace().any(|s| s == requested_scope) { return Err(56); }
    if claims.exp <= now || claims.exp.saturating_sub(now) > expires_in.saturating_add(90) { return Err(57); }
    if claims.iat > now.saturating_add(30) { return Err(58); }
    if claims.nbf.map(|n| n > now.saturating_add(30)).unwrap_or(false) { return Err(59); }
    match &claims.aud {
        serde_json::Value::String(s) if s == AUDIENCE => {},
        serde_json::Value::Array(a) if a.iter().any(|v| v.as_str() == Some(AUDIENCE)) => {},
        _ => return Err(60),
    }
    Ok(())
}

fn acquire(secret_bytes: &[u8]) -> Result<(Zeroizing<String>, u64), i32> {
    if secret_bytes.len() < 32 || secret_bytes.len() > 256 { return Err(2); }
    let secret_text = std::str::from_utf8(secret_bytes).map_err(|_| 3)?;
    let secret = Zeroizing::new(secret_text.to_owned());
    let c = client()?;
    let d = get_discovery(&c)?;
    let (status, body) = post_token(&c, &d.token_endpoint, CLIENT_ID, &secret, SCOPE)?;
    if status == 502 || status == 503 || status == 504 { return Err(20 + status as i32); }
    if status != 200 { return Err(100 + status as i32); }
    let response: TokenResponse = serde_json::from_slice(&body).map_err(|_| 30)?;
    if !response.token_type.eq_ignore_ascii_case("bearer") { return Err(33); }
    verify_token(&c, &response.access_token, response.expires_in, &response.scope, SCOPE)?;
    Ok((response.access_token, response.expires_in))
}

unsafe fn secret_slice<'a>(p: *const u8, len: u32) -> Result<&'a [u8], i32> {
    if p.is_null() || len < 32 || len > 256 { return Err(2); }
    Ok(std::slice::from_raw_parts(p, len as usize))
}

#[no_mangle]
pub unsafe extern "system" fn r5_acquire_token(
    secret_ptr: *const u8, secret_len: u32,
    token_out: *mut u8, token_capacity: u32,
    token_len_out: *mut u32, expires_in_out: *mut u32,
) -> i32 {
    if token_out.is_null() || token_len_out.is_null() || expires_in_out.is_null() { return 1; }
    let result = catch_unwind(AssertUnwindSafe(|| {
        let secret = secret_slice(secret_ptr, secret_len)?;
        let (token, expires) = acquire(secret)?;
        let bytes = token.as_bytes();
        if bytes.len() > token_capacity as usize { return Err(70); }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), token_out, bytes.len());
        *token_len_out = bytes.len() as u32;
        *expires_in_out = expires as u32;
        Ok::<(), i32>(())
    }));
    match result { Ok(Ok(())) => 0, Ok(Err(e)) => e, Err(_) => 99 }
}

#[no_mangle]
pub unsafe extern "system" fn r5_probe_wrong_secret(secret_ptr: *const u8, secret_len: u32) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let input = secret_slice(secret_ptr, secret_len)?;
        let mut wrong = Zeroizing::new(String::from_utf8(input.to_vec()).map_err(|_| 2)?);
        let last = wrong.pop().ok_or(2)?;
        wrong.push(if last == 'A' { 'B' } else { 'A' });
        let c = client()?;
        let d = get_discovery(&c)?;
        let (status, _) = post_token(&c, &d.token_endpoint, CLIENT_ID, &wrong, SCOPE)?;
        if status == 400 || status == 401 { Ok::<(), i32>(()) } else { Err(80 + status as i32) }
    }));
    match result { Ok(Ok(())) => 0, Ok(Err(e)) => e, Err(_) => 99 }
}

#[no_mangle]
pub unsafe extern "system" fn r5_probe_wrong_client(secret_ptr: *const u8, secret_len: u32) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let input = secret_slice(secret_ptr, secret_len)?;
        let secret = Zeroizing::new(std::str::from_utf8(input).map_err(|_| 2)?.to_owned());
        let c = client()?;
        let d = get_discovery(&c)?;
        let (status, _) = post_token(&c, &d.token_endpoint, "phone02-supervisor-p238b-wrong", &secret, SCOPE)?;
        if status == 400 || status == 401 { Ok::<(), i32>(()) } else { Err(90 + status as i32) }
    }));
    match result { Ok(Ok(())) => 0, Ok(Err(e)) => e, Err(_) => 99 }
}

#[no_mangle]
pub unsafe extern "system" fn r5_probe_wrong_scope(secret_ptr: *const u8, secret_len: u32) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let input = secret_slice(secret_ptr, secret_len)?;
        let secret = Zeroizing::new(std::str::from_utf8(input).map_err(|_| 2)?.to_owned());
        let c = client()?;
        let d = get_discovery(&c)?;
        let (status, body) = post_token(&c, &d.token_endpoint, CLIENT_ID, &secret, "team_room:admin")?;
        if status == 400 {
            let error: serde_json::Value = serde_json::from_slice(&body).map_err(|_| 111)?;
            let code = error.get("error").and_then(serde_json::Value::as_str).unwrap_or("");
            let description = error.get("error_description").and_then(serde_json::Value::as_str).unwrap_or("").to_lowercase();
            if code == "invalid_scope" || (code == "invalid_request" && description.contains("scope")) { return Ok::<(), i32>(()); }
            return Err(112);
        }
        if status != 200 { return Err(113 + status as i32); }
        let response: TokenResponse = serde_json::from_slice(&body).map_err(|_| 114)?;
        if !response.scope.split_whitespace().any(|s| s == "team_room:admin") { return Ok(()); }
        Err(110)
    }));
    match result { Ok(Ok(())) => 0, Ok(Err(e)) => e, Err(_) => 99 }
}

#[no_mangle]
pub extern "system" fn r5_probe_bad_endpoint() -> i32 {
    if !valid_url("https://evil.p238.test:8443/realms/phone02-p238b/protocol/openid-connect/token", TOKEN_PATH, false) { 0 } else { 1 }
}

#[no_mangle]
pub extern "system" fn r5_probe_malformed_response() -> i32 {
    let malformed = br#"{"access_token": "#;
    if serde_json::from_slice::<TokenResponse>(malformed).is_err() { 0 } else { 1 }
}

#[no_mangle]
pub extern "system" fn r5_probe_tls_failure() -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let c = client()?;
        let url = "https://keycloak.p238.test:8444/realms/phone02-p238b/.well-known/openid-configuration";
        if !valid_url(url, "/realms/phone02-p238b/.well-known/openid-configuration", true) { return Err(120); }
        let socket = std::net::TcpStream::connect_timeout(&"127.0.0.1:8444".parse().map_err(|_| 121)?, Duration::from_secs(2)).map_err(|_| 122)?;
        drop(socket);
        match c.get(url).send() { Err(e) => { let message=format!("{:?}",e).to_lowercase(); if message.contains("certificate")||message.contains("cert")||message.contains("tls")||message.contains("issuer")||message.contains("trust") { Ok::<(), i32>(()) } else { Err(124) } }, Ok(_) => Err(123) }
    }));
    match result { Ok(Ok(())) => 0, Ok(Err(e)) => e, Err(_) => 99 }
}

#[no_mangle]
pub extern "system" fn r5_probe_unavailable() -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let c = client()?;
        let socket = std::net::TcpStream::connect_timeout(&"127.0.0.1:8443".parse().map_err(|_| 130)?, Duration::from_secs(2));
        if socket.is_ok() { return Err(131); }
        match c.get(DISCOVERY).send() { Err(_) => Ok::<(), i32>(()), Ok(_) => Err(132) }
    }));
    match result { Ok(Ok(())) => 0, Ok(Err(e)) => e, Err(_) => 99 }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_https_discovery_and_token_endpoints_are_accepted() {
        assert!(valid_url(DISCOVERY, "/realms/phone02-p238b/.well-known/openid-configuration", false));
        assert!(valid_url("https://keycloak.p238.test:8443/realms/phone02-p238b/protocol/openid-connect/token", TOKEN_PATH, false));
        assert!(valid_url("https://keycloak.p238.test:8443/realms/phone02-p238b/protocol/openid-connect/certs", JWKS_PATH, false));
    }
    #[test]
    fn http_wrong_host_wrong_port_and_wrong_path_are_rejected() {
        assert!(!valid_url("http://keycloak.p238.test:8443/realms/phone02-p238b/protocol/openid-connect/token", TOKEN_PATH, false));
        assert!(!valid_url("https://evil.p238.test:8443/realms/phone02-p238b/protocol/openid-connect/token", TOKEN_PATH, false));
        assert!(!valid_url("https://keycloak.p238.test:9443/realms/phone02-p238b/protocol/openid-connect/token", TOKEN_PATH, false));
        assert!(!valid_url("https://keycloak.p238.test:8443/other", TOKEN_PATH, false));
        assert!(!valid_url("https://user@keycloak.p238.test:8443/realms/phone02-p238b/protocol/openid-connect/token", TOKEN_PATH, false));
    }
    #[test]
    fn malformed_token_response_is_rejected() {
        assert!(serde_json::from_slice::<TokenResponse>(br#"{"access_token": "#).is_err());
        assert!(serde_json::from_slice::<TokenResponse>(br#"{"access_token":"x","token_type":"Bearer"}"#).is_err());
    }
}
