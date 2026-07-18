# Bridge Codex Desktop

Bridge Codex is a Tauri 2 desktop client with a React 19 frontend and a Rust
host. The distributable contains the Bridge Codex application only. It does
not contain, download, or update CLIProxyAPI.

## Prerequisites

- Rust 1.95 (the repository toolchain), Node.js 22, pnpm 10.33, and `just`.
- Chrome or Chromium for the browser workspace.
- A separately operated CLIProxyAPI bound to loopback, protected by an API
  key, and compatible with `/v1/models` plus streaming OpenAI
  `/v1/responses` function calls. Chat-Completions-only deployments cannot
  power the embedded Codex agent runtime.
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

Install and operate CLIProxyAPI independently. Production Bridge Codex never
searches for, downloads, launches, signs in to, updates, or terminates a proxy
executable. This keeps proxy provenance and account authorization outside the
desktop application's trust boundary.

In Bridge settings, enter an authenticated loopback origin such as
`http://127.0.0.1:8317` and the service API key. The key is stored in the
operating-system credential vault and is never returned to the React frontend.
Bridge rejects non-loopback addresses, missing authentication, chat-only
services, malformed streaming responses, and services that cannot complete a
forced multi-step function-call probe. The probe makes a small model request
and can consume provider quota.

Known Codex model identifiers use their bundled capability metadata. Unknown
identifiers are shown as experimental because a standard `/v1/models`
response does not provide all context, reasoning, and tool capability fields
needed by Codex.

## Local data and privacy

Bridge Codex does not send product telemetry or upload crash reports. It
connects only to the configured loopback model gateway and to destinations the
user explicitly opens in the isolated browser workspace. Prompts, repository
snippets returned by code memory, and approved browser observations are sent
to the model selected through CLIProxyAPI.

Agent threads and rollouts, code-memory indexes, preferences, and diagnostic
logs live in the platform application-data directory. Secrets live in the
operating-system credential vault. Logs are redacted, rotate at five 4 MiB
files, and expire after seven days. A user-created support bundle excludes
prompts, source text, screenshots, browser profiles, rollouts, and credentials.

Use **Delete all local data** to stop local services and remove Bridge-owned
threads, indexes, browser profiles, preferences, and logs. Uninstalling the
binary alone does not implicitly destroy user data. Pre-release browser-storage
conversations can be exported once but are not imported into authoritative
Codex threads.

## Agent, browser, and delegation boundaries

The embedded app server owns durable threads, turns, sandbox execution, diffs,
and approval requests. The default agent sandbox may write inside the selected
workspace but has no shell network access. Command, file, permission, user
input, MCP, and state-changing browser actions remain subject to explicit user
approval.

Browser automation uses a unique ephemeral Chromium profile and enforces TLS
certificate errors. Browser content is shared with a model only after the user
enables the browser for that thread. OS-wide mouse and keyboard control is not
an agent tool: it remains a manual, default-off experimental Work Mode.

The A2A service is also default-off and loopback-only. Enabling it creates a
bearer token in the credential vault. Task endpoints require that token and
cannot bypass normal agent approvals.

## Production bundles and network boundary

Create the current platform's configured installers with:

```sh
pnpm --dir codex-rs/desktop tauri build
```

Tauri reports the generated bundle paths. Production v1 supports Windows 11
x64 only. Its MSI and NSIS installers include the offline WebView2 installer,
trading a larger package for installation without a WebView download.

The `desktop-release.yml` workflow accepts an existing
`bridge-desktop-v<version>` source tag. It verifies that the workflow event,
tag, checked-out commit, frozen upstream baseline, npm metadata, Tauri config,
Rust package, lockfile, and release notes all agree before building. A manual
dispatch must run from the supplied tag ref itself, not merely a branch at the
same commit. Numbered RCs use a monotonic numeric WiX product version while
retaining the semantic RC version in the app and artifact names; the version
check enforces that mapping and a stable upgrade code.

The protected `bridge-private-release` environment must provide:

- `BRIDGE_RELEASE_TOKEN`, with write access only to the private
  `KB01111/Bridge-codex-releases` repository;
- `WINDOWS_CERTIFICATE`, containing one base64-encoded code-signing PFX;
- `WINDOWS_CERTIFICATE_PASSWORD`; and
- `WINDOWS_TIMESTAMP_URL` for the certificate issuer's timestamp service.

The workflow builds exactly one MSI and one NSIS installer, verifies both
Authenticode signatures and timestamps, creates SHA-256 checksums, scans the
installer bundle into a CycloneDX SBOM, adds the installer filenames and
digests to that SBOM, and attests the installers, checksum file, SBOM, and
release notes from the public `KB01111/Bridge-codex` source repository. It
uploads a private draft, downloads every asset into a clean temporary
directory, and re-verifies the downloaded checksums, signatures, timestamps,
SBOM inventory, provenance, signing workflow, tag, source commit, and hosted
runner identity. Only then does it publish an RC as a private prerelease. The
workflow never replaces an already published tag and does not alter earlier
releases, so the previous signed installer remains available for manual
rollback. V1 has no automatic updater.

GitHub artifact attestations for a private source repository require GitHub
Enterprise Cloud. The current source repository is public while the release
repository is private. If the source repository becomes private without that
plan, provenance generation fails closed and the release cannot complete.

After downloading the private prerelease, compare `SHA256SUMS.txt`, inspect both
signatures, and verify each installer against the attached provenance bundle:

```powershell
Get-FileHash -Algorithm SHA256 -Path .\*.msi, .\*-setup.exe
Get-AuthenticodeSignature -FilePath .\*.msi, .\*-setup.exe
gh attestation verify .\<installer> `
  --repo KB01111/Bridge-codex `
  --bundle .\bridge-codex-provenance.jsonl `
  --signer-workflow KB01111/Bridge-codex/.github/workflows/desktop-release.yml `
  --source-ref refs/tags/bridge-desktop-v1.0.0-rc.1 `
  --source-digest "<release-source-commit>" `
  --deny-self-hosted-runners
```

The application WebView's production content security policy permits Tauri IPC
but no direct HTTP, WebSocket, or remote origin. CLIProxyAPI on port 8317 and
the A2A service on its configurable loopback port (8120 by default) are
reachable only through the bounded Rust host commands. The separately launched
browser workspace may navigate to an approved HTTP or HTTPS address; that
navigation occurs in Chrome or Chromium, outside the application WebView.
CLIProxyAPI account setup and OAuth remain external to Bridge Codex.

See [PRIVACY.md](PRIVACY.md), [RECOVERY.md](RECOVERY.md), and
[THIRD_PARTY.md](THIRD_PARTY.md) for data-flow, recovery, rollback, and
release-attribution policies. A release remains an RC until every item in
[RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md) has recorded evidence.
