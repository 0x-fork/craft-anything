//! Regressions for Hermes YAML setup edits with spaced empty braces, YAML 1.2 null case
//! variants, safe refusal of flow-style entries under `mcp_servers`, and preservation of sibling
//! entries next to block scalars or quoted strings containing `{`.

use craft_anything_setup::{ActionKind, Host, PlanRequest, apply, config, plan, uninstall, yaml};
use serde_json::json;

/// YAML 1.2 empty flow mappings with spaces (`{ }`) and case-variant nulls (`Null`, `NULL`)
/// parse as empty mappings and should be editable by `yaml::set`.
#[test]
fn yaml_set_accepts_spaced_empty_braces_and_case_variant_nulls() {
    let entry = json!({"command": "/opt/craft-anything-cli", "args": ["mcp"], "enabled": true});
    for (label, text) in [
        ("spaced braces", "model: big\nmcp_servers: { }\n"),
        ("spaced braces with comment", "model: big\nmcp_servers: {   } # empty for now\n"),
        ("titlecase Null", "model: big\nmcp_servers: Null\n"),
        ("uppercase NULL", "model: big\nmcp_servers: NULL # none yet\n"),
        ("child spaced braces", "model: big\nmcp_servers:\n  craft-anything: {   }\n"),
        ("child titlecase Null", "model: big\nmcp_servers:\n  craft-anything: Null\n"),
    ] {
        let updated = yaml::set(text, "mcp_servers", "craft-anything", &entry).unwrap_or_else(|e| panic!("{label}: expected yaml::set to succeed, got: {e}"));
        let parsed = yaml::parse(&updated).expect("updated YAML parses");
        assert_eq!(parsed["mcp_servers"]["craft-anything"], entry, "{label}");
        assert_eq!(parsed["model"], "big", "{label}");
        if text.contains('#') {
            assert!(updated.contains('#'), "{label} lost inline comment:\n{updated}");
        }
    }
}

/// Flow-style entries under `mcp_servers` (whether multi-line, single-line with plain apostrophes
/// such as `don't`, preceded by a flow sibling, or nested with a closing `}` at child indent)
/// must be refused with an actionable manual-edit error instead of guessing boundaries or deleting
/// sibling configurations.
#[test]
fn yaml_refuses_flow_children_without_guessing_boundaries() {
    let entry = json!({"command": "/new/craft-anything-cli", "args": ["mcp"], "enabled": true});
    let cases = [
        (
            "multiline flow child",
            "model: big\nmcp_servers:\n  craft-anything: {\n    command: \"/old/craft-anything-cli\",\n    args: [\"mcp\"]\n  }\n  other:\n    command: npx\n",
        ),
        ("flow sibling before target", "mcp_servers:\n  other: {\n    command: \"npx\",\n    args: [\"a\"]\n  }\n  craft-anything:\n    command: old\n"),
        ("single-line flow with plain apostrophe", "mcp_servers:\n  craft-anything: {command: old, note: don't}\n  other:\n    command: npx\n"),
        ("multiline flow array child", "mcp_servers:\n  craft-anything: [\n    \"item\"\n    ]\n  other:\n    command: npx\n"),
    ];

    for (label, text) in cases {
        let set_err = yaml::set(text, "mcp_servers", "craft-anything", &entry).expect_err(&format!("{label}: set should refuse flow child"));
        assert!(set_err.contains("by hand"), "{label}: unexpected error: {set_err}");

        let try_rm_err = yaml::try_remove(text, "mcp_servers", "craft-anything").expect_err(&format!("{label}: try_remove should refuse flow child"));
        assert!(try_rm_err.contains("by hand"), "{label}: unexpected try_remove error: {try_rm_err}");

        assert!(yaml::remove(text, "mcp_servers", "craft-anything").is_none(), "{label}: remove should return None");
    }
}

