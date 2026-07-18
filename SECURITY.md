# Security Policy

Thank you for helping us keep Bridge Codex secure.

## Reporting Security Issues

Bridge Codex is maintained by KB Helios as a downstream Codex-based project.
Please do not send Bridge Codex vulnerabilities to OpenAI's Bugcrowd program.

Report a vulnerability privately through the repository's
[security advisory form](https://github.com/KB01111/Bridge-codex/security/advisories/new).
Include the affected version, reproduction steps, impact, and any suggested
mitigation. Do not open a public issue for an undisclosed vulnerability.

## Response expectations

Maintainers will acknowledge a complete report as soon as practical, coordinate
validation and remediation privately, and credit reporters who request it.
Bridge Codex does not currently operate a paid bug-bounty program.

## Supported versions

Bridge Codex is in a private release-candidate phase. Only the latest signed
candidate under active pilot is eligible for security fixes; superseded
candidates and development previews are unsupported. No public-GA support
commitment is made by `1.0.0-rc.1`.

## Operating Bridge Codex safely

Keep CLIProxyAPI bound to loopback with API-key authentication, review agent
approval requests, and leave experimental desktop control disabled when it is
not actively needed. Upstream Codex security guidance remains useful for the
embedded agent runtime: [Agent approvals & security](https://developers.openai.com/codex/agent-approvals-security).
