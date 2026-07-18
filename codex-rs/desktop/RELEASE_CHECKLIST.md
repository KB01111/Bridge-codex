# Bridge Codex private-release checklist

This checklist is the production designation gate for Windows 11 x64. A
successful build creates a release candidate; it does not by itself authorize
production use. Record links, machine identifiers, dates, and owners beside
each completed item.

## Automated evidence

- The source tag is immutable and matches `VERSION`, npm, Tauri, Rust,
  `Cargo.lock`, release notes, and `UPSTREAM_BASELINE`. The numeric WiX product
  version matches that release and the WiX upgrade code remains pinned.
- `CI required`, `Frontend test and build`, `Native smoke (windows-2022)`, and
  `Windows installer package` genuinely executed and passed. A skipped or
  billing-blocked check is not evidence.
- The protected release workflow passed frozen install, frontend tests and
  build, Playwright keyboard/responsive flows at 760, 1024, and 1600 pixels,
  scoped Rust tests, signed MSI and NSIS builds, signature/timestamp
  verification, checksums, SBOM generation, and provenance verification.
- The downloaded private-release assets reproduce the published SHA-256
  checksums. Both Authenticode signatures and the attested installers,
  checksum file, SBOM, and release notes verify against the exact protected
  release workflow, tag, source commit, and GitHub-hosted runner.

## Clean-machine validation

Use clean Windows 11 x64 VMs at 100%, 150%, and 200% display scaling. Record
the Windows build, VM image, installer hashes, and test result for each scale.

- MSI install, launch, repair/uninstall, and NSIS install/uninstall complete.
- First-run setup works with the network unavailable except for the separately
  operated loopback CLIProxyAPI; offline WebView2 installation is confirmed.
- Valid gateway setup, invalid credentials, chat-only endpoint rejection,
  malformed SSE, unknown-model warning, outage, and recovery behave as
  documented.
- Thread start, streamed tool call, approval allow/deny, sandbox execution,
  diff rendering, interrupt, restart, resume, fork, archive, and export work.
- Sleep/resume, application crash recovery, and event-channel resynchronization
  preserve durable thread state without silent data loss.
- Browser consent and approval boundaries, HTTPS failure, profile cleanup,
  download/popup blocking, and absence of desktop input from agent tools are
  verified.
- A2A authentication, body/rate/concurrency limits, durable context mapping,
  cancellation, approval pause, restart recovery, and unauthenticated denial
  are verified.
- Upgrade from the previous signed candidate, rollback to the retained signed
  installer, and uninstall with both retained-data and delete-data paths work.

## Release decision

- No unresolved release-blocking security, data-loss, accessibility, agent
  loop, or installer defect remains.
- A ten-business-day private pilot completed on at least three representative
  Windows 11 x64 machines without a severe security, data-loss, agent-loop, or
  rollback failure.
- Privacy, recovery, proxy setup, attribution, release notes, support-bundle
  contents, and the rollback artifact were reviewed against the candidate.
- The protected-environment approver authorized the release workflow, which
  published the private prerelease only after re-verifying downloaded assets.
- After the pilot, a maintainer reviewed all evidence and explicitly approved
  production designation. V1 remains private, uses manual signed upgrades, and
  makes no public-GA claim.
