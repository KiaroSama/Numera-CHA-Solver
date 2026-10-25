# Agent instructions and plugin packaging

The canonical original instruction is [numera-cha-solver](../skills/numera-cha-solver/SKILL.md). It describes licensed local inputs, live capability inspection, bounded recognition, manual review and honest blockers. Instructions never supply a token, install a binary or authorize an external action.

The CLI/agent packaging includes the project-root `plugin.json`, `.claude-plugin/plugin.json`, `.claude-plugin/marketplace.json`, `.codex-plugin/plugin.json`, and `.cursor-plugin/plugin.json`, beside the same canonical `skills/` payload. The Claude marketplace catalog points to the existing root plugin with `source: "./"`; it is distinct from the plugin manifest. Previously published archives are not retroactively updated. The portable root manifest targets Agent Plugins1.0.0 with its required schema identifier and fixed `skills/` discovery; the other manifests retain client-native compatibility metadata. Native discovery varies with client/version. Use the distinct [Claude Code/Desktop](claude/README.md), [Codex CLI/IDE](codex/README.md) and [Cursor](cursor/README.md) guides. Hosted structure/archive checks are not proof that each client was installed or exercised.

For clients supporting Agent Skills, copy only `skills/numera-cha-solver/` into that client's documented skill directory with the owner's approval. Clients without native plugin support can use the standalone instruction. Do not overwrite modified client configuration or install these instructions automatically.

MCP installation is separate: extract the verified0.4.2 native archive and merge the [stdio configuration](../docs/MCP.md). The default client-owned process runs directly without a separate server; only explicit `--remote` relays to a manually managed authenticated loopback instance. Claude Desktop, Claude Code, Codex and Cursor examples retain the exact registration name `Numera-CHA-Solver-MCP` and `bridge` argument. Loading the skill alone does not connect the service.

| Client surface | Delivered route | Verification boundary |
|---|---|---|
| Claude Code | Native instruction plugin or standalone skill; separate MCP | Installed native strict manifest validator passed; actual skill invocation is separate. |
| Claude Desktop Chat | Explicit stdio bridge configuration | Not a Claude Code plugin installer; no client account/config mutation performed. |
| Codex CLI / IDE | CLI marketplace plugin / IDE standalone Agent Skills; separate MCP | Official stable loader inspected; these clients were not installed or invoked here. |
| Cursor IDE | Complete local instruction plugin or standalone skill; native MCP interpolation | Official format inspected; no Cursor runtime or public marketplace acceptance claimed. |
| Other agents | Self-contained skill and host-supported transport | Structure only; native plugin/runtime compatibility must be established per host. |

No instructions or GPL source from the referenced owner projects were copied. This project follows their packaging convention through independently authored content, keeping each component's original license separate.
