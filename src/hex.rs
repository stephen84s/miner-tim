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
    // Reject non-ASCII before slicing. `hex[i..i + 2]` panics rather than
    // returning None when the boundary falls inside a multi-byte character, so
    // a value like "aaa…€" reached the caller as a process abort instead of a
    // parse failure. Found reviewing --tls-fingerprint, where the operator
    // pastes arbitrary text.
    if !hex.is_ascii() || !hex.len().is_multiple_of(2) {
        return None;
    }

    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
        bytes.push(byte);
    }
    Some(bytes)
}
