# Agent instructions and plugin packaging

The canonical original instruction is [numera-cha-solver](../skills/numera-cha-solver/SKILL.md). It describes licensed local inputs, live capability inspection, bounded recognition, manual review and honest blockers. Instructions never supply a token, install a binary or authorize an external action.

The CLI/agent archive includes the project-root `plugin.json`, `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, and `.cursor-plugin/plugin.json`, beside the same canonical `skills/` payload. The portable root manifest targets Agent Plugins1.0.0 with its required schema identifier and fixed `skills/` discovery; the other manifests retain client-native compatibility metadata. Native discovery varies with client/version. Hosted structure/archive checks are not proof that each client was installed or exercised.

For clients supporting Agent Skills, copy only `skills/numera-cha-solver/` into that client's documented skill directory with the owner's approval. Clients without native plugin support can use the standalone instruction. Do not overwrite modified client configuration or install these instructions automatically.

MCP installation is separate: extract the verified native platform archive, start the selected loopback server, and merge the [stdio bridge example](../docs/MCP.md). Claude Desktop, Claude Code, Codex and Cursor examples retain the exact registration name `Numera-CHA-Solver-MCP` and `bridge` argument. Loading the skill alone does not connect the service.

No instructions or GPL source from the referenced owner projects were copied. This project follows their packaging convention through independently authored content, keeping each component's original license separate.
