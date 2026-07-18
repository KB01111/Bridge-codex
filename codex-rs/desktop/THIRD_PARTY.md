# Third-party software and attribution

Bridge Codex is downstream software based on OpenAI Codex and includes Tauri,
React, Chromium automation libraries, Rust crates, and npm packages under their
respective licenses. CLIProxyAPI and Chrome/Chromium are separate prerequisites
and are not redistributed by Bridge Codex.

The release workflow scans the exact signed installer bundle and publishes the
resulting CycloneDX SBOM beside the MSI, NSIS installer, checksums, and
provenance. That SBOM records what the pinned scanner can identify in the
packaged files; it is not a substitute for reviewing the resolved Rust and npm
dependency graphs or the license texts shipped by those dependencies.

For local review, enumerate declared and resolved dependencies with:

```powershell
pnpm licenses list --prod --json
Set-Location codex-rs
cargo metadata --locked --format-version 1
```

Before publishing a release, review the SBOM and dependency-license output for
new notice, source-offer, redistribution, or attribution requirements. Update
the repository `NOTICE` file when a dependency requires shipped attribution;
do not treat the SBOM as a substitute for a required license or notice text.

The workflow creates GitHub build provenance for the two installer digests in
the public source repository and attaches a copy of the Sigstore bundle to the
private release. The private release repository is only the distribution
location. If the source repository becomes private, GitHub requires an
Enterprise Cloud plan for artifact attestations; without it, the workflow must
fail rather than publish an unattested candidate.

Inventory and provenance metadata do not change third-party license
obligations. Questions about a dependency should be resolved against the
license text shipped in its exact resolved package.
