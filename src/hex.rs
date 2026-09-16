//! Hex encoding/decoding utilities shared across the crate.

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }

    // Decode nibble by nibble rather than via `u8::from_str_radix`, which is a
    // *number* parser and accepts a leading sign: `from_str_radix("+1", 16)` is
    // `Ok(1)`, so the previous implementation decoded "+1" as the byte 0x01 and
    // "+f" as 0x0f. Hex is not arithmetic and has no sign, so that was simply
    // wrong — found by writing this module's first tests, having been reachable
    // from pool-supplied job fields for the project's life.
    //
    // Working over `as_bytes()` also removes the panic this function used to
    // carry: `hex[i..i + 2]` slices by byte index, so a multi-byte character
    // straddling the boundary aborted the process instead of returning `None`.
    // A hostile or broken pool could crash a miner with one such byte in a job
    // (`parse_job` runs this on `blob`, `target` and `seed_hash`).
    let raw = hex.as_bytes();
    let nibble = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };

    let mut bytes = Vec::with_capacity(raw.len() / 2);
    for pair in raw.chunks_exact(2) {
        bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let bytes = vec![0x00, 0x0f, 0x10, 0x7f, 0x80, 0xff];
        assert_eq!(hex_encode(&bytes), "000f107f80ff");
        assert_eq!(hex_decode("000f107f80ff"), Some(bytes));
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn decodes_either_case_and_encodes_lower() {
        assert_eq!(hex_decode("DEADBEEF"), hex_decode("deadbeef"));
        assert_eq!(hex_encode(&[0xde, 0xad]), "dead");
    }

    #[test]
    fn rejects_odd_length_and_non_hex() {
        assert_eq!(hex_decode("abc"), None, "odd length has no byte boundary");
        assert_eq!(hex_decode("zz"), None);
        assert_eq!(hex_decode("0g"), None);
        // `u8::from_str_radix` accepts a leading '+' — it is a number parser,
        // and `from_str_radix("+1", 16)` is `Ok(1)`. The original implementation
        // therefore decoded "+1" as 0x01 and "+f" as 0x0f. Hex has no sign.
        assert_eq!(hex_decode("+1"), None);
        assert_eq!(hex_decode("+f"), None, "a sign is not a hex digit");
        assert_eq!(hex_decode("-1"), None);
        assert_eq!(hex_decode(" 1"), None, "whitespace is not a hex digit");
    }

    /// The regression this file exists to hold. `hex[i..i + 2]` slices by **byte**
    /// index, so a multi-byte character straddling the boundary panicked instead
    /// of returning `None` — a process abort, not a parse failure.
    ///
    /// It was found through `--tls-fingerprint`, where an operator pastes text,
    /// but the serious reach is `parse_job`: this runs on the pool-supplied
    /// `blob`, `target` and `seed_hash`, so a hostile or broken pool could crash
    /// a miner with one byte. That path is covered in `pool_connection`'s tests;
    /// these pin the primitive.
    #[test]
    fn non_ascii_returns_none_rather_than_panicking() {
        for input in [
            "€",           // 3 bytes, odd total — would fail the length check anyway
            "a€",          // 4 bytes: boundary lands inside the character
            "€€",          // 6 bytes, even: passes the length check, then slices mid-char
            "ab€cd",
            "\u{1F600}",   // 4-byte emoji
            "ffff€ffff",
        ] {
            assert_eq!(
                hex_decode(input),
                None,
                "hex_decode({input:?}) must return None, not panic"
            );
        }
    }

    #[test]
    fn every_byte_round_trips() {
        let all: Vec<u8> = (0u8..=255).collect();
        assert_eq!(hex_decode(&hex_encode(&all)), Some(all));
    }
}
