# Bridge Codex recovery guide

## Model gateway unavailable

Keep CLIProxyAPI running independently on an API-key-protected loopback origin,
for example `http://127.0.0.1:8317`. Re-test the connection from Bridge after
the service recovers. Bridge does not launch, stop, update, or sign in to the
proxy process. A chat-only endpoint cannot be used; the capability check must
complete a streaming Responses function-call round trip.

## Interrupted or crashed agent turn

Restart Bridge and resume the existing thread. Embedded app-server rollouts are
the authoritative history, so a process crash must not require reconstructing
conversation state from the WebView. If an event stream reports lag, refresh
the thread from app-server storage before continuing.

## Browser recovery

Stop and restart the browser workspace if Chromium becomes unresponsive. Each
start uses a new isolated profile. A normal stop removes the current profile
only after its name and path match Bridge's managed temporary namespace. A
process crash can leave that temporary directory behind; inspect the path and
namespace before removing it manually.

## Corrupt pre-release browser state

Use the one-time legacy conversation export before clearing incompatible
pre-release `localStorage` data. Legacy chats are not imported into production
agent threads. Small preferences can be reset without deleting rollouts.

## Upgrade and rollback

V1 upgrades are manual. Verify the downloaded installer's signature and the
published SHA-256 checksum before installation. Also verify the attached
`bridge-codex-provenance.jsonl` bundle against the public source repository:

```powershell
gh attestation verify .\<installer> `
  --repo KB01111/Bridge-codex `
  --bundle .\bridge-codex-provenance.jsonl
```

Keep the previous signed installer, checksum, and provenance bundle as the
rollback artifact. If rollback is required, close Bridge, install the previous
signed version, and resume an existing thread only if that version supports its
rollout format.

Use **Delete all local data** only when recovery requires a clean start. Export
anything needed first; deletion is intentionally irreversible.

Before publishing a candidate, record the clean-machine, upgrade, rollback,
and pilot evidence in [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md).
