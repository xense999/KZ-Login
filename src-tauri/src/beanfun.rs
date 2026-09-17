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
    /// The session is gone and the user must log in again. The message is the
    /// literal sentinel the frontend switches on — see `MainPage.vue`.
    #[error("SESSION_EXPIRED")]
    SessionExpired,
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

/// Truncate a server reply for an error message. `&s[..n]` slices by byte and
/// panics unless byte `n` lands on a char boundary — these bodies are beanfun's
/// Chinese error pages, where a 3-byte character makes that the common case, and
/// the panic escapes an async `#[tauri::command]`: the IPC reply is never sent,
/// so the caller's promise never settles and the row spins forever.
fn clip(s: &str, chars: usize) -> String {
    s.chars().take(chars).collect()
}

const LOGIN_BASE: &str = "https://login.beanfun.com/";
const PORTAL_BASE: &str = "https://tw.beanfun.com/";
const SERVICE_CODE: &str = "610074";
const SERVICE_REGION: &str = "T9";

// ─── Client Builders ─────────────────────────────────────────────────────────

/// The browser we present ourselves as. beanfun's HK portal turns away anything
/// that does not look like a modern browser, and its bot scoring reads the user
/// agent together with the client hints below — the Chrome major version has to
/// match in both, a mismatch is itself a signal.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/153.0.0.0 Safari/537.36";

/// The low-entropy client hints a Chrome of that version sends on every request,
/// read from a real Chrome 153 on this machine (`navigator.userAgentData`). The
/// brand list changes with each major version, so it moves with `USER_AGENT`.
const SEC_CH_UA: &str = r#""Google Chrome";v="153", "Not_A Brand";v="8", "Chromium";v="153""#;

fn client_builder() -> reqwest::ClientBuilder {
    let mut headers = header::HeaderMap::new();
    headers.insert("sec-ch-ua", header::HeaderValue::from_static(SEC_CH_UA));
    headers.insert("sec-ch-ua-mobile", header::HeaderValue::from_static("?0"));
    headers.insert("sec-ch-ua-platform", header::HeaderValue::from_static("\"Windows\""));
    Client::builder().user_agent(USER_AGENT).default_headers(headers)
}

pub fn build_client_with_store() -> Result<(Client, Arc<CookieStoreMutex>), BeanfunError> {
    let store = Arc::new(CookieStoreMutex::new(Default::default()));
    let client = client_builder()
        .cookie_provider(store.clone())
        .build()?;
    Ok((client, store))
}

