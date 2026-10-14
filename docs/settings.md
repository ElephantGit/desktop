# Settings

English | [中文](settings.zh.md)

Ora Desktop presents Settings through the App Shell and the Tauri-backed contracts client.

The Backend `Settings` interface schedules developer-mode, log-level, and network-proxy persistence on the blocking executor. Async callers await the result without managing SQLite scheduling; database failures retain their existing error projection. The synchronous marketplace Git/cache rebuild uses the underlying synchronous settings service on its host blocking executor.

## Plugin marketplace sources

In **Settings → Plugins → Manage marketplace**, choose **Add source** to open a dialog.
Enter the repository's HTTPS Git URL and branch (default `main`), and optionally enable
**Use proxy** to use Ora's configured proxy. **Plugin package retrieval** defaults to
**Direct HTTPS**. Choosing **S3 SigV4** reveals Endpoint, Bucket, Region, and both
credentials, which are submitted with the source. URL, branch, and those S3 fields are
required. The Backend validates repository identity, branch syntax, S3 fields, and
conflicts. **Edit** can change retrieval later; leaving both credential fields blank
while editing an existing S3 source keeps the stored pair.

The dialog focuses the URL field. Enter submits; Cancel, Escape, and the close button
discard the draft and return focus to Add source without closing Settings. During submission,
the form prevents duplicate requests and dismissal. Failures remain visible in the dialog
and preserve the draft for correction or retry. If the S3 credential write fails, the new
source is removed rather than left on Direct HTTPS. Success closes the dialog and refreshes
the source list. Use **Sync marketplace** in Plugins to refresh the available plugin listings.

## Developer mode

Settings always includes a Developer options category, whose page contains the developer-mode switch. Its authoritative value is `user_config.developer_mode`, interpreted by the application layer over the generic SQLite key/value adapter; the frontend does not persist a second copy. A failed initial read leaves the switch disabled, keeps developer-only controls hidden on that page, and offers retry. A failed update retains the last backend response.

Developer mode controls discoverability only. It does not grant permissions, change transport authorization, or make backend operations inaccessible when disabled.

## Developer options

The Developer options navigation category remains available regardless of the developer-mode value so users can always reach its switch. When developer mode is enabled, the same page reveals the process-wide log-level selector and a **Download logs** button that exports today's diagnostic log through the host's native save dialog; disabling it hides and unmounts both without navigating away. The download button only appears on hosts that expose the diagnostic-logs capability (Desktop), and it reuses the same export and toast messaging as the error-toast action described in [Runtime Logging](runtime-logging.md).

Log-level changes take effect for the current Desktop process and are persisted in `user_config.log_level`. The selector displays the authoritative effective level. Startup restores the persisted preference, defaulting to `info` only when unset; legacy environment values have no effect. Trace and Debug include a high-volume warning.

See [Runtime Logging](runtime-logging.md) for startup restoration and rollback behavior.
