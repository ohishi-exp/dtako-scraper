//! `.spd` — 速度データ。可変長レコードの列。
//! `docs/net780-binary-format.md`「.spd — 速度データ」節を参照。
//!
//! レコード: `ff ff | ts:u32le | f1:u16le | f2:u8 | samples:u8[] (次の ff ff まで)`
//! sample = 前サンプルからの速度変化量 (単位 0.1 km/h、127 = ±0 の符号付きオフセット)。
//! サンプリング周期は 0.5 秒固定。

use super::records::split_marker_records;
use super::Net780Error;

/// サンプリング周期 (秒)。
pub const SAMPLE_INTERVAL_SECS: f64 = 0.5;

/// sample byte のゼロ点オフセット (127 = 変化量 0)。
const SAMPLE_ZERO_OFFSET: i32 = 127;

/// sample 1 count あたりの速度変化量 (km/h)。
const SAMPLE_UNIT_KMH: f64 = 0.1;

/// レコード先頭の固定長部分 (ts:4 + f1:2 + f2:1)。
const RECORD_FIXED_LEN: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpdRecord {
    pub start_ts: u32,
    pub f1: u16,
    pub f2: u8,
    pub samples: Vec<u8>,
}

/// `.dsd` の走行距離 (m 正の値) と併用して速度時系列を作る際の 1 サンプル。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedSample {
    /// レコード先頭時刻 (docs 記載通り、JST 壁時計をそのまま格納した UNIX 秒)
    pub record_start_ts: u32,
    /// レコード先頭からの経過秒数
    pub offset_secs: f64,
    /// 累積速度 (km/h)。ドリフトで負に振れ得るため 0 で clamp 済み。
    pub speed_kmh: f64,
}

impl SpdRecord {
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
        let f1 = u16::from_le_bytes(rec[4..6].try_into().unwrap());
        let f2 = rec[6];
        let samples = rec[RECORD_FIXED_LEN..].to_vec();
        Ok(Self {
            start_ts,
            f1,
            f2,
            samples,
        })
    }

    /// サンプル列を累積和で速度時系列に変換する。負値は 0 で clamp する
    /// (docs: 「累積和は負に振れ得る (ドリフト)。実装では 0 で clamp する」)。
    pub fn speed_series(&self) -> Vec<SpeedSample> {
        let mut cumulative: f64 = 0.0;
        self.samples
            .iter()
            .enumerate()
            .map(|(i, &sample)| {
                let delta = (sample as i32 - SAMPLE_ZERO_OFFSET) as f64 * SAMPLE_UNIT_KMH;
                cumulative = (cumulative + delta).max(0.0);
                SpeedSample {
                    record_start_ts: self.start_ts,
                    offset_secs: i as f64 * SAMPLE_INTERVAL_SECS,
                    speed_kmh: cumulative,
                }
            })
            .collect()
    }
}

/// 全レコードを速度時系列 (0.5 秒粒度) にフラット化する。
pub fn parse_speed_series(data: &[u8]) -> Result<Vec<SpeedSample>, Net780Error> {
    Ok(SpdRecord::parse_all(data)?
        .iter()
        .flat_map(SpdRecord::speed_series)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_bytes(ts: u32, f1: u16, f2: u8, samples: &[u8]) -> Vec<u8> {
        let mut buf = vec![0xFF, 0xFF];
        buf.extend_from_slice(&ts.to_le_bytes());
        buf.extend_from_slice(&f1.to_le_bytes());
        buf.push(f2);
        buf.extend_from_slice(samples);
        buf
    }

    #[test]
    fn parses_single_record() {
        let data = record_bytes(1782_900_000, 42, 3, &[127, 137, 127, 117]);
        let records = SpdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.start_ts, 1782_900_000);
        assert_eq!(r.f1, 42);
        assert_eq!(r.f2, 3);
        assert_eq!(r.samples, vec![127, 137, 127, 117]);
    }

    #[test]
    fn speed_series_accumulates_and_clamps_at_zero() {
        // deltas (km/h): 0, +1.0, 0, -1.0 → cumulative: 0, 1.0, 1.0, 0.0
        let data = record_bytes(1782_900_000, 0, 0, &[127, 137, 127, 117]);
        let series = parse_speed_series(&data).unwrap();
        let speeds: Vec<f64> = series.iter().map(|s| s.speed_kmh).collect();
        assert_eq!(speeds, vec![0.0, 1.0, 1.0, 0.0]);
        assert_eq!(series[1].offset_secs, 0.5);

        // 大きく負に振れるケース → 0 で clamp される
        let data = record_bytes(0, 0, 0, &[127, 0]); // delta = -12.7 km/h
        let series = parse_speed_series(&data).unwrap();
        assert_eq!(series[1].speed_kmh, 0.0);
    }

    #[test]
    fn parses_multiple_records() {
        let mut data = record_bytes(100, 1, 1, &[127, 130]);
        data.extend(record_bytes(200, 2, 2, &[127]));
        let records = SpdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].start_ts, 100);
        assert_eq!(records[1].start_ts, 200);
    }
}
