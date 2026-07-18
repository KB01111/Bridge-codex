# Bridge Codex privacy and data locations

Bridge Codex is a local-first Windows desktop application. It does not include
product telemetry, advertising identifiers, or automatic crash uploads.

## Data that can leave the computer

Prompts, agent tool results, and approved repository or browser context are
sent to the model selected through the user-managed CLIProxyAPI service. Bridge
connects to that service only over an authenticated loopback origin. Web pages
opened in the isolated browser connect to their normal remote origins.

Browser observations are not agent context until consent is granted for the
current thread. State-changing browser calls require approval. OS-wide desktop
input is never registered as an agent tool.

## Local storage

Bridge-owned agent rollouts, code-memory indexes, diagnostic logs, and
versioned non-secret preferences are stored below the operating system's
application-data directory for `com.kbhelios.bridge-codex`. Ephemeral Chromium
profiles are created below the operating system temporary directory and
removed when the browser or application closes.

CLIProxyAPI and A2A bearer tokens are stored in Windows Credential Manager.
They are not written to browser storage, returned by status calls, or included
in support exports. React `localStorage` is limited to small UI preferences.

## Retention and deletion

Diagnostic files are local-only, bounded to five 4 MiB files, and pruned after
seven days. A default support export contains application/version state and
redacted diagnostics; it excludes prompts, source text, screenshots, browser
profiles, rollouts, and credentials.

**Delete all local data** stops Bridge-owned services and removes Bridge-owned
rollouts, indexes, profiles, preferences, diagnostics, and credentials. The
application asks for confirmation because this operation cannot be undone.
Uninstalling the application does not imply that data should be deleted.