/// Build a client reusing an existing cookie store (e.g. from the login session).
pub fn build_client_from_store(store: &Arc<CookieStoreMutex>) -> Result<Client, BeanfunError> {
    Ok(client_builder()
        .cookie_provider(store.clone())
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

// ─── QR Login ─────────────────────────────────────────────────────────────────

pub async fn get_session_key(client: &Client) -> Result<String, BeanfunError> {
    let resp = client
        .get(&format!("{}beanfun_block/bflogin/default.aspx?service=999999_T0", PORTAL_BASE))
        .send()
        .await?;
    let final_url = resp.url().to_string();
    let body = resp.text().await.unwrap_or_default();

    // The redirect chain normally ends on checkin_step2.aspx?skey=… , but the
    // page also declares the key in its own script. Reading both means a
    // redirect that lands somewhere else (a logged-in jar taking a different
    // route through the SSO checkpoint) does not blind the callers — for the
    // session probe that would silently turn every verdict into "unknown".
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| Regex::new(r"[sp][Ss]?[Kk]ey=([^&]+)").unwrap());
    if let Some(k) = url_re.captures(&final_url).and_then(|c| c.get(1)) {
        return Ok(k.as_str().to_owned());
    }

    static BODY_RE: OnceLock<Regex> = OnceLock::new();
    let body_re = BODY_RE.get_or_init(|| Regex::new(r#"strSessionKey\s*=\s*"([^"]+)""#).unwrap());
    body_re.captures(&body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
        .ok_or_else(|| BeanfunError::Parse(format!("pSKey not found in redirect URL: {}", clip(&final_url, 200))))
}

/// The login page as both login methods need it: the anti-forgery token every
/// POST must echo back, and what `InitLogin` reports about this session.
#[derive(Debug, Clone)]
pub struct LoginPage {
    pub skey: String,
    pub verification_token: String,
    pub captcha_site_key: String,
    qr_image: Option<String>,
    deeplink: Option<String>,
}

impl LoginPage {
    pub fn url(&self) -> String {
        login_index_url(&self.skey)
    }
}

fn login_index_url(skey: &str) -> String {
    format!("{}Login/Index?pSKey={}", LOGIN_BASE, skey)
}

pub async fn open_login_page(client: &Client, skey: &str) -> Result<LoginPage, BeanfunError> {
    let index_url = login_index_url(skey);

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
        #[serde(rename = "RecaptchaV2PublicKey")] captcha_site_key: Option<String>,
    }

    let parsed: InitResp = serde_json::from_str(&body)
        .map_err(|e| BeanfunError::Parse(format!("Login init JSON parse failed: {e}")))?;
    if parsed.result.unwrap_or(-1) != 0 {
        return Err(BeanfunError::Parse("Login init result error".into()));
    }
    let data = parsed.result_data.ok_or_else(|| BeanfunError::Parse("No ResultData".into()))?;

    Ok(LoginPage {
        skey: skey.to_owned(),
        verification_token,
        captcha_site_key: data.captcha_site_key.unwrap_or_default(),
        qr_image: data.qr_image.filter(|s| !s.is_empty()),
        deeplink: data.deep_link.filter(|s| !s.is_empty()),
    })
}

pub async fn init_qr_login(client: &Client, skey: &str) -> Result<QrInit, BeanfunError> {
    let page = open_login_page(client, skey).await?;
    let qr_image = page.qr_image
        .ok_or_else(|| BeanfunError::Parse("QRImage empty".into()))?;

    Ok(QrInit {
        skey: page.skey,
        bitmap_base64: format!("data:image/png;base64,{}", qr_image),
        deeplink: page.deeplink,
        verification_token: page.verification_token,
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
        .map_err(|_| BeanfunError::Parse(format!("QR poll JSON parse failed: {}", clip(&body, 200))))?;

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
    let _ = client
        .get(&format!("{}QRLogin/QRLogin", LOGIN_BASE))
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, login_index_url(&init.skey))
        .send().await;

    complete_login(client, cookie_store, &init.skey).await
}

/// The tail both login methods share once beanfun has accepted the user:
/// beanfun's own page navigates to the same place after a QR approval and after
/// a password login, and that is what hands out `bfWebToken`.
pub async fn complete_login(
    client: &Client,
    cookie_store: &Arc<CookieStoreMutex>,
    skey: &str,
) -> Result<String, BeanfunError> {
    let index_url = login_index_url(skey);

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
        ("SessionKey", skey),
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

/// Ask for the GamaPass entry point. beanfun mints it per session — it carries
/// the way back to this `pSKey`, which is why the address cannot be hardcoded:
/// a bare `accounts.gamania.com/login` would sign the user in and leave them
/// there, with nothing returning to the portal for `complete_login` to finish.
///
/// Mirrors what the login page's own 「使用 gamapass」 button does.
pub async fn go_gamapass(client: &Client, page: &LoginPage) -> Result<String, BeanfunError> {
    let body = client
        .get(&format!("{}Login/GoGamaPass", LOGIN_BASE))
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, page.url())
        .header("X-Requested-With", "XMLHttpRequest")
        .header("Origin", "https://login.beanfun.com")
        .send().await?.text().await?;

    read_gamapass_url(&body)
}

fn read_gamapass_url(body: &str) -> Result<String, BeanfunError> {
    let reply = read_login_reply(body)?;
    if reply.code != 1 {
        let m = reply.message.trim();
        return Err(BeanfunError::Parse(if m.is_empty() {
            "beanfun 沒有給 GamaPass 登入網址".to_owned()
        } else {
            m.to_owned()
        }));
    }
    reply
        .data
        .as_str()
        .filter(|u| u.starts_with("https://"))
        .map(str::to_owned)
        .ok_or_else(|| BeanfunError::Parse("GamaPass 登入網址格式不對".into()))
}

// ─── Password Login ───────────────────────────────────────────────────────────

/// What one step of the password login concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStep {
    /// Accepted — go on to the next step.
    Proceed,
    /// beanfun wants a reCAPTCHA token before it will look at this step.
    CaptchaRequired,
    /// Refused with a message the user can act on here (wrong password…).
    Rejected(String),
    /// This account needs a flow we do not handle; the user should scan a QR.
    UseQr(String),
}

pub async fn check_account_type(
    client: &Client,
    page: &LoginPage,
    account: &str,
    captcha: &str,
) -> Result<LoginStep, BeanfunError> {
    let body = post_login_json(
        client,
        page,
        "Login/CheckAccountType",
        serde_json::json!({ "Account": account, "Captcha": captcha }),
    ).await?;
    read_account_type(&body)
}

pub async fn account_login(
    client: &Client,
    page: &LoginPage,
    account: &str,
    password: &str,
    captcha: &str,
) -> Result<LoginStep, BeanfunError> {
    let body = post_login_json(
        client,
        page,
        "Login/AccountLogin",
        serde_json::json!({ "Account": account, "Pasw": password, "Captcha": captcha, "IsMobile": false }),
    ).await?;
    read_account_login(&body)
}

async fn post_login_json(
    client: &Client,
    page: &LoginPage,
    path: &str,
    payload: serde_json::Value,
) -> Result<String, BeanfunError> {
    let mut req = client
        .post(&format!("{}{}", LOGIN_BASE, path))
        .header(header::ACCEPT, "application/json, text/plain, */*")
        .header(header::REFERER, page.url())
        .header("Origin", "https://login.beanfun.com")
        .header("X-Requested-With", "XMLHttpRequest")
        .json(&payload);
    if !page.verification_token.is_empty() {
        req = req.header("RequestVerificationToken", &page.verification_token);
    }
    Ok(req.send().await?.text().await?)
}

struct LoginReply {
    code: i64,
    result: i64,
    message: String,
    data: serde_json::Value,
}

fn read_login_reply(body: &str) -> Result<LoginReply, BeanfunError> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|_| BeanfunError::Parse(format!("Login reply parse failed: {}", clip(body, 200))))?;
    let code = v["ResultCode"].as_i64()
        .ok_or_else(|| BeanfunError::Parse(format!("Login reply has no ResultCode: {}", clip(body, 200))))?;
    Ok(LoginReply {
        code,
        result: v["Result"].as_i64().unwrap_or(0),
        message: v["ResultMessage"].as_str().unwrap_or_default().to_owned(),
        data: v["ResultData"].clone(),
    })
}

/// A refusal that is really a demand for reCAPTCHA. The flag is not on every
/// step's reply, but the message ("請點選「我不是機器人」！") is.
fn refusal(reply: &LoginReply) -> LoginStep {
    if reply.data["IsRecaptcha"].as_bool() == Some(true) || reply.message.contains("機器人") {
        LoginStep::CaptchaRequired
    } else if reply.message.is_empty() {
        LoginStep::Rejected("登入失敗".into())
    } else {
        LoginStep::Rejected(reply.message.clone())
    }
}

/// Our own description of why QR is needed, plus beanfun's message when it is
/// something a person can read rather than a status word or a URL.
fn use_qr(reason: &str, reply: &LoginReply) -> LoginStep {
    let m = reply.message.trim();
    let readable = !m.is_empty() && !m.is_ascii();
    LoginStep::UseQr(if readable { format!("{reason}：{m}") } else { reason.to_owned() })
}

/// Unlike AccountLogin, this step has no third outcome code: beanfun's page
/// treats every reply that is not 1 as a refusal to show.
fn read_account_type(body: &str) -> Result<LoginStep, BeanfunError> {
    let reply = read_login_reply(body)?;
    match reply.code {
        1 if reply.data["IsGamaPass"].as_bool() == Some(true) => Ok(use_qr("此帳號是 GamaPass 帳號", &reply)),
        1 if reply.result == 2 => Ok(use_qr("此帳號使用動態密碼（OTP）", &reply)),
        1 => Ok(LoginStep::Proceed),
        _ => Ok(refusal(&reply)),
    }
}

fn read_account_login(body: &str) -> Result<LoginStep, BeanfunError> {
    let reply = read_login_reply(body)?;
    match reply.code {
        0 => Ok(refusal(&reply)),
        1 => Ok(LoginStep::Proceed),
        // Either the lock notice or a URL to an advance check the user must
        // pass on beanfun's site; the URL itself means nothing to them.
        2 if reply.message == "AccountLock" => Ok(use_qr("帳號已被鎖定", &reply)),
        2 => Ok(use_qr("beanfun 要求進階驗證", &reply)),
        _ => Err(BeanfunError::Parse(format!("Unknown account login reply: {}", clip(body, 200)))),
    }
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

// ─── Game Launch (GGM handoff) ─────────────────────────────────────────────────

/// Fetch `game_start_step2.aspx` and build the `gamaniagames://` URI that the
/// local Gamania Games Manager (`GGMWebStart.exe`) consumes to launch the game.
///
/// As of the 2026-08-17 beanfun revision the launch ticket is no longer served
/// by `get_webstart_otp.ashx` (which now returns `Query String Error`); it is
/// embedded — encrypted — inside the `m_objData.data` blob on this page, and GGM
/// decrypts and consumes it at launch. We only need to hand the blob back to GGM.
pub async fn build_launch_uri(
    cookie_store: &Arc<CookieStoreMutex>,
    token: &str,
    account_sn: &str,
) -> Result<String, BeanfunError> {
    prime_game_zone(cookie_store, token).await?;
    launch_uri_for(cookie_store, account_sn).await
}

/// Prime the game_zone session the way the website navigates before step 2.
/// This warms the session, not a single account, so a batch only needs it once.
pub async fn prime_game_zone(
    cookie_store: &Arc<CookieStoreMutex>,
    token: &str,
) -> Result<(), BeanfunError> {
    let client = build_client_from_store(cookie_store)?;
    let inner = format!("game_start.aspx?service_code_and_region={}_{}", SERVICE_CODE, SERVICE_REGION);
    let _ = client
        .get(&format!("{}beanfun_block/auth.aspx", PORTAL_BASE))
        .query(&[("channel", "game_zone"), ("page_and_query", inner.as_str()), ("web_token", token)])
        .send().await;
    Ok(())
}

/// The per-account half, on a session already primed above. Unlike the OTP
/// path, one prime does cover a batch of these — verified against links the
/// batch produced (2026-09-15).
pub async fn launch_uri_for(
    cookie_store: &Arc<CookieStoreMutex>,
    account_sn: &str,
) -> Result<String, BeanfunError> {
    let client = build_client_from_store(cookie_store)?;

    let body = client
        .get(&format!("{}beanfun_block/game_zone/game_start_step2.aspx", PORTAL_BASE))
        .query(&[
            ("service_code", SERVICE_CODE),
            ("service_region", SERVICE_REGION),
            ("sotp", account_sn),
            ("dt", dt_compact().as_str()),
        ])
        .send().await?.text().await?;

    let (region, sn, data) = match parse_m_objdata(&body) {
        Ok(v) => v,
        Err(e) => return Err(classify(&client, e).await),
    };
    Ok(format!("gamaniagames://Region={region}&&&&SN={sn}&&&&Cmd=06004&&&&Data={data}"))
}

/// Extract `region` / `sn` / `data` from the inline
/// `var m_objData = { "region": "...", "sn": "...", "data": "..." };` literal.
fn parse_m_objdata(html: &str) -> Result<(String, String, String), BeanfunError> {
    let grab = |key: &str| -> Option<String> {
        Regex::new(&format!(r#""{}"\s*:\s*"([^"]*)""#, key))
            .ok()?
            .captures(html)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
            .filter(|s| !s.is_empty())
    };
    match (grab("region"), grab("sn"), grab("data")) {
        (Some(region), Some(sn), Some(data)) => Ok((region, sn, data)),
        // Deliberately does not guess *why*: beanfun serves a login page, a
        // redirect, or an error blob depending on how the session died, and this
        // used to catch only the one that says 尚未登入 — every other shape was
        // reported as a generic error, leaving a dead token marked as connected.
        // The caller asks the session endpoint instead; see `classify`.
        _ => Err(BeanfunError::Parse("遊戲啟動頁沒有 m_objData（beanfun 可能改版）".into())),
    }
}

/// Decide whether a failed launch/OTP step actually means "logged out".
///
/// Only an authoritative answer overrides `fallback`: if the probe itself fails,
/// or says the session is fine, the original error is what the user sees. A
/// wrong `SESSION_EXPIRED` costs a QR rescan, so it is never a guess.
async fn classify(client: &Client, fallback: BeanfunError) -> BeanfunError {
    match token_state(client).await {
        SessionState::Expired => BeanfunError::SessionExpired,
        _ => fallback,
    }
}

// ─── OTP (revived with CV/Hash/arch integrity params) ──────────────────

#[derive(Debug, Serialize)]
pub struct OtpResult {
    pub sid: String,
    pub otp: String,
}

/// Nibble-substitution alphabets used by GGM's blob decryption (DecryptParam).
/// Only 0..3 are ever used (index = first-hex-digit % 4) but all 8 are kept.
const OTP_ALPHABETS: [&str; 8] = [
    "bac987d65e432f10", "3bc4d5e6f2a79108", "cdbeaf9012456378", "4e6fb81a3c5d7092",
    "bdef1246789ac530", "5f82cb4093e71d6a", "df1468ace0357b92", "b50c61a4f93e82d7",
];

/// Port of GGM's blob decryption (`DecryptParam`): the first hex digit `v`
/// selects alphabet `v % 4`; each remaining char is mapped to its index in that
/// alphabet (a nibble), forming a hex string; an 8-char DES key is spliced out
/// at offset `v + 1`; the remainder is DES-ECB/NoPadding-decrypted with that key.
fn decrypt_blob(data: &str) -> Result<String, BeanfunError> {
    let first = data.chars().next()
        .ok_or_else(|| BeanfunError::Parse("blob 為空".into()))?;
    let v = first.to_digit(16)
        .ok_or_else(|| BeanfunError::Parse("blob 首字非十六進位".into()))? as usize;
    let alpha = OTP_ALPHABETS[v % 4];

    let mut decoded = String::with_capacity(data.len());
    for c in data[1..].chars() {
        let j = alpha.find(c)
            .ok_or_else(|| BeanfunError::Parse(format!("blob 字元 '{c}' 不在 alphabet[{}]", v % 4)))?;
        decoded.push(std::char::from_digit(j as u32, 16).unwrap());
    }

    let off = v + 1;
    if decoded.len() < off + 8 {
        return Err(BeanfunError::Parse("blob 解碼結果過短".into()));
    }
    let key = decoded[off..off + 8].to_string();
    let cipher_hex = format!("{}{}", &decoded[..off], &decoded[off + 8..]);
    let cipher = hex::decode(&cipher_hex)
        .map_err(|e| BeanfunError::Parse(format!("blob 密文非 hex：{e}")))?;
    if cipher.len() % 8 != 0 {
        return Err(BeanfunError::Parse("blob 密文非 8 bytes 倍數".into()));
    }

    let des = Des::new(GenericArray::from_slice(key.as_bytes()));
    let mut buf = cipher;
    for chunk in buf.chunks_exact_mut(8) {
        des.decrypt_block(GenericArray::from_mut_slice(chunk));
    }
    let plain: String = buf.iter()
        .map(|&b| if b <= 0x7F { b as char } else { '?' })
        .collect();
    Ok(plain.trim_matches('\0').to_string())
}

/// Pull one `Key=Value` field out of a `&&&&`-joined blob plaintext.
fn blob_field(plain: &str, key: &str) -> Option<String> {
    plain.split(';').next().unwrap_or("")
        .split("&&&&")
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
        .map(|s| s.to_string())
}

/// Fetch the game login OTP via the 2026-08-17 **v2** flow: decrypt the
/// `m_objData.data` launch blob to recover the LaunchTicket, then POST it — with
/// the GGM client-integrity trio — to `get_webstart_otp_v2.ashx` and decrypt the
/// returned OTP. Replaces the retired v1 GET (`get_webstart_otp.ashx`).
pub async fn get_otp(
    cookie_store: &Arc<CookieStoreMutex>,
    token: &str,
    account_sn: &str,
    account_sid: &str,
    _account_sname: &str,
) -> Result<OtpResult, BeanfunError> {
    prime_game_zone(cookie_store, token).await?;
    otp_for(cookie_store, account_sn, account_sid).await
}

/// A real OTP is exactly this long. Anything else is beanfun answering with
/// something other than a password — seen as a literal "5381" and as 9-char
/// replies (2026-09-15).
const OTP_LEN: usize = 10;

/// The per-account half of [`get_otp`], on a session already primed above.
pub async fn otp_for(
    cookie_store: &Arc<CookieStoreMutex>,
    account_sn: &str,
    account_sid: &str,
) -> Result<OtpResult, BeanfunError> {
    let client = build_client_from_store(cookie_store)?;

    // 1. game_start_step2 → m_objData (sn + encrypted launch blob).
    let body = client
        .get(&format!("{}beanfun_block/game_zone/game_start_step2.aspx", PORTAL_BASE))
        .query(&[
            ("service_code", SERVICE_CODE),
            ("service_region", SERVICE_REGION),
            ("sotp", account_sn),
            ("dt", dt_compact().as_str()),
        ])
        .send().await?.text().await?;
    let (_region, sn, data) = match parse_m_objdata(&body) {
        Ok(v) => v,
        Err(e) => return Err(classify(&client, e).await),
    };

    // 2. Decrypt the blob and exchange the LaunchTicket for the OTP (v2). Only
    // the exchange is ambiguous: a session that dies there comes back as a
    // non-JSON page or a JSON body with no `data`, neither of which names the
    // real cause. The local prep is left to speak for itself.
    let (_service_account, req) = v2_request(&sn, &data)?;
    let otp = match v2_exchange(&client, &req).await {
        Ok(v) => v,
        Err(e) => return Err(classify(&client, e).await),
    };

    Ok(OtpResult { sid: account_sid.to_owned(), otp })
}

/// The half of the v2 OTP call that comes from this machine: the launch blob's
/// own fields plus the GGM integrity trio. Returns `(service_account, body)`.
///
/// Split from [`v2_exchange`] because nothing here touches the network — a
/// corrupt blob or a missing GGM install fails before beanfun is ever asked, so
/// it can never mean "logged out". Callers must not run these through
/// [`classify`]: replacing 找不到遊戲管理員（GGM），請先安裝 with SESSION_EXPIRED
/// sends the user to rescan a QR code, which cannot fix a missing install.
fn v2_request(sn: &str, data: &str) -> Result<(String, serde_json::Value), BeanfunError> {
    let plain = decrypt_blob(data)?;
    let launch_ticket = blob_field(&plain, "LaunchTicket")
        .ok_or_else(|| BeanfunError::Parse("blob 內找不到 LaunchTicket".into()))?;
    let service_account = blob_field(&plain, "ServiceAccount").unwrap_or_default();
    let (cv, hash, arch) = ggm_integrity()?;
    let body = serde_json::json!({
        "SN": sn, "LaunchTicket": launch_ticket, "CV": cv, "Hash": hash, "arch": arch,
    });
    Ok((service_account, body))
}

/// POST a prepared v2 request to `get_webstart_otp_v2.ashx` and decrypt the OTP
/// out of the reply. Stateless — the LaunchTicket self-authenticates — so this
/// works with or without a login session; but every failure here involves
/// beanfun, so an ambiguous one is worth handing to [`classify`].
async fn v2_exchange(client: &Client, body: &serde_json::Value) -> Result<String, BeanfunError> {
    let v2_url = format!("{}beanfun_block/generic_handlers/get_webstart_otp_v2.ashx", PORTAL_BASE);
    let resp_text = client.post(&v2_url).json(body).send().await?.text().await?;

    let resp: serde_json::Value = serde_json::from_str(&resp_text).map_err(|e| {
        BeanfunError::Parse(format!("v2 回應非 JSON：{e} — {}", clip(&resp_text, 200)))
    })?;
    let enc = resp.get("data").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
        .ok_or_else(|| BeanfunError::Parse(format!("v2 回應無 data 欄位：{}", clip(&resp_text, 200))))?;

    // The v2 `data` is `{key8}{cipher_hex}` (no alphabet step) — DES-ECB/NoPadding
    // with the 8-char prefix as the key, i.e. the classic envelope minus "1;".
    let otp = decrypt_envelope(&format!("1;{enc}"))?;

    // A rejection decrypts just as cleanly as a password, so the length is the
    // only thing telling them apart, and it has to be exact — the bad replies
    // seen include 9-character ones as well as the literal "5381". Report what
    // beanfun actually sent alongside it: the reply is a JSON object and we
    // only ever read `data`, so whatever it says about the refusal has been
    // going straight in the bin. Callers retry once on this error.
    if otp.len() != OTP_LEN || !otp.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(BeanfunError::Parse(format!(
            "OTP 內容異常（{} 字：{}）beanfun 回應：{}",
            otp.len(),
            clip(&otp, 16),
            clip(&resp_text, 200)
        )));
    }
    Ok(otp)
}

/// Recover `(service_account, otp)` straight from a shared
/// `gamaniagames://…&&&&SN=…&&&&Data=…` launch URI — no login session needed.
/// Used by 代理登入 to fill a running game with a login someone shared.
pub async fn otp_from_uri(uri: &str) -> Result<(String, String), BeanfunError> {
    let field = |key: &str| -> Option<String> {
        uri.split("&&&&")
            .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
            .map(|s| s.to_string())
    };
    let sn = field("SN").ok_or_else(|| BeanfunError::Parse("連結缺少 SN".into()))?;
    let data = field("Data").ok_or_else(|| BeanfunError::Parse("連結缺少 Data".into()))?;
    let client = client_builder().build()?;
    let (service_account, req) = v2_request(&sn, &data)?;
    let otp = v2_exchange(&client, &req).await?;
    Ok((service_account, otp))
}

/// Compute the GGM client-integrity trio the OTP endpoint now requires:
/// `(CV, Hash, arch)` = (GGM version, lowercase-hex SHA256 of GGMWebStart.dll,
/// x64/x86). Read live from the local GGM install so a GGM update is followed
/// automatically. Fails clearly if GGM is not installed.
fn ggm_integrity() -> Result<(String, String, String), BeanfunError> {
    use sha2::{Digest, Sha256};
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let ggm = hklm
        .open_subkey(r"SOFTWARE\GamaniaGamesManager")
        .map_err(|_| BeanfunError::Parse("找不到遊戲管理員（GGM），請先安裝".into()))?;
    let install_path: String = ggm
        .get_value("InstallPath")
        .map_err(|_| BeanfunError::Parse("讀不到 GGM 安裝路徑".into()))?;
    let cv: String = ggm
        .get_value("Version")
        .map_err(|_| BeanfunError::Parse("讀不到 GGM 版本".into()))?;

    let dll = std::path::Path::new(&install_path).join("GGMWebStart.dll");
    let bytes = std::fs::read(&dll)
        .map_err(|e| BeanfunError::Parse(format!("讀不到 GGMWebStart.dll：{e}")))?;
    let hash = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();

    let arch = if cfg!(target_pointer_width = "64") { "x64" } else { "x86" };
    Ok((cv, hash, arch.to_string()))
}

// ─── GGM update check ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct GgmUpdate {
    /// Locally installed GGM version, or empty if GGM is not installed.
    pub current: String,
    /// Latest version the server offers.
    pub server: String,
    pub has_update: bool,
    /// Installer download URL (GGMSetup_X.exe).
    pub url: String,
}

/// Locally installed GGM version from the registry, `None` if not installed.
fn ggm_version() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\GamaniaGamesManager")
        .ok()?
        .get_value("Version")
        .ok()
}

/// Compare dotted version strings numerically; true when `a` is newer than `b`.
fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| s.split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let (x, y) = (va.get(i).copied().unwrap_or(0), vb.get(i).copied().unwrap_or(0));
        if x != y { return x > y; }
    }
    false
}

