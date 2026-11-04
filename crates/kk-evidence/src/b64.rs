const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum B64Error {
    InvalidCharacter,
    InvalidLength,
    NonCanonicalTrailingBits,
}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

fn value(c: u8) -> Result<u32, B64Error> {
    match c {
        b'A'..=b'Z' => Ok(u32::from(c - b'A')),
        b'a'..=b'z' => Ok(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Ok(u32::from(c - b'0') + 52),
        b'-' => Ok(62),
        b'_' => Ok(63),
        _ => Err(B64Error::InvalidCharacter),
    }
}

pub fn decode(text: &str) -> Result<Vec<u8>, B64Error> {
    let b = text.as_bytes();
    if b.len() % 4 == 1 {
        return Err(B64Error::InvalidLength);
    }
    let mut out = Vec::with_capacity(b.len() * 3 / 4);
    for chunk in b.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= value(*c)? << (18 - 6 * i as u32);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
        let leftover = match chunk.len() {
            2 => n & 0xffff,
            3 => n & 0xff,
            _ => 0,
        };
        if leftover != 0 {
            return Err(B64Error::NonCanonicalTrailingBits);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        for (raw, enc) in [
            (&b""[..], ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg"),
            (b"fooba", "Zm9vYmE"),
            (b"foobar", "Zm9vYmFy"),
            (&[0xfb, 0xff, 0xfe][..], "-__-"),
        ] {
            assert_eq!(encode(raw), enc);
            assert_eq!(decode(enc).unwrap(), raw);
        }
    }

    #[test]
    fn strictness() {
        assert_eq!(decode("Zg=="), Err(B64Error::InvalidCharacter));
        assert!(decode("Zm9v\n").is_err());
        assert!(decode("Zm9 v").is_err());
        assert_eq!(decode("Zm9 "), Err(B64Error::InvalidCharacter));
        assert_eq!(decode("Z"), Err(B64Error::InvalidLength));
        assert_eq!(decode("Zh"), Err(B64Error::NonCanonicalTrailingBits));
        assert_eq!(decode("Zm9"), Err(B64Error::NonCanonicalTrailingBits));
        assert_eq!(decode("Zm8"), Ok(b"fo".to_vec()));
        assert_eq!(decode("+/+/"), Err(B64Error::InvalidCharacter));
    }

    #[test]
    fn roundtrip_all_lengths() {
        for len in 0..70usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data);
        }
    }
}
