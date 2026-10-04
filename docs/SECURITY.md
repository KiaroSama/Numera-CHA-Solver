# Safe local operation

Use licensed inputs and owner-controlled test environments. This application does not provide token forgery, target-domain impersonation, browser identity churn, credential testing, account creation, spam or rate-limit evasion.

API binds `127.0.0.1` by default and requires bearer or HTTP-only same-origin session authorization. Host/origin/input-size/schema checks reject endpoint overrides and arbitrary URLs/paths. Non-loopback CLI binding is disabled. TLS/auth hardening is required before any separately approved remote deployment.

The local credential is generated at manual startup or supplied as `NUMERA_API_TOKEN`; its value is not printed or logged. Keep local data under an owner-restricted directory. Windows file modes alone do not implement a restrictive NTFS ACL, so enforce the documented launcher ACL or configure a credential through your approved secret store.

No raw challenge assets, cookies, signed URLs, response tokens, screenshots, HAR or browser traces are captured by default. Logs contain operational metadata only; exception input/details are suppressed or redacted. Results stay local with a 24-hour retrieval TTL; physical row deletion occurs on later task creation and is not an idle-retention guarantee. Credential publication follows exclusive startup ownership and successful initialization; duplicate or failed startup must not rotate the running server's credential. Never attach raw credentials or private assets to support reports.

Model hashes establish artifact identity, not a security certification. No model-provided Python/custom-op library is executed. Native model processing remains inside an owned bounded worker; no broad process-name termination.
