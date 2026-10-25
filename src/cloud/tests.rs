use super::*;
use std::collections::VecDeque;

pub(super) struct MockTransport {
    pub requests: Vec<reqwest::Request>,
    pub responses: VecDeque<reqwest::Response>,
}
fn raw(status: u16, body: impl Into<Vec<u8>>) -> reqwest::Response {
    http::Response::builder()
        .status(status)
        .body(body.into())
        .unwrap()
        .into()
}
fn ok(data: Value) -> reqwest::Response {
    raw(200, json!({"code":0,"data":data}).to_string())
}
fn client(responses: Vec<reqwest::Response>) -> CloudClient {
    let mut c = CloudClient::new("synthetic-client".into()).unwrap();
    c.mock = Some(MockTransport {
        requests: vec![],
        responses: responses.into(),
    });
    c
}
fn session() -> CloudSession {
    CloudSession {
        base_url: INITIAL_URL.into(),
        uid: "synthetic-user".into(),
        access_token: "synthetic-token".into(),
        data_key: "0123456789abcdef".into(),
        data_iv: "fedcba9876543210".into(),
    }
}
fn authenticated(responses: Vec<reqwest::Response>) -> CloudClient {
    let mut c = client(responses);
    c.use_session(session()).unwrap();
    c
}
fn body(r: &reqwest::Request) -> Value {
    serde_json::from_slice(r.body().unwrap().as_bytes().unwrap()).unwrap()
}
fn complete(c: &CloudClient, count: usize) {
    let m = c.mock.as_ref().unwrap();
    assert_eq!(m.requests.len(), count);
    assert!(m.responses.is_empty());
}
#[tokio::test]
async fn login_routes_hashes_password_and_decrypts_session() {
    let hash = hex::encode(Sha256::digest(APP_KEY.as_bytes()));
    let route = "https://region.appsmb.com/mas/v5/app/proxy?alias=";
    let mut c = client(vec![
        ok(json!({"masUrl":route})),
        ok(json!({"loginId":123})),
        ok(json!({
            "uid":456,"accessToken":encrypt("0123456789abcdef",&hash[..16],&hash[16..32]).unwrap(),
            "randomData":encrypt("fedcba9876543210",&hash[..16],&hash[16..32]).unwrap(),
            "mdata":{"accessToken":"synthetic-token"}
        })),
    ]);
    c.login("example@example.invalid", "Example-password!")
        .await
        .unwrap();
    complete(&c, 3);
    let s = c.session.as_ref().unwrap();
    assert_eq!(s.uid, "456");
    assert_eq!(s.data_key, "0123456789abcdef");
    assert_eq!(s.data_iv, "fedcba9876543210");
    let requests = &c.mock.as_ref().unwrap().requests;
    assert_eq!(
        requests[0].url().as_str(),
        format!("{INITIAL_URL}/v1/multicloud/platform/user/route")
    );
    assert_eq!(requests[2].url().as_str(), format!("{route}/mj/user/login"));
    let (password, iam) = password_hashes("123", "Example-password!");
    assert_eq!(body(&requests[2])["iotData"]["password"], password);
    assert_eq!(body(&requests[2])["iotData"]["iampwd"], iam);
    for r in requests {
        let bytes = r.body().unwrap().as_bytes().unwrap();
        let text = std::str::from_utf8(bytes).unwrap();
        assert!(!text.contains("Example-password!"));
        assert_eq!(r.method(), reqwest::Method::POST);
        assert_eq!(
            r.headers()["sign"],
            sign(text, r.headers()["random"].to_str().unwrap())
        );
        assert!(!r.headers().contains_key("accessToken"));
    }
}
#[tokio::test]
async fn login_rejects_untrusted_region_before_sending_credentials() {
    for url in [
        "http://region.appsmb.com/",
        "https://attacker.invalid/",
        "https://region.appsmb.com.attacker.invalid/",
    ] {
        let mut c = client(vec![ok(json!({"masUrl":url}))]);
        assert!(
            c.login("example@example.invalid", "password")
                .await
                .is_err()
        );
        assert!(c.session.is_none());
        complete(&c, 1);
    }
}
#[tokio::test]
async fn invalid_or_failed_login_never_installs_session() {
    let mut c = client(vec![]);
    assert!(c.login(" ", "password").await.is_err());
    complete(&c, 0);
    for data in [json!({}), json!({"loginId":""})] {
        let mut c = authenticated(vec![ok(json!({})), ok(data)]);
        assert!(
            c.login("example@example.invalid", "password")
                .await
                .is_err()
        );
        assert!(c.session.is_none());
        complete(&c, 2);
    }
}
#[tokio::test]
async fn discovery_filters_appliances_and_sends_session_headers() {
    let mut c = authenticated(vec![ok(json!({"list":[
        {"id":1,"type":172,"name":"Example AC"},
        {"id":"2","type":"0xAC"}, {"id":"3","type":"172"},
        {"id":"4","type":161}, {"id":"5"}
    ]}))]);
    let devices = c.appliances().await.unwrap();
    assert_eq!(
        devices.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
    assert_eq!(devices[0].name, "Example AC");
    assert_eq!(devices[1].name, "");
    let r = &c.mock.as_ref().unwrap().requests[0];
    assert_eq!(r.headers()["uid"], "synthetic-user");
    assert_eq!(r.headers()["accessToken"], "synthetic-token");
    assert_eq!(body(r)["uid"], "synthetic-user");
    complete(&c, 1);
}
#[tokio::test]
async fn discovery_rejects_missing_list_and_ac_id() {
    for data in [
        json!({}),
        json!({"list":{}}),
        json!({"list":[{"type":172}]}),
    ] {
        let mut c = authenticated(vec![ok(data)]);
        assert!(c.appliances().await.is_err());
        complete(&c, 1);
    }
    let mut c = client(vec![]);
    assert!(c.appliances().await.is_err());
    assert!(c.send_frame("1", &[0xaa]).await.is_err());
    complete(&c, 0);
}
#[tokio::test]
async fn service_codes_preserve_classification_without_echoing_sensitive_messages() {
    for code in [
        65027, 3101, 3102, 3004, 3106, 3144, 3302, 65002, 65006, 9999,
    ] {
        for encoded in [json!(code), json!(code.to_string())] {
            let mut c = client(vec![raw(
                200,
                json!({"code":encoded,"msg":"sensitive-server-message"}).to_string(),
            )]);
            let error = c.post("/test", json!({})).await.unwrap_err();
            assert!(matches!(error,AcError::Cloud{code: actual,..} if actual==code));
            assert!(!error.to_string().contains("sensitive-server-message"));
            complete(&c, 1);
        }
    }
}
#[tokio::test]
async fn http_errors_invalid_json_missing_codes_and_oversized_bodies_fail() {
    for response in [
        raw(503, "sensitive-server-message"),
        raw(302, "redirect"),
        raw(200, "not json"),
        raw(200, "{}"),
        raw(200, r#"{"code":false}"#),
        raw(200, vec![b' '; 2 * 1024 * 1024 + 1]),
    ] {
        let mut c = client(vec![response]);
        let error = c.post("/test", json!({})).await.unwrap_err();
        assert!(!error.to_string().contains("sensitive-server-message"));
        complete(&c, 1);
    }
}
#[tokio::test]
async fn command_serializes_signed_bytes_and_decodes_reply() {
    let s = session();
    let reply = encrypt("-86,3,-84,0,99", &s.data_key, &s.data_iv).unwrap();
    let mut c = authenticated(vec![ok(json!({"reply":reply}))]);
    assert_eq!(
        c.send_frame("123", &[0xaa, 0, 127, 128, 255])
            .await
            .unwrap(),
        Some(vec![0xaa, 3, 0xac, 0])
    );
    let data = body(&c.mock.as_ref().unwrap().requests[0]);
    assert_eq!(data["applianceCode"], "123");
    assert_eq!(
        decrypt(data["order"].as_str().unwrap(), &s.data_key, &s.data_iv).unwrap(),
        "-86,0,127,-128,-1"
    );
    complete(&c, 1);
}
#[tokio::test]
async fn command_handles_acknowledgement_and_absent_reply_without_retrying() {
    for response in [
        raw(200, r#"{"code":1306}"#),
        ok(json!({})),
        ok(json!({"reply":""})),
    ] {
        let mut c = authenticated(vec![response]);
        assert_eq!(c.send_frame("123", &[0xaa]).await.unwrap(), None);
        complete(&c, 1);
    }
}
#[tokio::test]
async fn command_rejects_corrupt_ciphertext_and_invalid_frames() {
    let s = session();
    for csv in ["256,1", "-129,1", "not-a-byte", "1,2,3", "-86,9,0", ""] {
        let reply = encrypt(csv, &s.data_key, &s.data_iv).unwrap();
        let mut c = authenticated(vec![ok(json!({"reply":reply}))]);
        assert!(c.send_frame("123", &[0xaa]).await.is_err());
        complete(&c, 1);
    }
    let mut c = authenticated(vec![ok(json!({"reply":"invalid ciphertext"}))]);
    assert!(c.send_frame("123", &[0xaa]).await.is_err());
}

#[tokio::test]
async fn out_of_range_result_codes_cannot_wrap_into_success() {
    for code in [
        json!(4294967296_i64),
        json!("4294967296"),
        json!(-4294967296_i64),
    ] {
        let mut c = client(vec![raw(200, json!({"code":code,"data":{}}).to_string())]);
        assert!(c.post("/test", json!({})).await.is_err());
        complete(&c, 1);
    }
}
#[tokio::test]
async fn command_removes_transport_envelope() {
    let mut bytes = vec![0x5a_u8, 0x5a];
    bytes.resize(40, 0);
    bytes.extend_from_slice(&[0xaa, 3, 0xac, 0]);
    let csv = bytes
        .iter()
        .map(|b| (*b as i8).to_string())
        .collect::<Vec<_>>()
        .join(",");
    let s = session();
    let reply = encrypt(&csv, &s.data_key, &s.data_iv).unwrap();
    let mut c = authenticated(vec![ok(json!({"reply":reply}))]);
    assert_eq!(
        c.send_frame("123", &[0xaa]).await.unwrap(),
        Some(vec![0xaa, 3, 0xac, 0])
    );
    complete(&c, 1);
}

#[tokio::test]
async fn rejected_saved_token_renews_once_and_retries_inventory() {
    for final_code in [0, 40002] {
        let hash = hex::encode(Sha256::digest(APP_KEY.as_bytes()));
        let mut c = authenticated(vec![
            raw(200, json!({"code":40002}).to_string()),
            ok(json!({"masUrl":INITIAL_URL})),
            ok(json!({"loginId":"test-login"})),
            ok(json!({"uid":"test-user",
                "accessToken":encrypt("0123456789abcdef", &hash[..16], &hash[16..32]).unwrap(),
                "randomData":encrypt("fedcba9876543210", &hash[..16], &hash[16..32]).unwrap(),
                "mdata":{"accessToken":"renewed-token"}})),
            raw(
                200,
                json!({"code":final_code,"data":{"list":[]}}).to_string(),
            ),
        ]);
        let result = c
            .appliances_with_relogin("test@example.invalid", "test-password")
            .await;
        assert_eq!(result.is_ok(), final_code == 0);
        complete(&c, 5);
        let requests = &c.mock.as_ref().unwrap().requests;
        assert_eq!(requests[0].headers()["accessToken"], "synthetic-token");
        assert_eq!(requests[4].headers()["accessToken"], "renewed-token");
    }
}

#[tokio::test]
async fn inventory_service_failure_does_not_create_another_login() {
    let mut c = authenticated(vec![raw(503, "unavailable")]);
    assert!(
        c.appliances_with_relogin("test@example.invalid", "test-password")
            .await
            .is_err()
    );
    complete(&c, 1);
    assert_eq!(c.session.as_ref().unwrap().access_token, "synthetic-token");
}
