//! BCD (Binary-Coded Decimal) ヘルパー。NET780 のバイナリ共通ヘッダは数値を
//! 1 byte = 10進 2 桁の BCD で持つ (docs/net780-binary-format.md 参照)。

use super::Net780Error;

fn digit(nibble: u8) -> Result<u32, Net780Error> {
    if nibble > 9 {
        return Err(Net780Error::InvalidBcd(nibble));
    }
    Ok(nibble as u32)
}

fn byte_to_u32(b: u8) -> Result<u32, Net780Error> {
    let hi = digit(b >> 4)?;
    let lo = digit(b & 0x0F)?;
    Ok(hi * 10 + lo)
}

/// 複数 byte の BCD を 1 つの整数に連結する (先頭 byte が上位 2 桁)。
pub fn decode_u32(bytes: &[u8]) -> Result<u32, Net780Error> {
    let mut acc: u32 = 0;
    for &b in bytes {
        acc = acc * 100 + byte_to_u32(b)?;
    }
    Ok(acc)
}

/// [`decode_u32`] の u64 版 (オドメーター等、u32 に収まらない桁数向け)。
pub fn decode_u64(bytes: &[u8]) -> Result<u64, Net780Error> {
    let mut acc: u64 = 0;
    for &b in bytes {
        acc = acc * 100 + byte_to_u32(b)? as u64;
    }
    Ok(acc)
}

/// テスト fixture 組み立て専用の逆変換。本番コードパスからは呼ばない。
#[cfg(test)]
pub fn encode_u64(value: u64, len_bytes: usize) -> Vec<u8> {
    let digits = format!("{:0width$}", value, width = len_bytes * 2);
    let digits = digits.as_bytes();
    (0..len_bytes)
        .map(|i| {
            let hi = digits[i * 2] - b'0';
            let lo = digits[i * 2 + 1] - b'0';
            (hi << 4) | lo
        })
        .collect()
}

#[cfg(test)]
pub fn encode_u32(value: u32, len_bytes: usize) -> Vec<u8> {
    encode_u64(value as u64, len_bytes)
}

#[cfg(test)]
pub fn encode_datetime(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> Vec<u8> {
    let yy = (year.rem_euclid(100)) as u64;
    [
        encode_u64(yy, 1),
        encode_u64(month as u64, 1),
        encode_u64(day as u64, 1),
        encode_u64(hour as u64, 1),
        encode_u64(min as u64, 1),
        encode_u64(sec as u64, 1),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_matches_spec_example_vehicle_code() {
        // docs/net780-binary-format.md: 車両CD `38 99` → 3899
        assert_eq!(decode_u32(&[0x38, 0x99]).unwrap(), 3899);
    }

    #[test]
    fn decode_matches_spec_example_odometer() {
        // docs/net780-binary-format.md: 開始時オドメーター
        // `00 91 91 74 50 60` → 919,174,506.0 m (0.1m 単位、raw = 9,191,745,060)
        assert_eq!(
            decode_u64(&[0x00, 0x91, 0x91, 0x74, 0x50, 0x60]).unwrap(),
            9_191_745_060
        );
    }

    #[test]
    fn decode_rejects_non_bcd_nibble() {
        assert_eq!(decode_u32(&[0xFA]), Err(Net780Error::InvalidBcd(0x0F)));
    }

    #[test]
    fn encode_decode_roundtrip() {
        let bytes = encode_u32(3899, 2);
        assert_eq!(bytes, vec![0x38, 0x99]);
        assert_eq!(decode_u32(&bytes).unwrap(), 3899);
    }

    #[test]
    fn encode_datetime_matches_spec_example() {
        // docs/net780-binary-format.md: 運行開始日時 (BCD) `26 07 01 06 02 39`
        assert_eq!(
            encode_datetime(2026, 7, 1, 6, 2, 39),
            vec![0x26, 0x07, 0x01, 0x06, 0x02, 0x39]
        );
    }
}
