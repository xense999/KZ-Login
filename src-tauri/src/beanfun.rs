use chrono::{Datelike, Local, Timelike};
use cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use des::Des;
use regex::Regex;
use reqwest::{Client, header};
use reqwest_cookie_store::CookieStoreMutex;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BeanfunError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    Parse(String),
}

impl Serialize for BeanfunError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GameAccount {
    pub sn: String,    // serial number (ssn) — sotp in OTP calls
    pub sid: String,   // account ID (div id) — service_account_id in OTP calls
    pub sname: String, // display name — shown in UI
}

#[derive(Debug, Serialize)]
pub struct OtpResult {
    pub sid: String,
    pub otp: String,
}

#[derive(Debug, Clone)]
pub struct QrInit {
    pub skey: String,
    pub bitmap_base64: String,
    pub deeplink: Option<String>,
    pub verification_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrPollOutcome {
    Waiting,
    Expired,
    Approved,
}

const LOGIN_BASE: &str = "https://login.beanfun.com/";
const PORTAL_BASE: &str = "https://tw.beanfun.com/";
const NEWLOGIN_BASE: &str = "https://tw.newlogin.beanfun.com/";
const SERVICE_CODE: &str = "610074";
const SERVICE_REGION: &str = "T9";

/// 64-char uppercase hex literal required by step 5 of the OTP flow.
/// Verbatim copy from the WPF source — do not modify.
const PPPPP: &str = "1F552AEAFF976018F942B13690C990F60ED01510DDF89165F1658CCE7BC21DBA";

// ─── Client Builders ─────────────────────────────────────────────────────────

pub fn build_client_with_store() -> Result<(Client, Arc<CookieStoreMutex>), BeanfunError> {
    let store = Arc::new(CookieStoreMutex::new(Default::default()));
    let client = Client::builder()
        .cookie_provider(store.clone())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")
        .build()?;
    Ok((client, store))
}

/// Build a client reusing an existing cookie store (e.g. from the login session).
pub fn build_client_from_store(store: &Arc<CookieStoreMutex>) -> Result<Client, BeanfunError> {
    Ok(Client::builder()
        .cookie_provider(store.clone())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")
        .build()?)
}

// ─── Timestamp helpers ────────────────────────────────────────────────────────

/// WPF GetCurrentTime(2): Y(M-1)DDhhmmssfff — cache buster for game_zone URLs
fn dt_compact() -> String {
    let now = Local::now();
    format!(
        "{}{}{:02}{:02}{:02}{:02}{:03}",
        now.year(),
        now.month0(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.nanosecond() / 1_000_000
    )
}

/// WPF GetCurrentTime(0): yyyyMMddHHmmss.fff — cache buster for get_result.ashx
fn dt_iso() -> String {
    Local::now().format("%Y%m%d%H%M%S%.3f").to_string()
}

/// .NET Environment.TickCount equivalent: bottom 32 bits of ms
fn tick_count() -> i32 {
    Local::now().timestamp_millis() as i32
}

// ─── QR Login ─────────────────────────────────────────────────────────────────

pub async fn get_session_key(client: &Client) -> Result<String, BeanfunError> {
    let resp = client
        .get(&format!("{}beanfun_block/bflogin/default.aspx?service=999999_T0", PORTAL_BASE))
        .send()
        .await?;
    let final_url = resp.url().to_string();
    let _ = resp.text().await;

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[sp][Ss]?[Kk]ey=([^&]+)").unwrap());
    re.captures(&final_url)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .ok_or_else(|| BeanfunError::Parse(format!("pSKey not found in redirect URL: {}", &final_url[..final_url.len().min(200)])))
}

pub async fn init_qr_login(client: &Client, skey: &str) -> Result<QrInit, BeanfunError> {
    let index_url = format!("{}Login/Index?pSKey={}", LOGIN_BASE, skey);

    let index_body = client
        .get(&index_url)
        .header(header::ACCEPT, "text/html")
        .send().await?.text().await?;

    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    let token_re = TOKEN_RE.get_or_init(|| {
        Regex::new(r#"__RequestVerificationToken[^>]+value="([^"]+)""#).unwrap()
    });
    let verification_token = token_re.captures(&index_body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .unwrap_or_default();

    let init_url = format!("{}Login/InitLogin?pSKey={}", LOGIN_BASE, skey);
    let body = client
        .get(&init_url)
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, &index_url)
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Origin", "https://login.beanfun.com")
        .send().await?.text().await?;

    #[derive(Deserialize)]
    struct InitResp {
        #[serde(rename = "Result")] result: Option<i64>,
        #[serde(rename = "ResultData")] result_data: Option<InitData>,
    }
    #[derive(Deserialize)]
    struct InitData {
        #[serde(rename = "QRImage")] qr_image: Option<String>,
        #[serde(rename = "DeepLink")] deep_link: Option<String>,
    }

    let parsed: InitResp = serde_json::from_str(&body)
        .map_err(|e| BeanfunError::Parse(format!("QR init JSON parse failed: {e}")))?;
    if parsed.result.unwrap_or(-1) != 0 {
        return Err(BeanfunError::Parse("QR init result error".into()));
    }
    let data = parsed.result_data.ok_or_else(|| BeanfunError::Parse("No ResultData".into()))?;
    let qr_image = data.qr_image.filter(|s| !s.is_empty())
        .ok_or_else(|| BeanfunError::Parse("QRImage empty".into()))?;

    Ok(QrInit {
        skey: skey.to_owned(),
        bitmap_base64: format!("data:image/png;base64,{}", qr_image),
        deeplink: data.deep_link.filter(|s| !s.is_empty()),
        verification_token,
    })
}

pub async fn poll_qr(client: &Client, init: &QrInit) -> Result<QrPollOutcome, BeanfunError> {
    let url = format!("{}QRLogin/CheckLoginStatus", LOGIN_BASE);
    let referer = format!("{}Login/Index?pSKey={}", LOGIN_BASE, &init.skey);

    let mut req = client
        .post(&url)
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, &referer)
        .header("Origin", "https://login.beanfun.com")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::CONTENT_LENGTH, "0")
        .body("");
    if !init.verification_token.is_empty() {
        req = req.header("RequestVerificationToken", &init.verification_token);
    }

    let body = req.send().await?.text().await?;

    #[derive(Deserialize)]
    struct PollResp { #[serde(rename = "ResultMessage")] result_message: Option<String> }

    let parsed: PollResp = serde_json::from_str(&body)
        .map_err(|_| BeanfunError::Parse(format!("QR poll JSON parse failed: {}", &body[..body.len().min(200)])))?;

    match parsed.result_message.as_deref() {
        Some("Failed") | Some("Wait Login") => Ok(QrPollOutcome::Waiting),
        Some("Token Expired") => Ok(QrPollOutcome::Expired),
        Some("Success") => Ok(QrPollOutcome::Approved),
        other => Err(BeanfunError::Parse(format!("Unknown QR status: {:?}", other))),
    }
}

pub async fn finalize_qr(
    client: &Client,
    cookie_store: &Arc<CookieStoreMutex>,
    init: &QrInit,
) -> Result<String, BeanfunError> {
    let index_url = format!("{}Login/Index?pSKey={}", LOGIN_BASE, &init.skey);

    let _ = client
        .get(&format!("{}QRLogin/QRLogin", LOGIN_BASE))
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, &index_url)
        .send().await;

    let send_login_body = client
        .get(&format!("{}Login/SendLogin", LOGIN_BASE))
        .header(header::ACCEPT, "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8")
        .header(header::REFERER, &index_url)
        .send().await?.text().await?;

    let form_inputs = extract_hidden_inputs(&send_login_body);
    if !form_inputs.is_empty() {
        let _ = client
            .post(&format!("{}beanfun_block/bflogin/return.aspx", PORTAL_BASE))
            .header(header::REFERER, LOGIN_BASE)
            .form(&form_inputs)
            .send().await;
    }

    let form5: &[(&str, &str)] = &[
        ("SessionKey", &init.skey),
        ("AuthKey", "OK"),
        ("ServiceCode", ""),
        ("ServiceRegion", ""),
        ("ServiceAccountSN", "0"),
    ];
    let _ = client
        .post(&format!("{}beanfun_block/bflogin/return.aspx", PORTAL_BASE))
        .form(form5)
        .send().await?.text().await;

    let token = {
        let store = cookie_store.lock()
            .map_err(|_| BeanfunError::Parse("Cookie store mutex poisoned".into()))?;
        let result = store
            .iter_unexpired()
            .find(|c| c.name().eq_ignore_ascii_case("bfWebToken"))
            .map(|c| c.value().to_string());
        result
    };
    token.ok_or_else(|| BeanfunError::Parse("bfWebToken not found in any cookie after finalize".into()))
}

// ─── Game Accounts ────────────────────────────────────────────────────────────

pub async fn get_game_accounts(client: &Client, token: &str) -> Result<Vec<GameAccount>, BeanfunError> {
    let inner = format!("game_start.aspx?service_code_and_region={}_{}", SERVICE_CODE, SERVICE_REGION);
    let _ = client
        .get(&format!("{}beanfun_block/auth.aspx", PORTAL_BASE))
        .query(&[("channel", "game_zone"), ("page_and_query", inner.as_str()), ("web_token", token)])
        .send().await;

    let dt = dt_compact();
    let body = client
        .get(&format!("{}beanfun_block/game_zone/game_server_account_list.aspx", PORTAL_BASE))
        .query(&[("sc", SERVICE_CODE), ("sr", SERVICE_REGION), ("dt", dt.as_str())])
        .send().await?.text().await?;

    Ok(parse_game_accounts(&body).unwrap_or_default())
}

fn parse_game_accounts(html: &str) -> Result<Vec<GameAccount>, BeanfunError> {
    // Actual HTML structure from game_server_account_list.aspx:
    // <div id="T..." sn="..." name="..." ... onclick="GameAccount.ShowEditAcountDialog(...)">
    // Disabled accounts have onclick="" — we skip those.
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"<div id="(\w+)" sn="(\d+)" name="([^"]+)"[^>]*onclick="GameAccount\.ShowEditAcountDialog"#).unwrap()
    });

    let accounts: Vec<GameAccount> = re
        .captures_iter(html)
        .filter_map(|c| {
            let sid   = c.get(1)?.as_str();
            let sn    = c.get(2)?.as_str();
            let sname = c.get(3)?.as_str();
            if sid.is_empty() || sn.is_empty() || sname.is_empty() { return None; }
            Some(GameAccount {
                sn: sn.to_string(),
                sid: sid.to_string(),
                sname: html_decode(sname),
            })
        })
        .collect();

    if accounts.is_empty() {
        Err(BeanfunError::Parse("No game accounts found".into()))
    } else {
        Ok(accounts)
    }
}

// ─── OTP (5-step flow) ────────────────────────────────────────────────────────

pub async fn get_otp(
    cookie_store: &Arc<CookieStoreMutex>,
    token: &str,
    account_sn: &str,
    account_sid: &str,
    account_sname: &str,
) -> Result<OtpResult, BeanfunError> {
    let client = build_client_from_store(cookie_store)?;

    // Step 1: game_start_step2.aspx → long_polling_key, unk_data, screatetime
    let step1_body = client
        .get(&format!("{}beanfun_block/game_zone/game_start_step2.aspx", PORTAL_BASE))
        .query(&[
            ("service_code", SERVICE_CODE),
            ("service_region", SERVICE_REGION),
            ("sotp", account_sn),
            ("dt", dt_compact().as_str()),
        ])
        .send().await?.text().await?;

    let long_polling_key = parse_long_polling_key(&step1_body)?;
    let unk_data = parse_unk_data(&step1_body);
    let screatetime = parse_screatetime(&step1_body)?;

    // Step 2: get_cookies.ashx → m_strSecretCode
    let step2_body = client
        .get(&format!("{}generic_handlers/get_cookies.ashx", NEWLOGIN_BASE))
        .send().await?.text().await?;
    let secret_code = parse_secret_code(&step2_body)?;

    // Step 3: record_service_start.ashx (response discarded)
    let mut form3: Vec<(&str, &str)> = vec![
        ("service_code", SERVICE_CODE),
        ("service_region", SERVICE_REGION),
        ("service_account_id", account_sid),
        ("sotp", account_sn),
        ("service_account_display_name", account_sname),
        ("service_account_create_time", screatetime.as_str()),
    ];
    let unk_owned;
    if let Some((k, v)) = &unk_data {
        unk_owned = (k.clone(), v.clone());
        form3.push((unk_owned.0.as_str(), unk_owned.1.as_str()));
    }
    let _ = client
        .post(&format!("{}beanfun_block/generic_handlers/record_service_start.ashx", PORTAL_BASE))
        .form(&form3)
        .send().await;

    // Step 4: get_result.ashx long poll (response discarded)
    let _ = client
        .get(&format!("{}generic_handlers/get_result.ashx", PORTAL_BASE))
        .query(&[
            ("meth", "GetResultByLongPolling"),
            ("key", long_polling_key.as_str()),
            ("_", dt_iso().as_str()),
        ])
        .send().await;

    // Step 5: get_webstart_otp.ashx → "1;{key8}{cipher_hex}"
    let create_time_encoded = screatetime.replace(' ', "%20");
    let otp_url = format!(
        "{}beanfun_block/generic_handlers/get_webstart_otp.ashx\
         ?SN={}&WebToken={}&SecretCode={}&ppppp={}&ServiceCode={}\
         &ServiceRegion={}&ServiceAccount={}&CreateTime={}&d={}",
        PORTAL_BASE,
        long_polling_key, token, secret_code, PPPPP,
        SERVICE_CODE, SERVICE_REGION, account_sid,
        create_time_encoded, tick_count()
    );
    let envelope = client.get(&otp_url).send().await?.text().await?;

    // Step 6: decrypt 1;{key8}{cipher_hex}
    let otp = decrypt_envelope(&envelope)?;

    Ok(OtpResult { sid: account_sid.to_owned(), otp })
}

fn parse_long_polling_key(html: &str) -> Result<String, BeanfunError> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"GetResultByLongPolling&key=(.*?)""#).unwrap());
    re.captures(html).and_then(|c| c.get(1)).map(|m| m.as_str().to_owned())
        .ok_or_else(|| BeanfunError::Parse(format!("No long polling key. Preview: {}", &html[..html.len().min(300)])))
}

