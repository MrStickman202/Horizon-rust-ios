//! The iOS app's Microsoft sign-in: the OAuth device-code flow, then Xbox Live, XSTS and the
//! Minecraft services, the game ownership check and the profile, as Horizon's launcher signs in
//! (`launcher/src/account.rs`). The tokens live in the device's Keychain only (never a file in
//! Documents, never a log); each player's sign-in stays on their own device. The Azure app is
//! the iOS app's own: `HORIZON_MSA_CLIENT_ID` at build time (`ios/config.env`).
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCOPE: &str = "XboxLive.signin offline_access";
const MICROSOFT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const XBOX_USER: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT: &str = "https://api.minecraftservices.com";
/// The Minecraft token is renewed when it has less than this left (seconds).
const RENEW_BEFORE: u64 = 3600;

/// The app's Azure client ID, if this build has one.
pub fn client_id() -> Option<&'static str> {
    option_env!("HORIZON_MSA_CLIENT_ID").map(str::trim).filter(|id| !id.is_empty())
}

/// A signed-in account, as the Keychain keeps it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub name: String,
    /// The profile UUID without dashes.
    pub uuid: String,
    xuid: Option<String>,
    refresh_token: String,
    minecraft_token: String,
    minecraft_expires_at: u64,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("name", &self.name).finish_non_exhaustive()
    }
}

impl Account {
    /// The game's session for this account (`auth::AuthSession`), while its token is valid.
    pub fn session(&self) -> Option<crate::auth::AuthSession> {
        if self.minecraft_expires_at <= now() {
            return None;
        }
        let text = json!({
            "name": self.name,
            "uuid": self.uuid,
            "access_token": self.minecraft_token,
            "user_type": "msa",
            "xuid": self.xuid,
        })
        .to_string();
        crate::auth::AuthSession::parse(&text).ok()
    }
}

/// Where a sign-in is.
#[derive(Clone, Debug)]
pub enum State {
    SignedOut,
    /// Asking Microsoft for a code.
    Starting,
    /// The player enters `code` at `uri` in a browser.
    Code { code: String, uri: String },
    /// Talking to Xbox Live and Minecraft (a message for the player).
    Working(String),
    SignedIn(Account),
    Failed(String),
}

/// A sign-in shared with the worker that runs it.
#[derive(Clone)]
pub struct SignIn(pub Arc<Mutex<State>>);

impl SignIn {
    /// The account kept in the Keychain, renewed in the background when its token is old.
    pub fn at_start() -> Self {
        let state = match load() {
            Some(account) => State::SignedIn(account),
            None => State::SignedOut,
        };
        let this = Self(Arc::new(Mutex::new(state)));
        if let State::SignedIn(account) = this.state()
            && account.minecraft_expires_at < now() + RENEW_BEFORE
        {
            let me = this.clone();
            std::thread::spawn(move || me.renew(account));
        }
        this
    }

