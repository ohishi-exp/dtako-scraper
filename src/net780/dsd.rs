//! `.dsd` — 走行距離データ。可変長レコードの列。
//! `docs/net780-binary-format.md`「.dsd — 走行距離データ」節を参照。
//!
//! レコード: `ff ff | ts:u32le | odometer:u32le | samples:u8[] (次の ff ff まで)`
//! sample = 0.5 秒間の走行距離 (m、整数)。全サンプルの総和が .inf / ヘッダの走行距離と一致する
//! (最重要の検証結果)。

use super::records::split_marker_records;
use super::Net780Error;

/// サンプリング周期 (秒)。`.spd` と同じ 0.5 秒。
pub const SAMPLE_INTERVAL_SECS: f64 = 0.5;

/// レコード先頭の固定長部分 (ts:4 + odometer:4)。
const RECORD_FIXED_LEN: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DsdRecord {
    pub start_ts: u32,
    /// 車両積算距離 (m、ヘッダ BCD オドメーターの 1/10 値と一致)
    pub odometer_m: u32,
    /// 0.5 秒毎の走行距離 (m)
    pub samples: Vec<u8>,
}

impl DsdRecord {
    pub fn parse_all(data: &[u8]) -> Result<Vec<Self>, Net780Error> {
        split_marker_records(data, RECORD_FIXED_LEN)?
            .into_iter()
            .map(Self::parse_one)
            .collect()
    }

    fn parse_one(rec: &[u8]) -> Result<Self, Net780Error> {
        if rec.len() < RECORD_FIXED_LEN {
            return Err(Net780Error::TooShort {
                expected: RECORD_FIXED_LEN,
                actual: rec.len(),
            });
        }
        let start_ts = u32::from_le_bytes(rec[0..4].try_into().unwrap());
        let odometer_m = u32::from_le_bytes(rec[4..8].try_into().unwrap());
        let samples = rec[RECORD_FIXED_LEN..].to_vec();
        Ok(Self {
            start_ts,
            odometer_m,
            samples,
        })
    }
}

/// 全レコードのサンプル総和 (m)。`.inf` / ヘッダの走行距離との突合に使う
/// (docs: 「全サンプルの総和 = 139,921 m = .inf の 139.90 km と完全一致」)。
pub fn total_distance_m(records: &[DsdRecord]) -> u64 {
    records
        .iter()
        .flat_map(|r| r.samples.iter())
        .map(|&s| s as u64)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_bytes(ts: u32, odometer_m: u32, samples: &[u8]) -> Vec<u8> {
        let mut buf = vec![0xFF, 0xFF];
        buf.extend_from_slice(&ts.to_le_bytes());
        buf.extend_from_slice(&odometer_m.to_le_bytes());
        buf.extend_from_slice(samples);
        buf
    }

    #[test]
    fn parses_single_record() {
        let data = record_bytes(1782_900_000, 919_174_506, &[3, 4, 5]);
        let records = DsdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].start_ts, 1782_900_000);
        assert_eq!(records[0].odometer_m, 919_174_506);
        assert_eq!(records[0].samples, vec![3, 4, 5]);
    }

    #[test]
    fn total_distance_sums_all_records() {
        let mut data = record_bytes(100, 0, &[1, 2, 3]);
        data.extend(record_bytes(200, 6, &[4, 5]));
        let records = DsdRecord::parse_all(&data).unwrap();
        assert_eq!(total_distance_m(&records), 1 + 2 + 3 + 4 + 5);
    }
}
