# NET-02 — Detect a silent pool instead of mining a stale job forever (#34).

**Status:** Completed

A pool that stops sending but leaves the TCP socket open left the miner hashing a job the pool had long since replaced. Every share found in that window was submitted into a connection that never answered and discarded with no error, no warning and no counter. Measured on the 12-hour live run: two windows of 121 and 88 minutes — 209 minutes, about 60% of the session — ending only when the pool finally closed the socket. Keepalives are writes and proved nothing about whether the peer was still talking. Fixed by tracking the time of the last received message and treating prolonged silence (3 × KEEPALIVE_INTERVAL = 180 s) as a dead connection. The test drives the real `receiver_loop` against a local listener that holds the socket open without sending — the only way the silence timeout fires. Break-tested: removing the check fails the test; source restored byte-identical. Review (Sonnet, round 1): MERGEABLE, no blockers, no majors, four minors (all fixed).

---

*Full record: the `NET-02` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
