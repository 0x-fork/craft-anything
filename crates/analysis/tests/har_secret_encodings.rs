//! Regression tests for form-encoded and mixed-case percent-escaped secret redaction in HAR inspection.

use craft_anything_analysis::web_har::{HarOptions, all_strings, inspect_har};
use serde_json::json;
use std::collections::BTreeSet;

/// Standard credential headers and exact literal secrets continue to be scrubbed.
#[test]
fn exact_literal_and_auth_header_redaction_works() {
    let har = serde_json::to_vec(&json!({
        "log": {
            "version": "1.2",
            "entries": [{
                "request": {
                    "method": "GET",
                    "url": "https://api.example.test/v1/ping?note=PlainSecret",
                    "headers": [{"name": "Authorization", "value": "Bearer tok"}]
                },
                "response": { "status": 200, "headers": [], "content": { "size": 0, "mimeType": "" } }
            }]
        }
    }))
    .unwrap();
    let out = inspect_har(&har, &HarOptions { hidden_values: vec!["PlainSecret".into()], limit: 10, max_entries: 100, ..Default::default() }).unwrap();
    assert_eq!(out["entries"][0]["url"], "https://api.example.test/v1/ping?note=[redacted]");
    assert_eq!(out["entries"][0]["request_headers"][0]["value"], serde_json::Value::Null);
}

/// `application/x-www-form-urlencoded` (`+` plus `%XX`) and mixed-case `%xx` hex escapes must be redacted everywhere.
#[test]
fn form_urlencoded_and_mixed_case_percent_escaped_secrets_never_leak() {
    // `URLSearchParams({ auth: "Bearer tok==" })` serializes space as `+` and `=` as `%3D`.
    // Many clients also emit lowercase hex escapes (`%2f`, `%3d`) while preserving mixed-case ASCII letters.
    let har = serde_json::to_vec(&json!({
        "log": {
            "version": "1.2",
            "entries": [{
                "request": {
                    "method": "POST",
                    "url": "https://api.example.test/v1/callback?state=Bearer+tok%3D%3D&next=MySecret%2f1&upper=MYSECRET/1",
                    "headers": [
                        {"name": "X-Custom-Trace", "value": "hint=Bearer+tok%3d%3d; path=MySecret%2f1"},
                        {"name": "Referer", "value": "https://app.example.test/cb?s=MySecret%2f1"}
                    ],
                    "postData": {
                        "mimeType": "application/x-www-form-urlencoded",
                        "text": "safe_field=1&MySecret%2f1=val&Bearer+tok%3D%3D=2"
                    }
                },
                "response": {
                    "status": 200,
                    "headers": [],
                    "content": {
                        "size": 32,
                        "mimeType": "application/json",
                        "text": "{\"ok\":true,\"MYSECRET/1\":\"v\"}"
                    }
                }
            }]
        }
    }))
    .unwrap();

    let out =
        inspect_har(&har, &HarOptions { hidden_values: vec!["Bearer tok==".into(), "MySecret/1".into()], limit: 10, max_entries: 100, ..Default::default() })
            .unwrap();

    let mut strings = BTreeSet::new();
    all_strings(&out, &mut strings);
    for s in &strings {
        let low = s.to_ascii_lowercase();
        for forbidden in ["tok==", "tok%3d%3d", "mysecret/1", "mysecret%2f1"] {
            assert!(!low.contains(forbidden), "secret spelling `{forbidden}` leaked in `{s}`");
        }
    }

    assert_eq!(out["entries"][0]["request_body"]["fields"], json!(["safe_field"]));
    assert!(
        out["redactions"].as_array().unwrap().iter().any(|r| r["scope"] == "form-field"),
        "dropping secret form field names should record a redaction entry: {}",
        out["redactions"]
    );
}

/// Multipart `params` field names carrying percent-encoded or uppercase spellings of declared secrets are dropped and logged.
#[test]
fn multipart_encoded_secret_field_names_are_filtered_and_logged() {
    let har = serde_json::to_vec(&json!({
        "log": {
            "version": "1.2",
            "entries": [{
                "request": {
                    "method": "POST",
                    "url": "https://api.example.test/v1/upload",
                    "headers": [],
                    "postData": {
                        "mimeType": "multipart/form-data; boundary=---x",
                        "params": [
                            {"name": "avatar", "value": "file"},
                            {"name": "Key%20One", "value": "x"},
                            {"name": "KEY ONE", "value": "y"}
                        ]
                    }
                },
                "response": { "status": 200, "headers": [], "content": { "size": 0, "mimeType": "" } }
            }]
        }
    }))
    .unwrap();

    let out = inspect_har(&har, &HarOptions { hidden_values: vec!["Key One".into()], limit: 10, max_entries: 100, ..Default::default() }).unwrap();
    assert_eq!(out["entries"][0]["request_body"]["fields"], json!(["avatar"]));
    assert!(out["redactions"].as_array().unwrap().iter().any(|r| r["scope"] == "form-field"));
}
