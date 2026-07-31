//! Donation ("donate-level") support.
//!
//! Like XMRig's own `donate-level`, MinerTim dedicates a small, configurable
//! fraction of mining time to donation, split 50/50 between:
//!   - the MinerTim author, and
//!   - the XMRig project — MinerTim is an AI-assisted Rust translation of XMRig,
//!     so a share of the donation goes upstream.
//!
//! The level defaults to [`DEFAULT_DONATE_LEVEL`]% and is configurable at runtime
//! (`--donate-level`) down to [`MIN_DONATE_LEVEL`]%. Going below the minimum is
//! deliberately not possible at runtime — it requires editing this file and
//! recompiling. The donation is disclosed at startup and in the README.

/// MinerTim author donation address (Monero mainnet).
pub const AUTHOR_ADDRESS: &str =
    "49stQdfmRNQctnx2wEo7JfZCgWp3WhcF4P1d1MXmiN6PEtd5fjpvVcS8XuYUyx6sdz4nyccsrhjBLgtuD5iejyZVUBbBZng";

/// XMRig project donation address, from <https://github.com/xmrig/xmrig>.
pub const XMRIG_ADDRESS: &str =
    "48edfHu7V9Z84YzzMa6fUueoELZ9ZRXq9VetWzYGzKt52XU5xvqgzYnDK9URnRoJMk1j8nLwEVsaSWJ4fhdUyZijBGUicoD";

/// Default donate level (percent of mining time).
pub const DEFAULT_DONATE_LEVEL: u8 = 5;
/// Minimum donate level enforced at runtime. Lower requires recompiling.
pub const MIN_DONATE_LEVEL: u8 = 1;
/// Maximum donate level.
pub const MAX_DONATE_LEVEL: u8 = 100;

/// Length of one donation cycle, in seconds (100 minutes, matching XMRig).
const CYCLE_SECS: u64 = 100 * 60;

/// Clamp a requested level into the permitted runtime range
/// `[MIN_DONATE_LEVEL, MAX_DONATE_LEVEL]`.
pub fn clamp_level(level: u8) -> u8 {
    level.clamp(MIN_DONATE_LEVEL, MAX_DONATE_LEVEL)
}

/// Who the current mining slice belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beneficiary {
    User,
    Author,
    Xmrig,
}

/// Time-based donation schedule. Given the seconds elapsed since mining started,
/// decides whose wallet the current slice mines for. Within each cycle the
/// donated portion (`level`%) sits at the end, split 50/50 author/XMRig.
#[derive(Clone, Copy)]
pub struct DonationSchedule {
    level: u8,
}

impl DonationSchedule {
    pub fn new(level: u8) -> Self {
        Self {
            level: clamp_level(level),
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn beneficiary_at(&self, elapsed_secs: u64) -> Beneficiary {
        let t = elapsed_secs % CYCLE_SECS;
        let donate = CYCLE_SECS * self.level as u64 / 100;
        let author = donate / 2; // XMRig gets the remainder (donate - author)
        let user = CYCLE_SECS - donate;
        if t < user {
            Beneficiary::User
        } else if t < user + author {
            Beneficiary::Author
        } else {
            Beneficiary::Xmrig
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_level_5_phases() {
        // cycle 6000s, donate 300s => author 150s, xmrig 150s, user 5700s.
        let s = DonationSchedule::new(DEFAULT_DONATE_LEVEL);
        assert_eq!(s.beneficiary_at(0), Beneficiary::User);
        assert_eq!(s.beneficiary_at(5699), Beneficiary::User);
        assert_eq!(s.beneficiary_at(5700), Beneficiary::Author);
        assert_eq!(s.beneficiary_at(5849), Beneficiary::Author);
        assert_eq!(s.beneficiary_at(5850), Beneficiary::Xmrig);
        assert_eq!(s.beneficiary_at(5999), Beneficiary::Xmrig);
        assert_eq!(s.beneficiary_at(6000), Beneficiary::User); // wraps
    }

    #[test]
    fn floor_enforced() {
        assert_eq!(clamp_level(0), MIN_DONATE_LEVEL);
        assert_eq!(DonationSchedule::new(0).level(), MIN_DONATE_LEVEL);
        assert_eq!(clamp_level(250), MAX_DONATE_LEVEL);
    }

    #[test]
    fn donated_fraction_matches_level() {
        for level in [MIN_DONATE_LEVEL, 5, 50] {
            let s = DonationSchedule::new(level);
            let donated = (0..CYCLE_SECS)
                .filter(|&t| s.beneficiary_at(t) != Beneficiary::User)
                .count() as u64;
            assert_eq!(donated, CYCLE_SECS * level as u64 / 100);
        }
    }
}
