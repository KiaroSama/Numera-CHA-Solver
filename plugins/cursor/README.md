# Cursor IDE

Release0.4.1: verify the native package against the pinned owner self-signed key before replacement ([signing guide](../../docs/SIGNING.md)). Instructions and MCP installation are separate; self-signed does not establish public OS trust or notarization. Preserve private state and matching service/device configuration.

## Local instruction plugin

Copy the complete repository or CLI/agent package root to `~/.cursor/plugins/local/numera-cha-solver/` as a real directory, not this guide folder alone. Preserve modified files; team policy can restrict local imports. Reload Cursor, open **Customize**, verify the plugin/skill is enabled, then invoke `/numera-cha-solver` through supported skill discovery. JSON/cache presence does not prove loading.

Root portable metadata and `.cursor-plugin/plugin.json` expose the same canonical `skills/` payload. This instruction-only plugin neither installs binaries nor starts/configures MCP. A standalone skill is an alternative on versions supporting Agent Skills: copy the complete skill into the client's documented project directory rather than enabling duplicates.

## MCP connection

Install the verified0.4.1 native package. Default direct mode needs no separately started loopback service. Merge [cursor-mcp.json](../../examples/cursor-mcp.json) into project `.cursor/mcp.json` or the intended user scope. Set non-secret launching environment variables `NUMERA_MCP_COMMAND` (absolute executable), `NUMERA_ROOT` (absolute installation/state root), and `NUMERA_DEVICE` (`cuda` only with verified runtime; otherwise explicit `cpu`). The example uses native `${env:NAME}` interpolation, `args:["bridge"]` and port1583. The direct process uses the selected root/device; port1583 is used only with explicit `--remote`, whose separately managed server must match root/port/device.

Native plugin MCP supports `${CURSOR_PLUGIN_ROOT}` / `${CLAUDE_PLUGIN_ROOT}`; portable `${PLUGIN_ROOT}` / `${PLUGIN_DATA}` are not currently expanded. Do not add a falsely universal bundled MCP file or hidden autostart. Only explicit remote mode uses the owner's restricted HTTP credential.

## Verify and limits

Inspect Customize and discovered tools. Owned `12 * 3 = ?` should suggest `36`, `server_verified:false`. Missing service or permission refusal remains a blocker without changing provider/permissions. Image recognition needs actual weights/runtime execution; GPU inventory is insufficient. Agents create/read/cancel requested pending reviews, never confirm them.

Evidence is official format/interpolation and maintained Cursor source, not an installed Cursor runtime test. Public marketplace acceptance is not promised: its open-source eligibility and this custom noncommercial license differ. Local authorized installation does not publish to a marketplace.