/// TW-only: extract (key, value) from `MyAccountData.ServiceAccountCreateTime + "k=v";`
fn parse_unk_data(html: &str) -> Option<(String, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"MyAccountData.ServiceAccountCreateTime \+ "(.*)=(.*)""#).unwrap()
    });
    re.captures(html).map(|c| {
        (percent_decode(c.get(1).map_or("", |m| m.as_str())),
         percent_decode(c.get(2).map_or("", |m| m.as_str())))
    })
}

fn parse_screatetime(html: &str) -> Result<String, BeanfunError> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"ServiceAccountCreateTime: "([^"]+)""#).unwrap());
    re.captures(html).and_then(|c| c.get(1)).map(|m| m.as_str().to_owned())
        .ok_or_else(|| BeanfunError::Parse("No ServiceAccountCreateTime".into()))
}

fn parse_secret_code(html: &str) -> Result<String, BeanfunError> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"var m_strSecretCode = '(.*?)';").unwrap());
    re.captures(html).and_then(|c| c.get(1)).map(|m| m.as_str().to_owned())
        .ok_or_else(|| BeanfunError::Parse("No m_strSecretCode".into()))
}

fn decrypt_envelope(envelope: &str) -> Result<String, BeanfunError> {
    let parts: Vec<&str> = envelope.split(';').collect();
    if parts.len() < 2 || parts[0] != "1" {
        return Err(BeanfunError::Parse(format!(
            "OTP envelope rejected: {}",
            &envelope[..envelope.len().min(100)]
        )));
    }
    let payload = parts[1];
    if payload.len() < 8 {
        return Err(BeanfunError::Parse("OTP payload too short".into()));
    }
    let (key_str, cipher_hex) = payload.split_at(8);

    // Key: 8 ASCII bytes (code points > 0x7F → '?', matching WPF's Encoding.ASCII)
    let key_bytes: Vec<u8> = key_str.chars()
        .map(|c| if (c as u32) <= 0x7F { c as u8 } else { b'?' })
        .collect();

    let ciphertext = hex::decode(cipher_hex)
        .map_err(|e| BeanfunError::Parse(format!("Invalid hex: {e}")))?;

    if ciphertext.len() % 8 != 0 {
        return Err(BeanfunError::Parse("Ciphertext not multiple of 8 bytes".into()));
    }

    // DES/ECB/NoPadding — block-by-block, no padding removal
    let cipher = Des::new(GenericArray::from_slice(&key_bytes));
    let mut buf = ciphertext.clone();
    for chunk in buf.chunks_exact_mut(8) {
        let block = GenericArray::from_mut_slice(chunk);
        cipher.decrypt_block(block);
    }

    // Decode as ASCII (bytes > 0x7F → '?'), then trim NUL bytes (WPF: otp.Trim('\0'))
    let raw: String = buf.iter()
        .map(|&b| if b <= 0x7F { b as char } else { '?' })
        .collect();
    Ok(raw.trim_matches('\0').to_string())
}

