# Bridge Codex 1.0.0-rc.1

This is the first private Windows 11 x64 release candidate for the Bridge
Codex agent workspace.

## Highlights

- Embedded app-server threads and turns are the authoritative conversation
  runtime, with streamed agent activity, interrupt/resume, diffs, and explicit
  approval handling.
- CLIProxyAPI is a separately managed, API-key-protected loopback service. The
  setup probe requires conformant streaming Responses and multi-step function
  tool calls; Bridge does not discover or launch proxy executables.
- Browser automation uses an isolated disposable profile, enforces TLS errors,
  and keeps state-changing actions approval-gated. Experimental desktop-wide
  input remains manual-only and disabled by default.
- A2A delegation is disabled by default and protected by a credential-vault
  token, body/concurrency/rate limits, durable thread mapping, and the same
  approval loop used by local work.
- Signed MSI and NSIS packages include offline WebView2 installation. The
  private release also includes SHA-256 checksums, an installer-bundle
  CycloneDX SBOM, and the public source repository's build-provenance bundle.

## Before deployment

The protected workflow re-downloads and verifies every asset before publishing
the private prerelease. Before deployment, independently verify the
Authenticode signatures, published checksums, SBOM, and provenance from a fresh
download. Configure CLIProxyAPI on an authenticated loopback origin and
complete the in-app capability test.

This release has no automatic updater and makes no public-GA claim. Keep the
previous signed installer as the rollback artifact. Production designation
still requires every automated, clean-machine, rollback, and ten-business-day
pilot gate in [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md).