/// Block mappings containing braces inside multi-line quoted strings, block scalars (`|`), or
/// plain scalars (`prefix{suffix`) are delimited safely by indentation alone so updating or
/// removing `craft-anything` always preserves sibling entries like `other`.
#[test]
fn block_scalars_and_quoted_or_plain_braces_preserve_siblings() {
    let entry = json!({"command": "/new/craft-anything-cli", "args": ["mcp"], "enabled": true});
    let cases = [
        (
            "multiline double-quoted brace",
            "mcp_servers:\n  craft-anything:\n    command: old\n    note: \"first\n      { second\"\n  other:\n    command: npx\n",
        ),
        ("literal block scalar brace", "mcp_servers:\n  craft-anything:\n    command: old\n    note: |\n      {\n  other:\n    command: npx\n"),
        ("plain scalar containing brace", "mcp_servers:\n  craft-anything:\n    command: old\n    note: prefix{suffix\n  other:\n    command: npx\n"),
        (
            "nested flow mapping inside block child",
            "mcp_servers:\n  craft-anything:\n    command: old\n    env: {\n      FOO: \"1\"\n    }\n  other:\n    command: npx\n",
        ),
    ];

    for (label, text) in cases {
        let updated = yaml::set(text, "mcp_servers", "craft-anything", &entry).unwrap_or_else(|e| panic!("{label}: set failed: {e}"));
        let parsed = yaml::parse(&updated).expect("updated YAML parses");
        assert_eq!(parsed["mcp_servers"]["craft-anything"], entry, "{label}");
        assert_eq!(parsed["mcp_servers"]["other"]["command"], "npx", "{label}: lost other on set");

        let removed =
            yaml::try_remove(text, "mcp_servers", "craft-anything").unwrap_or_else(|e| panic!("{label}: try_remove failed: {e}")).expect("entry removed");
        let parsed_removed = yaml::parse(&removed).expect("removed YAML parses");
        assert!(parsed_removed["mcp_servers"].get("craft-anything").is_none(), "{label}");
        assert_eq!(parsed_removed["mcp_servers"]["other"]["command"], "npx", "{label}: lost other on remove");
    }
}

/// End-to-end Hermes `plan`, `apply`, and `uninstall` succeed starting from `mcp_servers: { }`,
/// and refuse to overwrite or delete when the user edits their entry into multi-line flow style.
#[test]
fn hermes_setup_works_with_spaced_braces_and_refuses_multiline_flow_entry() {
    let dir = tempfile::tempdir().unwrap();
    let hermes_dir = dir.path().join("hermes");
    std::fs::create_dir_all(&hermes_dir).unwrap();
    let cfg = hermes_dir.join("config.yaml");
    std::fs::write(&cfg, "model: big\nmcp_servers: { } # configured empty\n").unwrap();

    let mut host = Host::with_home(dir.path());
    host.env.insert("HERMES_HOME".into(), hermes_dir.display().to_string());
    let cli = dir.path().join("craft-anything-cli");
    std::fs::write(&cli, "").unwrap();
    let cmd = vec![cli.display().to_string(), "mcp".into()];

    let req = PlanRequest { clients: vec!["hermes".into()], all_detected: false, command: cmd, include_skill: false };
    let p = plan(&host, &req).unwrap();
    assert!(!p.blocked, "{:?}", p.actions);
    assert_eq!(apply(&p).status, "configured");

    // Rewrite our entry as multi-line flow alongside a sibling, then verify plan/uninstall refuse without altering the file.
    let stale_cli = dir.path().join("old").join("craft-anything-cli");
    let flow_cfg = format!(
        "model: big\nmcp_servers:\n  craft-anything: {{\n    command: {},\n    args: [\"mcp\"],\n    enabled: true\n  }}\n  other:\n    command: npx\n",
        json!(stale_cli)
    );
    std::fs::write(&cfg, &flow_cfg).unwrap();
    let hermes_client = craft_anything_setup::clients(&host).into_iter().find(|c| c.id == "hermes").unwrap();
    assert!(config::read_entry(&hermes_client, &std::fs::read_to_string(&cfg).unwrap()).unwrap().is_some());

    let p2 = plan(&host, &req).unwrap();
    assert_eq!(p2.actions.len(), 1);
    assert_eq!(p2.actions[0].action, ActionKind::Invalid);
    assert!(p2.actions[0].reason.as_deref().is_some_and(|r| r.contains("by hand")), "{:?}", p2.actions[0].reason);
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), flow_cfg);

    let u = uninstall(&host);
    assert_eq!(u.status, "blocked");
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), flow_cfg);
}