fn html_decode(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"&#(?:x([0-9a-fA-F]+)|([0-9]+));|&(amp|lt|gt|quot|apos|nbsp);").unwrap()
    });

    let mut result = String::with_capacity(s.len());
    let mut last_end = 0;

    for cap in re.captures_iter(s) {
        let m = cap.get(0).unwrap();
        result.push_str(&s[last_end..m.start()]);

        if let Some(hex) = cap.get(1) {
            if let Ok(code) = u32::from_str_radix(hex.as_str(), 16) {
                if let Some(c) = char::from_u32(code) {
                    result.push(c);
                }
            }
        } else if let Some(dec) = cap.get(2) {
            if let Ok(code) = dec.as_str().parse::<u32>() {
                if let Some(c) = char::from_u32(code) {
                    result.push(c);
                }
            }
        } else if let Some(named) = cap.get(3) {
            match named.as_str() {
                "amp"  => result.push('&'),
                "lt"   => result.push('<'),
                "gt"   => result.push('>'),
                "quot" => result.push('"'),
                "apos" => result.push('\''),
                "nbsp" => result.push('\u{00A0}'),
                _      => result.push_str(m.as_str()),
            }
        }

        last_end = m.end();
    }
    result.push_str(&s[last_end..]);
    result
}

/// Simple percent-decode (%XX only, `+` treated as literal)
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_nibble(bytes[i + 1]);
            let lo = hex_nibble(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h << 4 | l) as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ─── Shared HTML form input parser ───────────────────────────────────────────

