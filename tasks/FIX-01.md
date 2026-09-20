# FIX-01 — `DonationSchedule::level()` pinned (#25).

**Status:** Completed

The first real defect mutation testing found, one run after the tooling landed — and in code nobody was editing. `level()` had one assertion, `new(0).level() == MIN_DONATE_LEVEL`, written to check clamping; since the minimum *is* 1, an accessor that always returned 1 satisfied it for the wrong reason. Impact stated at its real size: `beneficiary_at` reads the **field**, so the donation was never at risk — only the `"donate-level {}%"` line reported to the operator. Still worth fixing: it is the one financial setting the miner has, and that line is how someone confirms `--donate-level` took effect. Fixed by pinning the accessor across values that defeat both plausible constants (the clamp floor and the default). Verified by the tool that found it: 2 mutants, 2 caught, 12 s.

---

*Full record: the `FIX-01` entry in [`AUDIT.md`](../AUDIT.md), which is authoritative. This file is the summary.*
