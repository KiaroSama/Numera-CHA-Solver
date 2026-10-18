use serde_json::Value;
use std::{fs, path::Path};

#[test]
fn public_clients_are_portable_and_the_skill_is_self_contained() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let skill = fs::read_to_string(root.join("skills/numera-cha-solver/SKILL.md")).unwrap();
    assert!(skill.starts_with("---\n") || skill.starts_with("---\r\n"));
    assert!(skill.contains("name: numera-cha-solver"));
    assert!(skill.contains("description: ") && skill.contains("license: "));
    for tool in [
        "capabilities",
        "models",
        "doctor",
        "recognize",
        "create_review",
        "get_review",
        "cancel_review",
    ] {
        assert!(
            skill.contains(&format!("| `{tool}` |")),
            "Missing callable contract: {tool}"
        );
    }
    for boundary in [
        "4096",
        "256",
        "128",
        "8MiB",
        "16million",
        "outcome unknown",
        "does not echo",
        "server_verified",
        "not idempotent",
    ] {
        assert!(
            skill.to_lowercase().contains(&boundary.to_lowercase()),
            "Missing skill boundary: {boundary}"
        );
    }
    assert!(
        !skill.contains("](../"),
        "Standalone skill must not depend on its former parent."
    );
    for path in [
        "README.md",
        "docs/MCP.md",
        "examples/claude_desktop_config.json",
        "examples/cursor-mcp.json",
        "examples/codex-config.toml",
    ] {
        let text = fs::read_to_string(root.join(path)).unwrap();
        assert!(
            !text.contains("G:") && !text.contains("Portable\\") && !text.contains("Users\\"),
            "Nonportable public configuration: {path}"
        );
    }
    for path in [
        "examples/claude_desktop_config.json",
        "examples/cursor-mcp.json",
    ] {
        let config: Value = serde_json::from_slice(&fs::read(root.join(path)).unwrap()).unwrap();
        let server = &config["mcpServers"]["Numera-CHA-Solver-MCP"];
        assert_eq!(server["args"], serde_json::json!(["bridge"]));
        assert_eq!(server["env"]["NUMERA_MCP_PORT"], "1583");
        assert!(server["env"].get("NUMERA_MCP_TOKEN").is_none());
    }
    for host in ["claude", "codex", "cursor"] {
        let guide = fs::read_to_string(root.join(format!("plugins/{host}/README.md"))).unwrap();
        assert!(
            guide.lines().count() > 15,
            "Client guide is an unimplemented stub."
        );
        assert!(guide.contains("bridge") && guide.contains("server_verified"));
    }
    let claude = fs::read_to_string(root.join("plugins/claude/README.md")).unwrap();
    assert!(claude.contains("--plugin-dir") && claude.contains("Desktop"));
    let codex = fs::read_to_string(root.join("plugins/codex/README.md")).unwrap();
    assert!(codex.contains("IDE") && codex.contains(".agents/skills/"));
    let cursor = fs::read_to_string(root.join("plugins/cursor/README.md")).unwrap();
    assert!(cursor.contains("${env:NAME}") || cursor.contains("${env:"));
}
