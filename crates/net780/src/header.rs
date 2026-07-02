//! バイナリ共通ヘッダ (.spd/.dsd/.gpd/.rvd/.evd/.tpd 先頭 0x100 byte)。
//! レイアウトは `docs/net780-binary-format.md`「バイナリ共通ヘッダ」節を参照。

use chrono::{NaiveDate, NaiveDateTime};

use super::bcd;
use super::Net780Error;

/// 共通ヘッダの長さ (byte)。データ部はこの直後 (offset 0x100) から始まる。
pub const HEADER_LEN: usize = 0x100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommonHeader {
    pub magic: [u8; 4],
    pub device_id: String,
    pub vehicle_code: u32,
    pub driver_code: u32,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    /// 開始時オドメーター (0.1m 単位の raw 値)
    pub start_odometer_raw: u64,
    /// 終了時オドメーター (0.1m 単位の raw 値)
    pub end_odometer_raw: u64,
}

impl CommonHeader {
    pub fn parse(data: &[u8]) -> Result<Self, Net780Error> {
        if data.len() < HEADER_LEN {
            return Err(Net780Error::TooShort {
                expected: HEADER_LEN,
                actual: data.len(),
            });
        }

        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[0x00..0x04]);

        let device_id = String::from_utf8_lossy(&data[0x04..0x0E])
            .trim_end_matches('\0')
            .to_string();

        let vehicle_code = bcd::decode_u32(&data[0x12..0x14])?;
        let driver_code = bcd::decode_u32(&data[0x16..0x18])?;
        let start_at = parse_bcd_datetime(&data[0x18..0x1E])?;
        let end_at = parse_bcd_datetime(&data[0x1E..0x24])?;
        let start_odometer_raw = bcd::decode_u64(&data[0x24..0x2A])?;
        let end_odometer_raw = bcd::decode_u64(&data[0x2A..0x30])?;

        Ok(Self {
            magic,
            device_id,
            vehicle_code,
            driver_code,
            start_at,
            end_at,
            start_odometer_raw,
            end_odometer_raw,
        })
    }

    /// 走行距離 (km)。オドメーターは 0.1m 単位なので raw の差分を 10,000 で割る。
    pub fn distance_km(&self) -> f64 {
        self.end_odometer_raw
            .saturating_sub(self.start_odometer_raw) as f64
            / 10_000.0
    }
}

fn parse_bcd_datetime(bytes: &[u8]) -> Result<NaiveDateTime, Net780Error> {
    let yy = bcd::decode_u32(&bytes[0..1])? as i32 + 2000;
    let mm = bcd::decode_u32(&bytes[1..2])?;
    let dd = bcd::decode_u32(&bytes[2..3])?;
    let hh = bcd::decode_u32(&bytes[3..4])?;
    let mi = bcd::decode_u32(&bytes[4..5])?;
    let ss = bcd::decode_u32(&bytes[5..6])?;
    NaiveDate::from_ymd_opt(yy, mm, dd)
        .and_then(|d| d.and_hms_opt(hh, mi, ss))
        .ok_or_else(|| {
            Net780Error::InvalidInf(format!(
                "invalid BCD datetime: {yy:04}-{mm:02}-{dd:02} {hh:02}:{mi:02}:{ss:02}"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/net780-binary-format.md の実例 (車両3899、2026/07/01 運行)。
    /// オドメーター diff = 1,399,050 (0.1m 単位) = 139.905 km (.inf の 139.90 と一致)。
    fn sample_header() -> Vec<u8> {
        let mut buf = vec![0u8; HEADER_LEN];
        buf[0x00..0x04].copy_from_slice(&[0x01, 0x00, 0x00, 0x93]);
        buf[0x04..0x0E].copy_from_slice(b"nrbn1Sk07T");
        buf[0x12..0x14].copy_from_slice(&[0x38, 0x99]); // 3899
        buf[0x16..0x18].copy_from_slice(&[0x12, 0x70]); // 1270
        buf[0x18..0x1E].copy_from_slice(&[0x26, 0x07, 0x01, 0x06, 0x02, 0x39]);
        buf[0x1E..0x24].copy_from_slice(&[0x26, 0x07, 0x01, 0x16, 0x37, 0x10]);
        buf[0x24..0x2A].copy_from_slice(&[0x00, 0x91, 0x91, 0x74, 0x50, 0x60]);
        buf[0x2A..0x30].copy_from_slice(&[0x00, 0x91, 0x93, 0x14, 0x41, 0x10]);
        buf
    }

    #[test]
    fn parses_documented_example() {
        let header = CommonHeader::parse(&sample_header()).unwrap();
        assert_eq!(header.magic, [0x01, 0x00, 0x00, 0x93]);
        assert_eq!(header.device_id, "nrbn1Sk07T");
        assert_eq!(header.vehicle_code, 3899);
        assert_eq!(header.driver_code, 1270);
        assert_eq!(
            header.start_at,
            NaiveDate::from_ymd_opt(2026, 7, 1)
                .unwrap()
                .and_hms_opt(6, 2, 39)
                .unwrap()
        );
        assert_eq!(
            header.end_at,
            NaiveDate::from_ymd_opt(2026, 7, 1)
                .unwrap()
                .and_hms_opt(16, 37, 10)
                .unwrap()
        );
        assert_eq!(header.start_odometer_raw, 9_191_745_060);
        assert_eq!(header.end_odometer_raw, 9_193_144_110);
        assert!((header.distance_km() - 139.905).abs() < 1e-9);
    }

    #[test]
    fn rejects_short_buffer() {
        let err = CommonHeader::parse(&[0u8; 10]).unwrap_err();
        assert_eq!(
            err,
            Net780Error::TooShort {
                expected: HEADER_LEN,
                actual: 10
            }
        );
    }
}
