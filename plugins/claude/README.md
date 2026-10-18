# Claude Code and Claude Desktop

## Claude Code: instruction plugin

Use the complete repository or CLI/agent package root, containing `.claude-plugin/plugin.json` and `skills/numera-cha-solver/SKILL.md`. This documentation subdirectory is not a separate plugin. Nothing here registers MCP or grants permissions.

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

Install the verified native executable, manually start the selected service and merge the [bridge configuration](../../docs/MCP.md) into the intended client scope. Preserve `Numera-CHA-Solver-MCP`, `args:["bridge"]`, matching root/port and protected credential. No token belongs in public configuration.

This plugin is instruction-only. A bundled MCP server would automatically launch a bridge and Claude scopes its name as `plugin:<plugin>:<server>`, rather than the existing bare identity. No hidden autostart or broad tool allowlist is installed.

## Desktop / Cowork / cloud

`--plugin-dir` is a Claude Code option, not a Desktop Chat installer. Desktop Chat uses its separate `claude_desktop_config.json`. Cowork/account-synced catalog installation is not established by local folders. A cloud client cannot be assumed to reach workstation loopback.

## Verify

Discover the selected connection's seven tools. Owned arithmetic `12 * 3 = ?` should suggest `36`, with `server_verified:false`. An absent service must report a blocker, not switch providers. Only requested pending reviews are created; the agent cannot confirm them.

Evidence distinguishes manifest validation, actual skill invocation, MCP operation and permission behavior. This guide does not claim runtime verification on every Claude surface.