    pub fn state(&self) -> State {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, state: State) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = state;
    }

    /// Starts a sign-in on a worker.
    pub fn start(&self) {
        let Some(client) = client_id() else {
            self.set(State::Failed(crate::lang::get("horizon.ios.account.no_client")));
            return;
        };
        if matches!(self.state(), State::Starting | State::Code { .. } | State::Working(_)) {
            return;
        }
        self.set(State::Starting);
        let me = self.clone();
        std::thread::spawn(move || {
            let result = me.sign_in(client);
            match result {
                Ok(account) => {
                    if let Err(e) = save(&account) {
                        log::warn!("Horizon: could not keep the sign-in in the Keychain: {e}");
                    }
                    me.set(State::SignedIn(account));
                }
                Err(e) => me.set(State::Failed(e)),
            }
        });
    }

    pub fn sign_out(&self) {
        forget();
        self.set(State::SignedOut);
    }

    fn sign_in(&self, client: &str) -> Result<Account, String> {
        let (status, v) = post_form(
            &format!("{MICROSOFT}/devicecode"),
            &[("client_id", client), ("scope", SCOPE)],
            "Microsoft",
        )?;
        let (Some(code), Some(device)) = (str_field(&v, "user_code"), str_field(&v, "device_code"))
        else {
            return Err(oauth_error(status, &v));
        };
        let uri = str_field(&v, "verification_uri").unwrap_or_else(|| "https://www.microsoft.com/link".into());
        let mut interval = v.get("interval").and_then(Value::as_u64).unwrap_or(5).clamp(1, 60);
        let expires = now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(900).min(3600);
        self.set(State::Code { code, uri });
        let (access, refresh) = loop {
            std::thread::sleep(Duration::from_secs(interval));
            if !matches!(self.state(), State::Code { .. }) {
                return Err(crate::lang::get("horizon.ios.account.cancelled"));
            }
            if now() > expires {
                return Err(crate::lang::get("horizon.ios.account.expired"));
            }
            let (status, v) = post_form(
                &format!("{MICROSOFT}/token"),
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", client),
                    ("device_code", &device),
                ],
                "Microsoft",
            )?;
            match v.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => continue,
                Some("slow_down") => {
                    interval = (interval + 5).min(60);
                    continue;
                }
                _ => {}
            }
            match (status, str_field(&v, "access_token"), str_field(&v, "refresh_token")) {
                (200, Some(a), Some(r)) => break (a, r),
                _ => return Err(oauth_error(status, &v)),
            }
        };
        self.set(State::Working(crate::lang::get("horizon.ios.account.working")));
        finish(&access, refresh)
    }

    /// A new Minecraft token through the refresh token (a kept sign-in more than a day old).
    fn renew(&self, account: Account) {
        let Some(client) = client_id() else {
            return;
        };
        let renewed = post_form(
            &format!("{MICROSOFT}/token"),
            &[
                ("grant_type", "refresh_token"),
                ("client_id", client),
                ("refresh_token", &account.refresh_token),
                ("scope", SCOPE),
            ],
            "Microsoft",
        )
        .and_then(|(status, v)| {
            match (status, str_field(&v, "access_token"), str_field(&v, "refresh_token")) {
                (200, Some(a), Some(r)) => finish(&a, r),
                _ => Err(oauth_error(status, &v)),
            }
        });
        match renewed {
            Ok(account) => {
                let _ = save(&account);
                self.set(State::SignedIn(account));
            }
            // No network: the kept account stays, its old token until it runs out.
            Err(e) => log::warn!("Horizon: could not renew the Microsoft sign-in: {e}"),
        }
    }
}

/// From the Microsoft access token: Xbox Live, XSTS, the Minecraft token, ownership, profile.
fn finish(ms_access: &str, refresh_token: String) -> Result<Account, String> {
    let (status, xbl) = post_json(
        XBOX_USER,
        json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }),
        "Xbox Live",
    )?;
    let xbl_token = str_field(&xbl, "Token")
        .filter(|_| status == 200)
        .ok_or_else(|| crate::lang::get("horizon.ios.account.xbox_refused"))?;
    let (status, xsts) = post_json(
        XSTS,
        json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }),
        "Xbox Live",
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(xsts_token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err(crate::lang::get("horizon.ios.account.xbox_refused"));
    };
    let xuid = claims.and_then(|c| str_field(c, "xid"));
    let xtoken = format!("XBL3.0 x={uhs};{xsts_token}");
    let lifetime = |v: &Value| v.get("expires_in").and_then(Value::as_u64).unwrap_or(86400);
    // `launcher/login` first (a new account's `login_with_xbox` token names no profile), then
    // `login_with_xbox`, as the launcher does.
    let (token, expires_in) = match post_json(
        &format!("{MINECRAFT}/launcher/login"),
        json!({ "xtoken": xtoken, "platform": "PC_LAUNCHER" }),
        "the Minecraft services",
    ) {
        Ok((200, v)) if str_field(&v, "access_token").is_some() => {
            (str_field(&v, "access_token").unwrap_or_default(), lifetime(&v))
        }
        _ => {
            let (status, mc) = post_json(
                &format!("{MINECRAFT}/authentication/login_with_xbox"),
                json!({ "identityToken": xtoken }),
                "the Minecraft services",
            )?;
            match (status, str_field(&mc, "access_token")) {
                (200, Some(token)) => (token, lifetime(&mc)),
                (403, _) => return Err(crate::lang::get("horizon.ios.account.not_approved")),
                (429, _) => return Err(crate::lang::get("horizon.ios.account.too_many")),
                _ => return Err(crate::lang::get("horizon.ios.account.minecraft_refused")),
            }
        }
    };
    let (status, items) = get(&format!("{MINECRAFT}/entitlements/mcstore"), &token)?;
    let owns = status == 200
        && items.get("items").and_then(Value::as_array).is_some_and(|items| {
            items.iter().any(|i| {
                matches!(i.get("name").and_then(Value::as_str), Some("game_minecraft" | "product_minecraft"))
            })
        });
    if !owns {
        return Err(crate::lang::get("horizon.ios.account.does_not_own"));
    }
    let (status, profile) = get(&format!("{MINECRAFT}/minecraft/profile"), &token)?;
    let (Some(uuid), Some(name)) = (str_field(&profile, "id"), str_field(&profile, "name")) else {
        return Err(crate::lang::get(if status == 404 {
            "horizon.ios.account.no_profile"
        } else {
            "horizon.ios.account.minecraft_refused"
        }));
    };
    Ok(Account {
        name,
        uuid: uuid.replace('-', "").to_lowercase(),
        xuid,
        refresh_token,
        minecraft_token: token,
        minecraft_expires_at: now().saturating_add(expires_in.min(7 * 86_400)),
    })
}