/// Ask beanfun's `CheckVersion.ashx` for the latest GGM version and compare with
/// the locally installed one. Used at startup so an out-of-date GGM (which would
/// make the OTP integrity check fail) can be updated before it bites.
pub async fn check_ggm_update() -> Result<GgmUpdate, BeanfunError> {
    let client = Client::new();
    let resp = client
        .get(&format!("{}generic_handlers/CheckVersion.ashx", PORTAL_BASE))
        .send().await?.text().await?;
    let v: serde_json::Value = serde_json::from_str(&resp)
        .map_err(|e| BeanfunError::Parse(format!("CheckVersion 非 JSON：{e}")))?;
    let server = v.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let url = v.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();

    let current = ggm_version().unwrap_or_default();
    // Update when GGM is missing entirely, or the server offers a newer build.
    let has_update = !server.is_empty() && (current.is_empty() || version_newer(&server, &current));

    Ok(GgmUpdate { current, server, has_update, url })
}

/// Download an installer to a temp file and return its path (caller runs it).
/// The file is named after the URL's last path segment, `fallback_name` if none.
pub async fn download_installer(url: &str, fallback_name: &str) -> Result<String, BeanfunError> {
    if !url.starts_with("https://") {
        return Err(BeanfunError::Parse("更新連結無效".into()));
    }
    let bytes = Client::new().get(url).send().await?.bytes().await?;
    let name = url.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(fallback_name);
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, &bytes)
        .map_err(|e| BeanfunError::Parse(format!("寫入安裝檔失敗：{e}")))?;
    Ok(path.to_string_lossy().to_string())
}

