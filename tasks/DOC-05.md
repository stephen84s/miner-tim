# DOC-05 — Refresh README's "How fast is it" headline figures; delete stale committed log

**Status:** Completed

Updated `README.md`'s headline hashrate table with the user's overnight 8-hour run (11 threads, confirmed same conditions as the existing figures — plugged in, Low Power Mode off): peak `~5,010` → `~5,470` H/s, sustained-over-an-hour `~4,925` → sustained-over-8-hours `~5,280` H/s. Deliberately left the "Leave one core free" paragraph's own 11-vs-12-thread comparison numbers untouched — that's a separate, self-consistent controlled A/B about share-rejection behavior with no fresh 12-thread figure to pair against the new data, and updating only half of it would misrepresent a controlled experiment. Deleted `LIVE8H_RUN.log`, a previously-committed live-run log whose sole finding (found-to-submit latency pre-dating #35/#36) is already fully captured in prose and closed via issue #40 — matching the precedent already set when `LIVE8H_RUN_2.log` was deleted for the same reason. Left the two TLS-pinning evidence logs untouched.

---

*Full record: the `DOC-05` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
