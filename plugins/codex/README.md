# Codex CLI and IDE

Release0.4.1: verify the native package against the pinned owner self-signed key before replacement ([signing guide](../../docs/SIGNING.md)). Instructions and MCP installation are separate; self-signed does not establish public OS trust or notarization. Preserve private state and matching service/device configuration.

## Codex CLI plugin

Use the complete repository/CLI archive root, not this guide directory. Root `plugin.json` targets Agent Plugins1.0.0 with fixed `skills/` discovery. `.codex-plugin/plugin.json` is compatibility metadata, not a second payload or MCP registration.

Use the current CLI's configured local/repository marketplace and `/plugins` interface to select/enable the package. Start a new session, inspect discovered skills, and invoke the displayed `numera-cha-solver` skill. A folder placed beside a project is not necessarily installed. Do not guess a Claude-qualified key. Marketplace addition, trust and enablement remain owner-controlled.

Portable loading uses fixed `skills/` and optional `mcp.json`. With root `plugin.json`, adding `skills` or `mcpServers` to the compatibility overlay does not override those paths. Numera deliberately keeps MCP explicit/separate.

[Official packaging](https://developers.openai.com/codex/plugins/build) defines the installed CLI's marketplace format. Standalone skills need no marketplace publication or account change.

## Standalone skill / IDE

The IDE extension does not support CLI plugins. Copy the complete `skills/numera-cha-solver/` folder to the project's `.agents/skills/numera-cha-solver/` with owner approval. Use `$numera-cha-solver` or the supported discovered skills UI; restart its session if discovery is stale. The skill contains its workflow/tool contracts without dependencies on neighboring project documents.

Configure MCP separately with [native TOML](../../examples/codex-config.toml) and [MCP setup](../../docs/MCP.md). Replace paths; keep `args = ["bridge"]` and exact `Numera-CHA-Solver-MCP` key. Credentials never belong in public TOML. Project configuration may need trust; loading instructions grants no tool permission.

## Verify and limits

Discover the seven tools. Owned `12 * 3 = ?` should suggest `36`, `server_verified:false`. Default direct mode needs no separate service. Missing models, permission refusal or an absent explicit remote service remain blockers, not permission to change providers. Retain review IDs/revisions; only the person confirms.

Official packaging and stable0.163.0 loader were inspected. Native CLI loading, IDE invocation and permission behavior are separate runtime evidence, not implied by JSON acceptance. Cloud/account catalogs are distinct from local installation and cannot be assumed to reach localhost.