fn xsts_error(code: u64) -> String {
    crate::lang::get(match code {
        2148916233 => "horizon.ios.account.no_xbox",
        2148916235 => "horizon.ios.account.region",
        2148916236 | 2148916237 => "horizon.ios.account.adult",
        2148916238 => "horizon.ios.account.child",
        _ => "horizon.ios.account.xbox_refused",
    })
}

fn oauth_error(status: u16, v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => crate::lang::get("horizon.ios.account.declined"),
        Some("expired_token") => crate::lang::get("horizon.ios.account.expired"),
        Some("invalid_grant") => crate::lang::get("horizon.ios.account.again"),
        _ => crate::lang::tr("horizon.ios.account.failed", &[&status]),
    }
}

// ----- HTTP -------------------------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .timeout_connect(Duration::from_secs(10))
        .user_agent(concat!("horizon-ios/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn read(result: Result<ureq::Response, ureq::Error>, service: &str) -> Result<(u16, Value), String> {
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(crate::lang::tr("horizon.ios.account.offline", &[&service]));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn post_form(url: &str, fields: &[(&str, &str)], service: &str) -> Result<(u16, Value), String> {
    read(agent().post(url).send_form(fields), service)
}

fn post_json(url: &str, body: Value, service: &str) -> Result<(u16, Value), String> {
    read(
        agent()
            .post(url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json")
            .send_string(&body.to_string()),
        service,
    )
}

fn get(url: &str, token: &str) -> Result<(u16, Value), String> {
    read(
        agent().get(url).set("Authorization", &format!("Bearer {token}")).call(),
        "the Minecraft services",
    )
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// ----- The Keychain -----------------------------------------------------------------------

const SERVICE: &str = "dev.softsignal.horizon-ios";
const KEY: &str = "microsoft-account";

#[cfg(target_os = "ios")]
fn load() -> Option<Account> {
    let bytes = security_framework::passwords::get_generic_password(SERVICE, KEY).ok()?;
    serde_json::from_slice(&bytes).ok()
}
#[cfg(target_os = "ios")]
fn save(account: &Account) -> Result<(), String> {
    let bytes = serde_json::to_vec(account).map_err(|e| e.to_string())?;
    security_framework::passwords::set_generic_password(SERVICE, KEY, &bytes).map_err(|e| e.to_string())
}
#[cfg(target_os = "ios")]
fn forget() {
    let _ = security_framework::passwords::delete_generic_password(SERVICE, KEY);
}

// Elsewhere the launcher signs in; nothing is kept here.
#[cfg(not(target_os = "ios"))]
fn load() -> Option<Account> {
    None
}
#[cfg(not(target_os = "ios"))]
fn save(_: &Account) -> Result<(), String> {
    Err("no keychain".into())
}
#[cfg(not(target_os = "ios"))]
fn forget() {}