/// Stream `url` into `dest`, reporting (downloaded, total) bytes as it goes.
/// `total` is 0 when the server sends no Content-Length.
pub async fn download_to(
    url: &str,
    dest: &std::path::Path,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), BeanfunError> {
    if !url.starts_with("https://") {
        return Err(BeanfunError::Parse("更新連結無效".into()));
    }
    let mut resp = Client::new().get(url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(dest)
        .map_err(|e| BeanfunError::Parse(format!("建立更新檔失敗：{e}")))?;
    let mut done = 0u64;
    on_progress(0, total);
    while let Some(chunk) = resp.chunk().await? {
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| BeanfunError::Parse(format!("寫入更新檔失敗：{e}")))?;
        done += chunk.len() as u64;
        on_progress(done, total);
    }
    // A short body that still ended cleanly would otherwise pass for a complete
    // download — and the caller is about to overwrite a running program with it.
    if total > 0 && done != total {
        return Err(BeanfunError::Parse(format!(
            "更新檔下載不完整（{done}/{total} bytes）"
        )));
    }
    Ok(())
}

// ─── App self-update（GitHub Releases） ────────────────────────────────────────

/// GitHub repo hosting the app's releases. Its latest release's `.exe` asset is
/// the installer we download to self-update.
const GITHUB_REPO: &str = "xense999/KZ-Login";