fn extract_hidden_inputs(html: &str) -> Vec<(String, String)> {
    static INPUT_RE: OnceLock<Regex> = OnceLock::new();
    static NAME_RE: OnceLock<Regex> = OnceLock::new();
    static VALUE_RE: OnceLock<Regex> = OnceLock::new();
    static SUBMIT_RE: OnceLock<Regex> = OnceLock::new();

    let input_re = INPUT_RE.get_or_init(|| Regex::new(r"(?is)<input[^>]+>").unwrap());
    let name_re = NAME_RE.get_or_init(|| Regex::new(r#"(?i)name\s*=\s*['"]([^'"]+)['"]"#).unwrap());
    let value_re = VALUE_RE.get_or_init(|| Regex::new(r#"(?i)value\s*=\s*['"]([^'"]*)['"]"#).unwrap());
    let submit_re = SUBMIT_RE.get_or_init(|| Regex::new(r#"(?i)type\s*=\s*["']submit["']"#).unwrap());

    input_re.find_iter(html)
        .filter_map(|tag| {
            let t = tag.as_str();
            if submit_re.is_match(t) { return None; }
            let name = name_re.captures(t)?.get(1)?.as_str().to_owned();
            let value = value_re.captures(t)?.get(1)?.as_str().to_owned();
            Some((name, value))
        })
        .collect()
}

// ─── Session Keep-Alive ───────────────────────────────────────────────────────

/// Ping the Beanfun session to prevent idle logout.
/// Returns true if the session is still valid.
pub async fn check_session_alive(
    cookie_store: &Arc<CookieStoreMutex>,
    token: &str,
) -> Result<bool, BeanfunError> {
    let client = build_client_from_store(cookie_store)?;
    let inner = format!(
        "game_start.aspx?service_code_and_region={}_{}",
        SERVICE_CODE, SERVICE_REGION
    );
    let resp = client
        .get(&format!("{}beanfun_block/auth.aspx", PORTAL_BASE))
        .query(&[
            ("channel", "game_zone"),
            ("page_and_query", inner.as_str()),
            ("web_token", token),
        ])
        .send()
        .await?;

    let final_url = resp.url().as_str().to_lowercase();
    let body = resp.text().await.unwrap_or_default();

    let expired = final_url.contains("login") || body.contains("尚未登入") || body.contains("Please login");
    Ok(!expired)
}
