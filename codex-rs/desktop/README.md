# Bridge Codex Desktop

Bridge Codex is a Tauri 2 desktop client with a React 19 frontend and a Rust
host. The distributable contains the Bridge Codex application only. It does
not contain, download, or update CLIProxyAPI.

## Prerequisites

- Rust 1.95 (the repository toolchain), Node.js 22, pnpm 10.33, and `just`.
- Chrome or Chromium for the browser workspace.
- A separately installed CLIProxyAPI compatible with
  `http://localhost:8317/v1/models` and `/v1/chat/completions`.
- Linux: WebKitGTK 4.1, GTK 3, libayatana-appindicator, librsvg, and the usual
  native build tools. Ubuntu 22.04 or Debian 12 are suitable build baselines.
- macOS: Xcode Command Line Tools.
- Windows: Visual Studio 2022 Build Tools with **Desktop development with
  C++**, a Windows 10 or 11 SDK, and WebView2. Production installers include
  the WebView2 offline installer, so installation does not depend on a network
  download.

Install the workspace dependencies from the repository root:

```sh
pnpm install --frozen-lockfile
```

## Development and verification

Run the native development app:

```sh
pnpm --dir codex-rs/desktop tauri dev
```

Useful production checks are:

```sh
pnpm --dir codex-rs/desktop build
cd codex-rs && just test -p codex-desktop
pnpm --dir codex-rs/desktop tauri build --no-bundle
```

The first command type-checks and bundles the frontend. The second runs the
Rust crate tests. The final command is a native release-build smoke test
without creating installers.

Bazel keeps `DEP_TAURI_DEV=true` for this target and supplies a Windows
`llvm-windres` shim to Tauri's build script. Consequently,
`bazel build //codex-rs/desktop/src-tauri:desktop` is a compile-only check; it
does not build the frontend or produce a distributable installer. Use the
Tauri CLI for production packaging.

## CLIProxyAPI deployment

Set `CLIPROXYAPI_PATH` to the absolute executable path for the most explicit
deployment. `CLIPROXYAPI_CONFIG_PATH` may point at its configuration file;
otherwise Bridge Codex uses `config.yaml` beside the executable when present.

Without `CLIPROXYAPI_PATH`, Bridge Codex searches `PATH`, then these locations:

- beside the Bridge Codex executable, and its `binaries/` and `resources/`
  subdirectories;
- `$HOME/.local/bin` and `$HOME/.cli-proxy-api`;
- `%LOCALAPPDATA%\CLIProxyAPI` on Windows, or `/usr/local/bin` on Unix.

Supported executable names are `cli-proxy-api`, `cliproxyapi`, and
`CLIProxyAPI`, with `.exe` on Windows.

A downstream distributor may place an independently obtained CLIProxyAPI
binary in one of the app-adjacent locations. That distributor owns binary
provenance, license compliance, signing, updates, and platform matching. Do
not add it to Tauri's `externalBin` until a reviewed, reproducible sidecar
supply chain exists.

## Production bundles and network boundary

Create the current platform's configured installers with:

```sh
pnpm --dir codex-rs/desktop tauri build
```

Tauri reports the generated bundle paths. Windows installers use the bundled
offline WebView2 installer, trading a substantially larger installer for an
offline installation path while retaining the system's updateable WebView2
runtime. macOS releases still require Apple signing and notarization, and
Windows installers should be Authenticode-signed before public distribution.
CI deliberately creates no unsigned release artifacts. Its
`--debug --no-bundle` smoke test reuses test-build artifacts and does not
download or package the configured WebView2 offline installer.

The separate `desktop-release.yml` workflow runs only for
`bridge-desktop-v*` tags or an explicit manual dispatch. It builds the real
platform bundles and adds them to a draft GitHub release. Linux requires no
additional release credentials. macOS jobs fail closed unless
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`,
`APPLE_PASSWORD`, and `APPLE_TEAM_ID` are configured as repository secrets.
The Windows job likewise requires a base64-encoded `WINDOWS_CERTIFICATE` PFX,
`WINDOWS_CERTIFICATE_PASSWORD`, and the certificate issuer's
`WINDOWS_TIMESTAMP_URL`. Publishing the draft remains an explicit maintainer
decision after signature and installer verification.

The application WebView's production content security policy permits Tauri IPC
but no direct HTTP, WebSocket, or remote origin. CLIProxyAPI on port 8317 and
the A2A service on port 8120 are reachable only through the bounded Rust host
commands. The separately launched browser workspace may navigate to an HTTP or
HTTPS address supplied by the user; that navigation occurs in Chrome or
Chromium, outside the application WebView. A user-initiated CLIProxyAPI OAuth
login may likewise open the provider's page in the system browser.
