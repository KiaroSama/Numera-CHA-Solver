# Claude Code and Claude Desktop

Release0.4.2: verify the native package against the pinned owner self-signed key before replacement ([signing guide](../../docs/SIGNING.md)). Instructions and MCP installation are separate; self-signed does not establish public OS trust or notarization. Preserve private state and matching service/device configuration.

## Claude Code: instruction plugin

Use the complete repository or CLI/agent package root, containing `.claude-plugin/plugin.json` and `skills/numera-cha-solver/SKILL.md`. This documentation subdirectory is not a separate plugin. Nothing here registers MCP or grants permissions.

### Install from the repository marketplace

Register the repository, not this documentation subdirectory:

```bash
claude plugin marketplace add KiaroSama/Numera-CHA-Solver
```

Install the plugin by its name and the catalog's declared marketplace name:

```bash
claude plugin install numera-cha-solver@numera-cha-solver-marketplace
```

The repository's `.claude-plugin/marketplace.json` lists the existing root plugin with `source: "./"`; `.claude-plugin/plugin.json` describes that plugin. Both are needed for this marketplace route. Adding a plugin-only repository as a marketplace fails with "no manifest found at .claude-plugin/marketplace.json". These commands change the selected client's marketplace/plugin configuration; they are user-run steps, not automatic setup. They do not start or register MCP.

This catalog is available in the updated source checkout. Previously published archives are immutable and may lack it; use the repository route or direct loading below rather than assuming an older archive has marketplace support.

### Load only for one session

Session-local loading, with your actual absolute path:

```bash
claude --plugin-dir /absolute/path/to/numera-cha-solver
```

After accepting project trust, invoke `/numera-cha-solver:numera-cha-solver`. Verify the displayed skill is the local recognition/human-handoff workflow. Cache presence or enabled metadata alone is not proof of loading. Restart only the relevant session if discovery is stale; do not patch installed caches.

Native structural validation (requires a supporting Claude Code version):

```bash
claude plugin validate --strict /absolute/path/to/numera-cha-solver
```

Alternatively, copy the complete `skills/numera-cha-solver/` folder into the project's `.claude/skills/numera-cha-solver/` with owner approval. Standalone invocation is `/numera-cha-solver`, not the plugin-qualified key. Avoid duplicate installations.

## Separate MCP connection

Install the verified0.4.2 native executable and merge the [connection configuration](../../docs/MCP.md) into the intended client scope. Preserve `Numera-CHA-Solver-MCP`, `args:["bridge"]` and the selected root/device. The client owns the local runtime directly; no separate HTTP server or token is required. Explicit `["bridge","--remote"]` retains the separately managed authenticated HTTP relay. No token belongs in public configuration.

This plugin is instruction-only. A bundled MCP server would automatically launch a bridge and Claude scopes its name as `plugin:<plugin>:<server>`, rather than the existing bare identity. No hidden autostart or broad tool allowlist is installed.

## Desktop / Cowork / cloud

`--plugin-dir` is a Claude Code option, not a Desktop Chat installer. Desktop Chat uses its separate `claude_desktop_config.json`. Cowork/account-synced catalog installation is not established by local folders. A cloud client cannot be assumed to reach workstation loopback.

## Verify

Discover the selected connection's seven tools. Owned arithmetic `12 * 3 = ?` should suggest `36`, with `server_verified:false`. Default direct mode needs no service; an unavailable explicit remote service remains a blocker, not permission to switch modes or providers. Only requested pending reviews are created; the agent cannot confirm them.

Evidence distinguishes manifest validation, actual skill invocation, MCP operation and permission behavior. This guide does not claim runtime verification on every Claude surface.