/// Release asset holding the bare executable, matching the installed binary's
/// name so an in-place update is a straight file swap.
pub const BARE_EXE_ASSET: &str = "kz-login.exe";

#[derive(Debug, Serialize)]
pub struct AppUpdate {
    /// Currently running app version.
    pub current: String,
    /// Latest version on GitHub (tag with any leading `v` stripped).
    pub latest: String,
    pub has_update: bool,
    /// Installer download URL, used for a fresh install and as the fallback
    /// when a release predates the bare-exe asset.
    pub url: String,
    /// Bare executable download URL, used to swap the exe in place. Empty on
    /// releases that ship only the installer.
    pub exe_url: String,
    /// Release notes body, shown to the user before updating.
    pub notes: String,
}

/// Ask GitHub for the latest release and compare its tag with the running
/// version. `current` is the running app version (from Tauri's package info).
pub async fn check_app_update(current: &str) -> Result<AppUpdate, BeanfunError> {
    let api = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let resp = Client::new()
        .get(&api)
        .header(header::USER_AGENT, "KZ-Login-Updater")
        .header(header::ACCEPT, "application/vnd.github+json")
        .send().await?
        .text().await?;
    let v: serde_json::Value = serde_json::from_str(&resp)
        .map_err(|e| BeanfunError::Parse(format!("GitHub 回應非 JSON：{e}")))?;

    let latest = v.get("tag_name").and_then(|x| x.as_str()).unwrap_or("")
        .trim_start_matches('v').to_string();
    // A release ships the NSIS installer plus, since v1.3.0, the bare exe.
    // Releases before that have only the installer, hence the empty exe_url.
    let assets = v.get("assets").and_then(|a| a.as_array());
    let pick = |want: &dyn Fn(&str) -> bool| -> String {
        assets
            .and_then(|arr| {
                arr.iter().find_map(|x| {
                    let name = x.get("name")?.as_str()?;
                    let u = x.get("browser_download_url")?.as_str()?;
                    want(name).then(|| u.to_string())
                })
            })
            .unwrap_or_default()
    };
    let url = pick(&|n| n.ends_with("-setup.exe"));
    let exe_url = pick(&|n| n == BARE_EXE_ASSET);
    let notes = v.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let has_update = !latest.is_empty() && version_newer(&latest, current);

    Ok(AppUpdate { current: current.to_string(), latest, has_update, url, exe_url, notes })
}

