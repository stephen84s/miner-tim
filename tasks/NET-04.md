# NET-04 — Close stream-clear and session-id-check gaps in relogin and login (#37)

**Status:** Implemented; awaiting review

GitHub #37 reported a failed donation relogin leaving a live unauthenticated stream with a stale session and 30s read timeout. `relogin_as()` returned early on login failure without clearing the stream; separately, `login()` accepted responses with no session id, allowing a submit-acknowledgement to be mistaken for a login reply. Fixed by (1) explicitly clearing the stream on `relogin_as()` failure, sending the receiver loop into its normal reconnect path, and (2) requiring an `"id"` field in login responses before proceeding, matching xmrig's logic. Four new tests cover both fixes in isolation and end-to-end through `receiver_loop`; break-tested three ways; mutation testing run with 2 caught, 2 unviable mutants (caveat: no mutant generated for the stream-clear line or let-else return itself — hand break-tests are the evidence). The general stale-session-id window in `submit_share` remains open and has been filed as GitHub #41.

---

*Full record: the `NET-04` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
