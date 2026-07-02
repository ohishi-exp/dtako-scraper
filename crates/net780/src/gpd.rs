//! `.gpd` — GPS データ。16 byte 固定長レコード。
//! `docs/net780-binary-format.md`「.gpd — GPS データ」節を参照。
//!
//! レコード: `ff ff | ts:u32le | lat:u32le | lon:u32le | b14:u8 | b15:u8`
//! lat/lon は度 × 10^6 (WGS84 と推定)。

use super::Net780Error;

/// 固定レコード長 (byte)。
pub const RECORD_LEN: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpdRecord {
    pub ts: u32,
    /// 緯度 × 10^6
    pub lat_e6: u32,
    /// 経度 × 10^6
    pub lon_e6: u32,
    pub b14: u8,
    pub b15: u8,
}

impl GpdRecord {
    pub fn lat(&self) -> f64 {
        self.lat_e6 as f64 / 1_000_000.0
    }

    pub fn lon(&self) -> f64 {
        self.lon_e6 as f64 / 1_000_000.0
    }

    pub fn parse_all(data: &[u8]) -> Result<Vec<Self>, Net780Error> {
        if data.len() % RECORD_LEN != 0 {
            return Err(Net780Error::TrailingBytes(data.len() % RECORD_LEN));
        }
        data.chunks_exact(RECORD_LEN)
            .enumerate()
            .map(|(i, rec)| Self::parse_one(rec, i * RECORD_LEN))
            .collect()
    }

    fn parse_one(rec: &[u8], offset: usize) -> Result<Self, Net780Error> {
        if rec[0] != 0xFF || rec[1] != 0xFF {
            return Err(Net780Error::InvalidMarker(offset));
        }
        let ts = u32::from_le_bytes(rec[2..6].try_into().unwrap());
        let lat_e6 = u32::from_le_bytes(rec[6..10].try_into().unwrap());
        let lon_e6 = u32::from_le_bytes(rec[10..14].try_into().unwrap());
        let b14 = rec[14];
        let b15 = rec[15];
        Ok(Self {
            ts,
            lat_e6,
            lon_e6,
            b14,
            b15,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_bytes(ts: u32, lat_e6: u32, lon_e6: u32, b14: u8, b15: u8) -> Vec<u8> {
        let mut buf = vec![0xFF, 0xFF];
        buf.extend_from_slice(&ts.to_le_bytes());
        buf.extend_from_slice(&lat_e6.to_le_bytes());
        buf.extend_from_slice(&lon_e6.to_le_bytes());
        buf.push(b14);
        buf.push(b15);
        assert_eq!(buf.len(), RECORD_LEN);
        buf
    }

    #[test]
    fn parses_two_records_and_converts_to_degrees() {
        // 長崎県沿岸部付近の座標例 (度)
        let mut data = record_bytes(1782_900_000, 32_750_000, 129_870_000, 0, 90);
        data.extend(record_bytes(1782_900_060, 32_750_100, 129_870_200, 0, 91));

        let records = GpdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 2);
        assert!((records[0].lat() - 32.75).abs() < 1e-9);
        assert!((records[0].lon() - 129.87).abs() < 1e-9);
        assert_eq!(records[1].ts, 1782_900_060);
        assert_eq!(records[1].b15, 91);
    }

    #[test]
    fn rejects_truncated_buffer() {
        let data = vec![0u8; RECORD_LEN + 3];
        let err = GpdRecord::parse_all(&data).unwrap_err();
        assert_eq!(err, Net780Error::TrailingBytes(3));
    }

    #[test]
    fn rejects_missing_marker() {
        let mut data = record_bytes(0, 0, 0, 0, 0);
        data[0] = 0x00;
        let err = GpdRecord::parse_all(&data).unwrap_err();
        assert_eq!(err, Net780Error::InvalidMarker(0));
    }
}
