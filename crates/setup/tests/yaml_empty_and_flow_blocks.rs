//! Regressions for Hermes YAML setup edits with spaced empty braces, YAML 1.2 null case
//! variants, and multi-line flow child blocks inside `mcp_servers`.

use craft_anything_setup::{Host, PlanRequest, apply, config, plan, uninstall, yaml};
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

/// Replacing or removing a multi-line flow mapping child whose closing `}` sits at the child's
/// own indent must remove the entire flow collection without leaving a dangling `}`.
#[test]
fn yaml_set_and_remove_handle_multiline_flow_child_blocks() {
    let text = "\
# Hermes config
model: big
mcp_servers:
  craft-anything: {
    command: \"/old/craft-anything-cli\",
    note: \"keep } # inside string\",
    args: [
      \"mcp\"
    ]
  }
  other:
    command: npx
";
    let entry = json!({"command": "/new/craft-anything-cli", "args": ["mcp"], "enabled": true});
    let replaced = yaml::set(text, "mcp_servers", "craft-anything", &entry).expect("replace multi-line flow child");
    let parsed = yaml::parse(&replaced).expect("replaced YAML parses");
    assert_eq!(parsed["mcp_servers"]["craft-anything"], entry);
    assert_eq!(parsed["mcp_servers"]["other"]["command"], "npx");
    assert!(replaced.contains("# Hermes config"));

    let removed = yaml::remove(text, "mcp_servers", "craft-anything").expect("remove multi-line flow child");
    let parsed_after_remove = yaml::parse(&removed).expect("YAML parses after removal");
    assert!(parsed_after_remove["mcp_servers"].get("craft-anything").is_none());
    assert_eq!(parsed_after_remove["mcp_servers"]["other"]["command"], "npx");
}

/// A sibling entry using multi-line flow syntax before `craft-anything`, plus single-quoted
/// braces/hashes inside `craft-anything`'s flow block, keeps the sibling intact while cleanly
/// replacing or removing `craft-anything`.
#[test]
fn yaml_preserves_multiline_flow_sibling_and_handles_single_quoted_braces() {
    let text = "\
mcp_servers:
  other: {
    command: \"npx\",
    args: [\"a\"]
  }
  craft-anything: {
    note: 'don''t } # close early',
    command: \"/old/craft-anything-cli\",
    args: [\"mcp\"]
  }
";
    let entry = json!({"command": "/new/craft-anything-cli", "args": ["mcp"], "enabled": true});
    let updated = yaml::set(text, "mcp_servers", "craft-anything", &entry).expect("replace after flow sibling");
    let parsed = yaml::parse(&updated).expect("valid YAML after replace");
    assert_eq!(parsed["mcp_servers"]["other"]["command"], "npx");
    assert_eq!(parsed["mcp_servers"]["craft-anything"], entry);

    let removed = yaml::remove(text, "mcp_servers", "craft-anything").expect("remove after flow sibling");
    let parsed_removed = yaml::parse(&removed).expect("valid YAML after remove");
    assert_eq!(parsed_removed["mcp_servers"]["other"]["command"], "npx");
    assert!(parsed_removed["mcp_servers"].get("craft-anything").is_none());
}

/// End-to-end Hermes `plan`, `apply`, and `uninstall` succeed starting from `mcp_servers: { }`
/// and after switching our entry to multi-line flow style.
#[test]
fn hermes_setup_and_uninstall_work_with_spaced_braces_and_multiline_flow_entry() {
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

    let req = PlanRequest { clients: vec!["hermes".into()], all_detected: false, command: cmd.clone(), include_skill: false };
    let p = plan(&host, &req).unwrap();
    assert!(!p.blocked, "{:?}", p.actions);
    assert_eq!(apply(&p).status, "configured");

    // Rewrite our entry as multi-line flow (e.g. edited by hand with an outdated path), then refresh and uninstall.
    let flow_cfg = format!(
        "model: big\nmcp_servers:\n  craft-anything: {{\n    command: {},\n    args: [\"mcp\"],\n    enabled: true\n  }}\n",
        json!(dir.path().join("old-cli"))
    );
    std::fs::write(&cfg, flow_cfg).unwrap();
    let hermes_client = craft_anything_setup::clients(&host).into_iter().find(|c| c.id == "hermes").unwrap();
    assert!(config::read_entry(&hermes_client, &std::fs::read_to_string(&cfg).unwrap()).unwrap().is_some());

    let p2 = plan(&host, &req).unwrap();
    assert!(!p2.blocked, "{:?}", p2.actions);
    assert_eq!(apply(&p2).status, "configured");

    let u = uninstall(&host);
    assert_eq!(u.status, "removed");
    let final_text = std::fs::read_to_string(&cfg).unwrap();
    assert!(config::read_entry(&hermes_client, &final_text).unwrap().is_none());
}
