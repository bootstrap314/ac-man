//! SmartHome's authenticated HTTPS relay. No LAN device connection or Python runtime.
use crate::{AcError, Result, problem};
use aes::{Aes128, Aes192, Aes256};
use base64::{Engine, engine::general_purpose::STANDARD};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use chrono::Utc;
use hmac::{Hmac, Mac};
use md5::Md5;
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use zeroize::{Zeroize, ZeroizeOnDrop};

// Public SmartHome client identifiers from midea-beautiful-air, not user credentials.
const APP_KEY: &str = "ac21b9f9cbfe4ca5a88562ef25e2b768";
const IOT_KEY: &str = "meicloud";
const HMAC_KEY: &str = "PROD_VnoClJI9aikS8dyy";
const INITIAL_URL: &str = "https://mp-prod.appsmb.com/mas/v5/app/proxy?alias=";

pub(crate) fn random_hex(len: usize) -> String {
    let mut b = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}
pub(crate) fn encrypt(text: &str, key: &str, iv: &str) -> Result<String> {
    macro_rules! encrypt_with {
        ($aes:ty) => {{
            let cipher = cbc::Encryptor::<$aes>::new_from_slices(key.as_bytes(), iv.as_bytes())
                .map_err(|_| problem("Invalid SmartHome session encryption parameters."))?;
            hex::encode(cipher.encrypt_padded_vec_mut::<Pkcs7>(text.as_bytes()))
        }};
    }
    Ok(match key.len() {
        16 => encrypt_with!(Aes128),
        24 => encrypt_with!(Aes192),
        32 => encrypt_with!(Aes256),
        _ => return Err(problem("Invalid SmartHome session key length.")),
    })
}
pub(crate) fn decrypt(text: &str, key: &str, iv: &str) -> Result<String> {
    let bytes = hex::decode(text).map_err(|_| problem("Invalid encrypted SmartHome response."))?;
    macro_rules! decrypt_with {
        ($aes:ty) => {{
            cbc::Decryptor::<$aes>::new_from_slices(key.as_bytes(), iv.as_bytes())
                .map_err(|_| problem("Invalid SmartHome session encryption parameters."))?
                .decrypt_padded_vec_mut::<Pkcs7>(&bytes)
                .map_err(|_| problem("Could not decrypt the SmartHome response."))?
        }};
    }
    let plain = match key.len() {
        16 => decrypt_with!(Aes128),
        24 => decrypt_with!(Aes192),
        32 => decrypt_with!(Aes256),
        _ => return Err(problem("Invalid SmartHome session key length.")),
    };
    String::from_utf8(plain).map_err(|_| problem("Invalid text in SmartHome response."))
}
pub(crate) fn password_hashes(login_id: &str, password: &str) -> (String, String) {
    let sha = hex::encode(Sha256::digest(password.as_bytes()));
    let first = hex::encode(Md5::digest(password.as_bytes()));
    let twice = hex::encode(Md5::digest(first.as_bytes()));
    (
        hex::encode(Sha256::digest(format!("{login_id}{sha}{APP_KEY}"))),
        hex::encode(Sha256::digest(format!("{login_id}{twice}{APP_KEY}"))),
    )
}
fn sign(body: &str, random: &str) -> String {
    let mut h =
        Hmac::<Sha256>::new_from_slice(HMAC_KEY.as_bytes()).expect("HMAC accepts any key length");
    h.update(IOT_KEY.as_bytes());
    h.update(body.as_bytes());
    h.update(random.as_bytes());
    hex::encode(h.finalize().into_bytes())
}
pub(crate) fn allowed_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port_or_known_default() == Some(443)
            && u.host_str()
                .is_some_and(|h| h.ends_with(".appsmb.com") || h.ends_with(".smartmidea.net"))
    })
}
fn required(v: &Value, key: &str) -> Result<String> {
    v.get(key)
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty())
        .ok_or_else(|| problem("SmartHome returned an incomplete response."))
}

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct CloudSession {
    pub base_url: String,
    pub uid: String,
    pub access_token: String,
    pub data_key: String,
    pub data_iv: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mac: String,
}
pub(crate) struct CloudClient {
    #[cfg(test)]
    mock: Option<tests::MockTransport>,
    normal: Client,
    wifi: Option<Client>,
    pub route: String,
    pub client_id: String,
    pub session: Option<CloudSession>,
    base_url: String,
}
impl CloudClient {
    pub fn new(client_id: String) -> Result<Self> {
        let builder = || {
            Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(15))
        };
        let normal = builder()
            .build()
            .map_err(|_| problem("Could not initialize the HTTPS client."))?;
        #[cfg(target_os = "macos")]
        let wifi = builder().interface("en0").build().ok();
        #[cfg(not(target_os = "macos"))]
        let wifi = None;
        Ok(Self {
            #[cfg(test)]
            mock: None,
            normal,
            wifi,
            route: "Default internet connection".into(),
            client_id,
            session: None,
            base_url: INITIAL_URL.into(),
        })
    }
    pub fn use_session(&mut self, session: CloudSession) -> Result<()> {
        if !allowed_url(&session.base_url) {
            return Err(problem("Unexpected saved SmartHome server address."));
        }
        self.base_url = session.base_url.clone();
        self.session = Some(session);
        Ok(())
    }
    fn general(&self) -> Value {
        json!({"appId":1010,"format":2,"clientType":1,"language":"en_US","src":1010,
            "stamp":Utc::now().format("%Y%m%d%H%M%S").to_string(),"deviceId":self.client_id,
            "appVNum":"2.22.0","appVersion":"2.22.0","clientVersion":"2.22.0","platformId":"1",
            "reqId":random_hex(16),"retryCount":"3","uid":self.session.as_ref().map(|s|s.uid.as_str()).unwrap_or(""),"userType":"0"})
    }
    async fn post(&mut self, endpoint: &str, data: Value) -> Result<Value> {
        let url = format!("{}{endpoint}", self.base_url);
        if !allowed_url(&url) {
            return Err(problem("Unexpected SmartHome server address."));
        }
        let body = serde_json::to_string(&data)
            .map_err(|_| problem("Could not encode SmartHome request."))?;
        let random = Utc::now().timestamp().to_string();
        let signature = sign(&body, &random);
        let auth = format!("Basic {}", STANDARD.encode(format!("{APP_KEY}:{IOT_KEY}")));
        let request = |client: &Client| {
            let mut r = client
                .post(&url)
                .header("x-recipe-app", "1010")
                .header("Authorization", &auth)
                .header("sign", &signature)
                .header("secretVersion", "1")
                .header("random", &random)
                .header("version", "2.22.0")
                .header("systemVersion", "8.1.0")
                .header("platform", "0")
                .header("Content-Type", "application/json")
                .header("Accept-Encoding", "identity")
                .body(body.clone());
            if let Some(s) = &self.session {
                r = r
                    .header("uid", &s.uid)
                    .header("accessToken", &s.access_token);
            }
            r
        };
        self.route = "Default internet connection".into();
        #[cfg(test)]
        let mocked = self.mock.as_mut().map(|mock| {
            mock.requests.push(request(&self.normal).build().unwrap());
            mock.responses
                .pop_front()
                .expect("Unexpected cloud request: mock exhausted")
        });
        #[cfg(not(test))]
        let mocked: Option<reqwest::Response> = None;
        let mut response = if let Some(response) = mocked {
            response
        } else {
            match request(&self.normal).send().await {
                Err(e) if e.is_connect() && self.wifi.is_some() => {
                    self.route = "Wi-Fi internet".into();
                    request(self.wifi.as_ref().unwrap()).send().await
                }
                other => other,
            }
            .map_err(|_| {
                problem("Cannot reach SmartHome. Check your internet connection and try again.")
            })?
        };
        if !response.status().is_success() {
            return Err(problem(format!(
                "SmartHome returned HTTP {}.",
                response.status().as_u16()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| problem("Could not read the SmartHome response."))?
        {
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
                return Err(problem("SmartHome response exceeded the size limit."));
            }
            bytes.extend_from_slice(&chunk);
        }
        let response: Value = serde_json::from_slice(&bytes)
            .map_err(|_| problem("SmartHome returned an invalid response."))?;
        let code = response
            .get("code")
            .and_then(|c| c.as_i64().or_else(|| c.as_str()?.parse().ok()))
            .ok_or_else(|| problem("SmartHome response is missing its result code."))?;
        let code = i32::try_from(code)
            .map_err(|_| problem("SmartHome returned an invalid result code."))?;
        if code != 0 {
            let message = match code {
                65027 => "The account has reached its signed-in device limit.",
                3101 | 3102 => "Sign-in was rejected. Check the account and password.",
                3004 | 3106 | 3144 | 3302 | 65002 | 65006 => "The session expired. Sign in again.",
                1306 => "The command was accepted without an immediate reply.",
                _ => "The service could not complete the request.",
            }
            .into();
            return Err(AcError::Cloud { code, message });
        }
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }
    pub async fn login(&mut self, account: &str, password: &str) -> Result<()> {
        if account.trim().is_empty() || password.is_empty() {
            return Err(problem("Enter your SmartHome email and password."));
        }
        self.session = None;
        self.base_url = INITIAL_URL.into();
        let mut data = self.general();
        data["userName"] = account.into();
        let region = self
            .post("/v1/multicloud/platform/user/route", data)
            .await?;
        if let Some(url) = region
            .get("masUrl")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            if !allowed_url(url) {
                return Err(problem("Unexpected SmartHome region server."));
            }
            self.base_url = url.into();
        }
        let mut data = self.general();
        data["loginAccount"] = account.into();
        let login_id = required(&self.post("/v1/user/login/id/get", data).await?, "loginId")?;
        let (password_hash, iam_hash) = password_hashes(&login_id, password);
        let stamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
        let data = json!({"iotData":{
            "src":"1010","format":"2","stamp":stamp,"platformId":"1","deviceId":self.client_id,
            "reqId":random_hex(16),"clientType":"1","appId":"1010","language":"en_US",
            "iampwd":iam_hash,"loginAccount":account,"password":password_hash},
            "data":{"appKey":APP_KEY,"deviceId":self.client_id,"platform":"2"},"stamp":stamp,"reqId":random_hex(16)});
        let s = self.post("/mj/user/login", data).await?;
        let hash = hex::encode(Sha256::digest(APP_KEY.as_bytes()));
        let data_key = decrypt(&required(&s, "accessToken")?, &hash[..16], &hash[16..32])?;
        let data_iv = decrypt(&required(&s, "randomData")?, &hash[..16], &hash[16..32])?;
        self.session = Some(CloudSession {
            base_url: self.base_url.clone(),
            uid: required(&s, "uid")?,
            access_token: required(&s["mdata"], "accessToken")?,
            data_key,
            data_iv,
        });
        Ok(())
    }
    pub async fn appliances(&mut self) -> Result<Vec<DiscoveredDevice>> {
        if self.session.is_none() {
            return Err(problem("Sign in to SmartHome first."));
        }
        let data = self
            .post("/v1/appliance/user/list/get", self.general())
            .await?;
        let list = data
            .get("list")
            .and_then(Value::as_array)
            .ok_or_else(|| problem("SmartHome did not return a device list."))?;
        let mut devices = Vec::new();
        for v in list {
            let is_ac = v.get("type").is_some_and(|t| {
                t.as_u64() == Some(172)
                    || t.as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("0xac") || s == "172")
            });
            if !is_ac {
                continue;
            }
            devices.push(DiscoveredDevice {
                id: required(v, "id")?,
                name: v.get("name").and_then(Value::as_str).unwrap_or("").into(),
                mac: v.get("mac").and_then(Value::as_str).unwrap_or("").into(),
            });
        }
        Ok(devices)
    }
    pub async fn send_frame(&mut self, id: &str, frame: &[u8]) -> Result<Option<Vec<u8>>> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| problem("Sign in to SmartHome first."))?;
        let csv = frame
            .iter()
            .map(|&b| (b as i8).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let order = encrypt(&csv, &session.data_key, &session.data_iv)?;
        let data = json!({"clientType":"1","appId":"1010","format":"2","deviceId":self.client_id,
            "applianceCode":id,"order":order,"timestamp":"true","isFull":"false","reqId":random_hex(16),
            "stamp":Utc::now().format("%Y%m%d%H%M%S").to_string()});
        let response = match self.post("/v1/appliance/transparent/send", data).await {
            Err(AcError::Cloud { code: 1306, .. }) => return Ok(None),
            other => other?,
        };
        let Some(reply) = response
            .get("reply")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            return Ok(None);
        };
        let s = self
            .session
            .as_ref()
            .ok_or_else(|| problem("SmartHome session was lost."))?;
        let csv = decrypt(reply, &s.data_key, &s.data_iv)?;
        let mut bytes = csv
            .trim()
            .split(',')
            .map(|v| {
                let n = v
                    .trim()
                    .parse::<i16>()
                    .map_err(|_| problem("Invalid AC response byte."))?;
                if !(-128..=255).contains(&n) {
                    return Err(problem("Invalid AC response byte."));
                }
                Ok(n as u8)
            })
            .collect::<Result<Vec<_>>>()?;
        if bytes.starts_with(&[0x5a, 0x5a]) && bytes.len() >= 40 {
            bytes.drain(..40);
        }
        if bytes.len() < 2 || bytes[0] != 0xaa {
            return Err(problem("Unrecognized AC cloud response."));
        }
        let length = usize::from(bytes[1]) + 1;
        if bytes.len() < length {
            return Err(problem("Incomplete AC cloud response."));
        }
        bytes.truncate(length);
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod crypto_tests {
    use super::*;
    #[test]
    fn credential_destinations_are_restricted() {
        assert!(allowed_url(INITIAL_URL));
        for url in [
            "http://mp-prod.appsmb.com",
            "https://appsmb.com.evil.test",
            "https://user@mp-prod.appsmb.com",
            "https://mp-prod.appsmb.com:444",
        ] {
            assert!(!allowed_url(url));
        }
    }
    #[test]
    fn encryption_rejects_corruption() {
        let encrypted =
            encrypt("synthetic fixture", "0123456789abcdef", "fedcba9876543210").unwrap();
        assert_eq!(
            decrypt(&encrypted, "0123456789abcdef", "fedcba9876543210").unwrap(),
            "synthetic fixture"
        );
        assert!(decrypt("aa", "0123456789abcdef", "fedcba9876543210").is_err());
    }
    #[test]
    fn crypto_matches_reference_library_vectors() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/protocol.json")).unwrap();
        let (sha, iam) = password_hashes("fixture-login-id", "Example-Only!123");
        assert_eq!(sha, fixture["password_sha"]);
        assert_eq!(iam, fixture["password_iam"]);
        assert_eq!(
            encrypt("-86,35,-84,0,37", "0123456789abcdef", "fedcba9876543210").unwrap(),
            fixture["encrypted"]
        );
        assert_eq!(
            sign("{\"fixture\":true}", "1234567890"),
            fixture["signature"]
        );
    }
}
