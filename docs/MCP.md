# Local MCP connection

Registration name: **Numera-CHA-Solver-MCP**. Native Rust tools expose licensed local recognition and human handoff, not third-party protection bypass.

## Default: client-owned local execution

From0.4.1, the client starts `bin/numera-cha-solver-mcp.exe bridge` on Windows (`bin/numera-cha-solver-mcp bridge` on Linux/macOS). This process directly uses the shared local SDK. **No separately started HTTP server, port1583 listener or MCP token is required.** Closing the client connection drains the owned runtime. Nothing is installed in Windows Startup, scheduled tasks or system services.

Merge the following entry into the intended client's configuration, preserving other entries. Replace paths with the actual installation; Windows uses `.exe`:

```json
{
  "mcpServers": {
    "Numera-CHA-Solver-MCP": {
      "command": "/absolute/path/to/numera-cha-solver/bin/numera-cha-solver-mcp",
      "args": ["bridge"],
      "env": {
        "NUMERA_ROOT": "/absolute/path/to/numera-cha-solver",
        "NUMERA_DEVICE": "cpu"
      }
    }
  }
}
```

Claude Desktop uses `claude_desktop_config.json`; Claude Code uses its supported MCP scope. Installing the instruction plugin is separate from configuring the executable. Existing `args:["bridge"]` configurations need no argument change after the binary update. Older0.4.0 binaries still require the separately managed server; updating only instructions cannot change them.

Direct clients share persisted reviews under the same root; their recognition runtimes and admission capacity are per process. A person uses the local dashboard/desktop with that same root to correct or confirm pending reviews. No agent tool can confirm them.

## Device and runtime

The distributed runtime is CPU-only: set `NUMERA_DEVICE=cpu`, or supply a complete matching CUDA runtime and CUDA/cuDNN dependencies for `cuda`. Explicit CUDA failures remain strict. On Windows a verified GPU configuration also sets `NUMERA_ORT_LIBRARY` to the matching core DLL and `NUMERA_DLL_DIRECTORY` to compatible dependency libraries. Never mix CPU core libraries with stale CUDA providers. macOS has no CUDA path.

In default direct mode the client process's device setting controls recognition. In explicit remote mode, the separately running server's setting controls it. `doctor` inventory is not actual inference evidence; verify executed image/profile results before claiming GPU recognition. See [GPU setup](GPU.md).

## Optional: explicitly managed HTTP relay

Applications that deliberately share an authenticated loopback instance can still start it manually:

```bash
NUMERA_ROOT="$PWD" NUMERA_DEVICE=cpu ./bin/numera-cha-solver-mcp serve --port 1583
```

Windows PowerShell:

```powershell
$env:NUMERA_ROOT = (Get-Location).Path; $env:NUMERA_DEVICE = 'cpu'; ./bin/numera-cha-solver-mcp.exe serve --port 1583
```

For this mode only, change the client's arguments to `["bridge", "--remote"]`. Set matching `NUMERA_ROOT` and `NUMERA_MCP_PORT` (default1583). The relay reads that root's protected `data/mcp-token.txt`; never publish its value. `NUMERA_MCP_TOKEN` can select an existing secret through an approved private environment. The server creates a protected credential only after binding successfully.

Remote mode never starts a server, switches endpoint/provider or falls back to direct mode after authentication/transport failure. Port/root/device must match the selected instance. No arbitrary URL or proxy is supported. The default direct connection does not use these HTTP credentials or ports.

## Other clients

Cursor: merge [cursor-mcp.json](../examples/cursor-mcp.json) into its intended MCP scope. Set native `${env:NAME}` variables in the launching environment; this is not universal JSON interpolation. `args:["bridge"]` uses the same direct runtime.

Codex:

```toml
[mcp_servers.Numera-CHA-Solver-MCP]
command = "/absolute/installation/bin/numera-cha-solver-mcp"
args = ["bridge"]

[mcp_servers.Numera-CHA-Solver-MCP.env]
NUMERA_ROOT = "/absolute/installation"
NUMERA_DEVICE = "cpu"
```

Other Agent Skills/MCP clients use their documented stdio configuration. See [Claude](../plugins/claude/README.md), [Codex](../plugins/codex/README.md) and [Cursor](../plugins/cursor/README.md) guides; manifests do not prove client UI installation.

## Tools and human review

`capabilities`, `models`, `doctor`, `recognize`, `create_review`, `get_review`, `cancel_review`. Unknown fields are rejected; base64 images are bounded to8MiB decoded, text/answers to4096 Unicode scalars. No URL/path/browser input is accepted.

Suggestions never establish site acceptance. Pending reviews bind the exact observation revision; the person corrects/confirms/cancels in the local workspace. There is no agent confirmation/submission/token tool. Human attestation is not cryptographic proof against another process controlling the same OS account.

## Troubleshooting

- Older binary plus `bridge`: update the signed native binary, not only the plugin. Check its actual `--version`.
- `mcp_stdio_failed`: inspect the local per-run log and client stderr; check root permissions and valid client initialization. stdout is MCP only.
- `mcp_unavailable` with `--remote`: start the selected server and verify matching root/port/credential; do not change providers or silently switch modes.
- Missing runtime/model: inspect `models` and `doctor`; acquire reviewed assets only through the explicit model workflow. Never execute remote model code.
- Live site verification: hand off to a person or the site's official API. Local OCR is not proof of provider acceptance or CAPTCHA accuracy.

[Signing](SIGNING.md) · [Native delivery](DESKTOP.md) · [License](LICENSING.md) · [Verification](../reports/VALIDATION.md).
