# Agent instructions and plugin packaging

The canonical original instruction is [numera-cha-solver](../skills/numera-cha-solver/SKILL.md). It describes licensed local inputs, live capability inspection, bounded recognition, manual review and honest blockers. Instructions never supply a token, install a binary or authorize an external action.

Compatibility metadata lives in the project-root `plugin.json`, `.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`, and `.cursor-plugin/plugin.json`. Native plugin discovery varies with the client/version; these are compatibility payloads, not proof that every client was exercised. Each manifest references the same `skills/` directory instead of duplicating instructions.

For clients supporting Agent Skills, copy only `skills/numera-cha-solver/` into that client's documented skill directory with the owner's approval. Clients without native plugin support can use the standalone instruction. Do not overwrite modified client configuration or install these instructions automatically.

MCP installation is separate: extract the verified native platform archive, start the selected loopback server, and merge the [stdio bridge example](../docs/MCP.md). Claude Desktop, Claude Code, Codex and Cursor examples retain the exact registration name `Numera-CHA-Solver-MCP` and `bridge` argument. Loading the skill alone does not connect the service.

No instructions or GPL source from the referenced owner projects were copied. This project follows their packaging convention through independently authored content, keeping each component's original license separate.
