# Local MCP connection

Registration name: **Numera-CHA-Solver-MCP**. The server and bridge are native Rust. They expose licensed local perception and explicit human handoff, not a universal CAPTCHA solver or third-party protection bypass.

## Start the selected server

Extract the inspected native archive for your OS. The binary is `bin/numera-cha-solver-mcp.exe` on Windows or `bin/numera-cha-solver-mcp` on Linux/macOS. Start it manually:

```powershell
$env:NUMERA_ROOT = (Get-Location).Path; $env:NUMERA_DEVICE = 'cpu'; ./bin/numera-cha-solver-mcp.exe serve --port 1583
```

```bash
NUMERA_ROOT="$PWD" NUMERA_DEVICE=cpu ./bin/numera-cha-solver-mcp serve --port 1583
```

The endpoint is `http://127.0.0.1:1583/mcp`. Change `--port` for the server and set the same `NUMERA_MCP_PORT` for bridges. Only loopback is accepted; no arbitrary URL, proxy or alternate service is supported.

### CUDA instead of the CPU package default

The public CPU example matches the CPU-only runtime shipped in the cross-platform archive; it is not a recommendation to ignore an available GPU. CUDA needs a complete matching GPU runtime (core/shared/CUDA-provider libraries) and compatible CUDA/cuDNN dependencies, not just a changed environment string. Never mix CPU core libraries with stale GPU providers. macOS has no CUDA path.

On a verified Windows GPU installation, configure the server process before starting it:

```powershell
$env:NUMERA_ROOT = (Get-Location).Path
$env:NUMERA_DEVICE = 'cuda'
$env:NUMERA_ORT_LIBRARY = Join-Path $env:NUMERA_ROOT 'data/runtime/onnxruntime.dll'
$env:NUMERA_DLL_DIRECTORY = '/absolute/path/to/compatible-cuda-cudnn-libraries'
./bin/numera-cha-solver-mcp.exe serve --port 1583
```

Use the same root/port and `NUMERA_DEVICE:"cuda"` in your private bridge configuration. **The server owns the device**: a bridge-only environment edit cannot change an already-running server. `doctor` reports configured inventory, not executed inference; actual image recognition must return CUDA device/profile evidence. Explicit CUDA failure remains a blocker, never a silent CPU fallback. Check compatible versions in [GPU setup](GPU.md).

On successful startup, the owner-restricted `data/mcp-token.txt` credential is created if absent, without printing its value. `NUMERA_MCP_TOKEN` can select an existing secret through an approved private environment. Keep credentials out of public config examples, command arguments, screenshots and logs. Reuse is local and does not grant other users access.

## Claude Desktop configuration

Merge this entry into `claude_desktop_config.json`; preserve all other entries. Replace the example installation path with your actual extracted archive path. The exact argument is `bridge`:

```json
{
  "mcpServers": {
    "Numera-CHA-Solver-MCP": {
      "command": "/absolute/path/to/numera-cha-solver/bin/numera-cha-solver-mcp",
      "args": ["bridge"],
      "env": {
        "NUMERA_ROOT": "/absolute/path/to/numera-cha-solver",
        "NUMERA_MCP_PORT": "1583",
        "NUMERA_DEVICE": "cpu"
      }
    }
  }
}
```

No token is copied into this example: the bridge reads the restricted credential for that root. The neutral example uses a Unix-style placeholder. On Windows replace it with your actual absolute path including `.exe`; Linux/macOS use the binary without that suffix. The server must already be running; the bridge never starts an alternate endpoint. stdout contains only MCP messages, with diagnostics on stderr and in per-run local logs.

## Claude Code and Cursor

Claude Code's project `.mcp.json` uses `mcpServers`; merge the explicit stdio entry after replacing paths. Cursor's project `.cursor/mcp.json` uses [its native interpolation example](../examples/cursor-mcp.json): set `NUMERA_MCP_COMMAND`, `NUMERA_ROOT` and `NUMERA_DEVICE` in the launching environment first. `${env:NAME}` is a Cursor contract, not a universal JSON feature. Install the binary, start the selected service, and reload only the intended client. No automatic client configuration or permissions are installed. See the separate [Claude](../plugins/claude/README.md), [Codex](../plugins/codex/README.md) and [Cursor](../plugins/cursor/README.md) loading guides.

## Codex

Merge into your Codex configuration using the native TOML server form:

```toml
[mcp_servers.Numera-CHA-Solver-MCP]
command = "/absolute/installation/bin/numera-cha-solver-mcp"
args = ["bridge"]

[mcp_servers.Numera-CHA-Solver-MCP.env]
NUMERA_ROOT = "/absolute/installation"
NUMERA_MCP_PORT = "1583"
NUMERA_DEVICE = "cpu"
```

Windows uses the `.exe` path. Other Agent Skills/MCP clients use their documented stdio configuration; a portable skill is not proof every client has the same native plugin system.

## Tools and human review

`capabilities`, `models`, `doctor`, `recognize`, `create_review`, `get_review`, and `cancel_review` return actual local evidence or actionable blockers. Image input is base64, bounded to8MiB decoded; text and answer limits are4096 Unicode scalars. Unknown fields and remote browser input are rejected.

A recognition answer remains a suggestion. A pending review is bound to its exact observation revision. The person corrects/confirms/cancels in the authenticated local dashboard or desktop workspace. There is no agent confirmation/submission/token-forgery tool, and manual attestation never sets `server_verified`. This is a workflow boundary, not cryptographic proof of a human against an agent already controlling the same OS account.

## Troubleshooting

- `mcp_unavailable`: start the selected local server; verify the same root and port. Do not silently switch providers or endpoints.
- Port collision: choose another free loopback port for server and bridge; a failed contender cannot replace the active credential.
- Missing model/runtime: inspect `models` and `doctor`; acquire reviewed assets only through the existing explicit model workflow.
- Invalid credential: preserve the current instance and check its owner-protected file privately. Never paste a token into an issue.
- Unsupported provider/live challenge: stop for a person or use the service's official API; generic OCR does not prove real CAPTCHA accuracy.

See [desktop delivery](DESKTOP.md), [license](LICENSING.md), and [verification](../reports/VALIDATION.md). Current client examples are structurally validated in CI; real client UI installation is not implied by a JSON file alone.