fn decrypt_envelope(envelope: &str) -> Result<String, BeanfunError> {
    let parts: Vec<&str> = envelope.split(';').collect();
    if parts.len() < 2 || parts[0] != "1" {
        return Err(BeanfunError::Parse(format!(
            "OTP envelope rejected: {}",
            clip(envelope, 100)
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

// ─── Session state ────────────────────────────────────────────────────────────

/// What beanfun says about a session. Three-state on purpose: a dropped
/// connection and "beanfun says you are logged out" must not collapse into the
/// same answer, because only the second one may cost the user a QR rescan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Alive,
    Expired,
    Unknown,
}

/// beanfun's own SSO checkpoint, and the authority on whether a web token is
/// still logged in.
const CHECK_TOKEN_URL: &str = "https://tw.newlogin.beanfun.com/generic_handlers/check_token.ashx";

/// beanfun's verdict on the `bfWebToken` in this client's cookie jar.
///
/// This runs the check beanfun's own pages run. `checkin_step2.aspx`'s
/// `DealWebToken()` reads the cookie, POSTs `web_token=1&skey=…` here, and
/// branches on `intResult`: 1 the token is good, 0 it sends the browser to the
/// login screen. Anything else is their "service error" path, which tells us
/// nothing about the session.
///
/// It replaces reading tea leaves from `auth.aspx`'s HTML. That check called a
/// session dead only when the reply happened to land on a login URL or contain
/// 尚未登入 — which a *superseded* token does not do. So when the same account
/// logged in again anywhere (a second scan here, a phone, the official
/// launcher), beanfun had already dropped the older token but the keepalive
/// ping kept reporting that account online, and the user only found out when
/// 取得密碼 failed. Verified against the live endpoint: an absent token answers
/// `0 Token value error`, an unknown one `0 Token is not a existence`.
async fn token_state(client: &Client) -> SessionState {
    // The skey identifies this check to beanfun and comes from the same SSO
    // entry point the QR login uses. Failing to get one means we never reached
    // the checkpoint — not that the session is gone.
    let skey = match get_session_key(client).await {
        Ok(k) => k,
        Err(_) => return SessionState::Unknown,
    };

    let referer = format!(
        "https://tw.newlogin.beanfun.com/checkin_step2.aspx?skey={}&display_mode=2",
        skey
    );
    let body = match client
        .post(CHECK_TOKEN_URL)
        .header(header::REFERER, referer)
        .header("X-Requested-With", "XMLHttpRequest")
        .form(&[("web_token", "1"), ("skey", skey.as_str())])
        .send()
        .await
    {
        Ok(r) => match r.text().await {
            Ok(b) => b,
            Err(_) => return SessionState::Unknown,
        },
        Err(_) => return SessionState::Unknown,
    };

    read_token_check(&body)
}

#[derive(Deserialize)]
struct TokenCheck {
    #[serde(rename = "intResult")]
    int_result: i64,
    #[serde(rename = "strResult", default)]
    str_result: String,
}

/// Split out from the request so the contract this depends on is testable
/// without a live session. An unparsable body is `Unknown`: beanfun changing
/// this reply must degrade into "ask again later", never into a mass logout.
///
/// `intResult: 0` alone is *not* a logout, and only one shape of it is. Verified
/// against the live endpoint on 2026-09-02:
///
/// | answer | what it means |
/// |---|---|
/// | `Token is not a existence(RememberFlag)` | beanfun does not know this token — the one real logout |
/// | `Token value error` | the request carried no token at all |
/// | `Session key dose not match.` | the skey was not this session's |
/// | `No Session Data.` | beanfun has no session for this caller |
///
/// Only the first is about the user. The other three are all this app asking
/// wrongly — the probe always sends the jar that holds the token, so a reply
/// saying no token arrived means the request went out broken, not that anyone
/// logged out. Clearing a token costs a QR rescan, so anything but that first
/// answer degrades into "ask again later".
fn read_token_check(body: &str) -> SessionState {
    match serde_json::from_str::<TokenCheck>(body) {
        Ok(c) if c.int_result == 1 => SessionState::Alive,
        Ok(c) if c.int_result == 0 && c.str_result.contains("not a existence") => SessionState::Expired,
        _ => SessionState::Unknown,
    }
}

/// Ask beanfun whether this session is still logged in.
///
/// The jar is checked first, but a mismatch is not a logout. The probe walks
/// beanfun's SSO entry point on this same jar, and that walk may hand back a
/// rotated `bfWebToken`; the account would then be logged in under a token this
/// app no longer knows, and calling that a logout would clear every account on
/// the next tick — 1.4.2's failure by another road. Adopting the new token is
/// the real answer and is more than a session check may decide, so a jar that
/// does not carry the token we were asked about settles nothing.
pub async fn check_session(
    cookie_store: &Arc<CookieStoreMutex>,
    token: &str,
) -> SessionState {
    let carries_token = match cookie_store.lock() {
        Ok(store) => store
            .iter_unexpired()
            .any(|c| c.name().eq_ignore_ascii_case("bfWebToken") && c.value() == token),
        // A poisoned mutex is our bug, not a logout.
        Err(_) => return SessionState::Unknown,
    };
    if !carries_token {
        return SessionState::Unknown;
    }

    probe_with_session(cookie_store).await
}

/// Ask beanfun with the session that owns the token.
///
/// It has to be this jar and no other. `check_token.ashx` is not a read-only
/// lookup: when it accepts a token it hands back a `strAuthKey` for the asking
/// session to finish the SSO handshake with, so asking from a session of our own
/// making is a stranger claiming the token. 1.4.2 did exactly that — a fresh jar
/// per account, every eight minutes — and beanfun stopped recognising the tokens
/// altogether, which the verdict then read as a logout and cleared every account
/// at once. The owning session asking about itself is the same call beanfun's own
/// page makes, and changes nothing.
async fn probe_with_session(cookie_store: &Arc<CookieStoreMutex>) -> SessionState {
    let client = match build_client_from_store(cookie_store) {
        Ok(c) => c,
        Err(_) => return SessionState::Unknown,
    };
    token_state(&client).await
}

#[cfg(test)]
mod tests {
    use super::{clip, read_account_login, read_account_type, read_gamapass_url, read_token_check, LoginStep, SessionState};

    /// The three replies the live endpoint actually returns. The two zero cases
    /// were captured from beanfun on 2026-08-30: no cookie answers "Token value
    /// error", an unknown token "Token is not a existence(RememberFlag)" — the
    /// shape a token beanfun has superseded comes back as.
    #[test]
    fn reads_beanfuns_verdict() {
        assert_eq!(
            read_token_check(r#"{"intResult": 1, "strResult": "", "strAuthKey": "abc"}"#),
            SessionState::Alive
        );
        assert_eq!(
            read_token_check(r#"{"intResult": 0, "strResult": "Token is not a existence(RememberFlag)", "strAuthKey": ""}"#),
            SessionState::Expired
        );
    }

    /// Anything we do not recognise must not log anyone out.
    #[test]
    fn an_unreadable_reply_is_never_a_logout() {
        for body in ["", "<html>maintenance</html>", r#"{"intResult": 9}"#] {
            assert_eq!(read_token_check(body), SessionState::Unknown);
        }
    }

    /// The zeroes that are about our own request, not the user's session: a skey
    /// from another session, a session beanfun no longer has, and a request that
    /// arrived carrying no token — which cannot happen unless we sent it wrong,
    /// since the probe always asks with the jar the token lives in. Reading any
    /// of them as a logout throws away a token that is still good.
    #[test]
    fn a_zero_that_blames_our_own_request_is_not_a_logout() {
        assert_eq!(
            read_token_check(r#"{"intResult": 0, "strResult": "Token value error", "strAuthKey": ""}"#),
            SessionState::Unknown
        );
        assert_eq!(
            read_token_check(r#"{"intResult": 0, "strResult": "Session key dose not match.", "strAuthKey": ""}"#),
            SessionState::Unknown
        );
        assert_eq!(
            read_token_check(r#"{"intResult": 0, "strResult": "No Session Data.", "strAuthKey": ""}"#),
            SessionState::Unknown
        );
    }

    /// The old `&s[..n]` form panicked here: byte 200 of a Chinese error page
    /// lands inside a 3-byte character.
    #[test]
    fn clip_never_splits_a_character() {
        let page = "登入逾時，請重新登入。".repeat(100);
        assert_eq!(clip(&page, 200).chars().count(), 200);
        assert!(page.starts_with(&clip(&page, 200)));
    }

    // Captured from beanfun on 2026-09-14 with an unknown account and no captcha:
    // both steps refuse with the same "tick I'm not a robot" message, and only
    // AccountLogin also raises the flag.
    #[test]
    fn the_gamapass_url_comes_from_the_reply() {
        assert_eq!(
            read_gamapass_url(r#"{"ResultData":"https://accounts.gamania.com/login?x=1","ResultCode":1,"ResultMessage":""}"#).unwrap(),
            "https://accounts.gamania.com/login?x=1"
        );
    }

    #[test]
    fn a_refusal_carries_beanfuns_own_words() {
        let err = read_gamapass_url(r#"{"ResultData":null,"ResultCode":0,"ResultMessage":"暫停服務"}"#).unwrap_err();
        assert!(err.to_string().contains("暫停服務"), "{err}");
    }

    #[test]
    fn anything_that_is_not_an_https_address_is_refused() {
        // 回的若是物件、空字串或 javascript: 之類，都不能拿去 navigate。
        assert!(read_gamapass_url(r#"{"ResultData":{"url":"x"},"ResultCode":1,"ResultMessage":""}"#).is_err());
        assert!(read_gamapass_url(r#"{"ResultData":"javascript:alert(1)","ResultCode":1,"ResultMessage":""}"#).is_err());
    }

    const ACCOUNT_TYPE_NEEDS_CAPTCHA: &str = r#"{"ResultData":{"IsGamaPass":false,"GamaPassUrl":null},"Result":0,"ResultCode":0,"ResultMessage":"請點選「我不是機器人」！"}"#;
    const ACCOUNT_LOGIN_NEEDS_CAPTCHA: &str = r#"{"ResultData":{"IsRecaptcha":true},"Result":0,"ResultCode":0,"ResultMessage":"請點選「我不是機器人」！"}"#;

    #[test]
    fn account_type_reads_each_branch() {
        assert_eq!(read_account_type(ACCOUNT_TYPE_NEEDS_CAPTCHA).unwrap(), LoginStep::CaptchaRequired);
        assert_eq!(
            read_account_type(r#"{"ResultData":{"IsGamaPass":false},"Result":0,"ResultCode":1,"ResultMessage":"Success"}"#).unwrap(),
            LoginStep::Proceed
        );
        assert!(matches!(
            read_account_type(r#"{"ResultData":{"IsGamaPass":false},"Result":2,"ResultCode":1,"ResultMessage":"Success"}"#).unwrap(),
            LoginStep::UseQr(_)
        ));
        assert!(matches!(
            read_account_type(r#"{"ResultData":{"IsGamaPass":true,"GamaPassUrl":"https://x"},"Result":0,"ResultCode":1,"ResultMessage":"Success"}"#).unwrap(),
            LoginStep::UseQr(_)
        ));
        assert_eq!(
            read_account_type(r#"{"ResultData":null,"Result":0,"ResultCode":0,"ResultMessage":"帳號格式錯誤"}"#).unwrap(),
            LoginStep::Rejected("帳號格式錯誤".into())
        );
        // Any code other than 1 is a refusal on this step, not an unreadable reply.
        assert_eq!(
            read_account_type(r#"{"ResultData":null,"Result":0,"ResultCode":-1,"ResultMessage":"系統忙碌中"}"#).unwrap(),
            LoginStep::Rejected("系統忙碌中".into())
        );
    }

    /// beanfun's own words are kept when they say something; status words
    /// like "Success" and "AccountLock" are not shown.
    #[test]
    fn use_qr_keeps_readable_beanfun_messages_only() {
        assert_eq!(
            read_account_type(r#"{"ResultData":{"IsGamaPass":false},"Result":2,"ResultCode":1,"ResultMessage":"請輸入動態密碼"}"#).unwrap(),
            LoginStep::UseQr("此帳號使用動態密碼（OTP）：請輸入動態密碼".into())
        );
        assert_eq!(
            read_account_login(r#"{"ResultData":null,"Result":0,"ResultCode":2,"ResultMessage":"AccountLock"}"#).unwrap(),
            LoginStep::UseQr("帳號已被鎖定".into())
        );
    }

    #[test]
    fn account_login_reads_each_branch() {
        assert_eq!(read_account_login(ACCOUNT_LOGIN_NEEDS_CAPTCHA).unwrap(), LoginStep::CaptchaRequired);
        // The message alone is enough, in case the flag goes missing.
        assert_eq!(
            read_account_login(r#"{"ResultData":null,"Result":0,"ResultCode":0,"ResultMessage":"請點選「我不是機器人」！"}"#).unwrap(),
            LoginStep::CaptchaRequired
        );
        assert_eq!(
            read_account_login(r#"{"ResultData":{"IsRecaptcha":false},"Result":1,"ResultCode":0,"ResultMessage":"帳號或密碼錯誤"}"#).unwrap(),
            LoginStep::Rejected("帳號或密碼錯誤".into())
        );
        assert_eq!(
            read_account_login(r#"{"ResultData":null,"Result":0,"ResultCode":1,"ResultMessage":"Success"}"#).unwrap(),
            LoginStep::Proceed
        );
        assert!(matches!(
            read_account_login(r#"{"ResultData":null,"Result":0,"ResultCode":2,"ResultMessage":"AccountLock"}"#).unwrap(),
            LoginStep::UseQr(_)
        ));
        // An advance-check redirect: the URL must not reach the user as a message.
        match read_account_login(r#"{"ResultData":null,"Result":0,"ResultCode":2,"ResultMessage":"https://login.beanfun.com/Advance"}"#).unwrap() {
            LoginStep::UseQr(msg) => assert!(!msg.contains("http")),
            other => panic!("expected UseQr, got {other:?}"),
        }
    }

    /// A page we cannot read is an error, never a wrong password.
    #[test]
    fn an_unreadable_login_reply_is_an_error() {
        for body in ["", "<html>maintenance</html>", r#"{"ResultMessage":"no code"}"#] {
            assert!(read_account_type(body).is_err(), "{body}");
            assert!(read_account_login(body).is_err(), "{body}");
        }
        assert!(read_account_login(r#"{"ResultCode":9,"ResultMessage":"?"}"#).is_err());
    }

    #[test]
    fn clip_keeps_short_input_whole() {
        assert_eq!(clip("abc", 200), "abc");
        assert_eq!(clip("", 200), "");
    }
}
