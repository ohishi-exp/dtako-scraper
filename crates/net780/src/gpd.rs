//! `.gpd` — GPS データ。`ff ff` マーカー付き 16 byte 固定長レコード。
//! `docs/net780-binary-format.md`「.gpd — GPS データ」節を参照。
//!
//! レコード: `ff ff | ts:u32le | lat:u32le | lon:u32le | b14:u8 | b15:u8`
//! lat/lon は度 × 10^6 (WGS84 と推定)。
//!
//! 実データ検証 (2026-07-03) で、GPS レコード同士の間に **未解読の可変長ブロック**
//! (4 byte 単位の繰り返しパターン、位置サンプルより高頻度に挿入されている) が
//! 挟まっていることが判明した。単純に「ヘッダ直後から純粋な固定長配列」として
//! `chunks_exact(16)` で読むと、この不明ブロックの分だけレコード境界がずれて
//! パース自体が即座に失敗し、GPS 点が 1 件も取れなくなる。
//!
//! そのため `.spd`/`.dsd` と同様に `ff ff` マーカーをスキャンして GPS レコード
//! だけを拾い、マーカーではないバイト列 (未解読の挿入ブロック) は読み飛ばす方式に
//! している。挿入ブロックの意味は未確定 (方位/精度/衛星数等の高頻度サブサンプルの
//! 可能性があるが未検証) — 現時点では GPS 位置情報の抽出のみを目的とし、この
//! ブロックの内容は破棄する。

use super::Net780Error;

/// 固定レコード長 (byte、`ff ff` マーカー含む)。
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

    /// `ff ff` マーカーをスキャンして GPS レコードを拾う。マーカーではないバイト
    /// (未解読の挿入ブロック) は読み飛ばす。末尾に 16 byte 未満のマーカー付き
    /// 断片が残った場合は不完全なレコードとして無視する (エラーにしない)。
    pub fn parse_all(data: &[u8]) -> Result<Vec<Self>, Net780Error> {
        let mut records = Vec::new();
        let mut i = 0;
        while i + 1 < data.len() {
            if data[i] == 0xFF && data[i + 1] == 0xFF {
                if i + RECORD_LEN > data.len() {
                    break;
                }
                records.push(Self::parse_one(&data[i..i + RECORD_LEN], i)?);
                i += RECORD_LEN;
            } else {
                i += 1;
            }
        }
        Ok(records)
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
    fn skips_unknown_bytes_between_records_instead_of_failing() {
        // 実データ検証 (2026-07-03) で判明した「GPS レコード間に挟まる未解読の
        // 可変長ブロック」を再現する回帰テスト。以前の実装 (chunks_exact 前提) だと
        // このブロックの分だけ境界がずれて全体が parse エラーになり GPS 点が
        // 0 件になっていた。
        let mut data = record_bytes(1782_900_000, 32_750_000, 129_870_000, 0, 90);
        // 未解読ブロック (4 byte 単位のパターン、実データ観測値を模した非マーカーの
        // バイト列。'ff ff' を含まないことだけが重要)。
        data.extend_from_slice(&[0x00, 0xfa, 0xef, 0x7f, 0x00, 0xfa, 0xaf, 0x7f]);
        data.extend(record_bytes(1782_900_060, 32_750_100, 129_870_200, 0, 91));

        let records = GpdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].ts, 1782_900_000);
        assert_eq!(records[1].ts, 1782_900_060);
    }

    #[test]
    fn all_zero_buffer_yields_no_records_without_erroring() {
        // マーカーが 1 つも見つからない場合は空の Vec を返す (以前の実装は
        // TrailingBytes エラーにしていたが、未解読ブロックを許容する設計上、
        // 「マーカーが見つからない」こと自体はエラーではない)。
        let data = vec![0u8; RECORD_LEN + 3];
        let records = GpdRecord::parse_all(&data).unwrap();
        assert!(records.is_empty());
    }

    #[test]
    fn skips_leading_corrupted_marker_and_finds_later_record() {
        // 先頭のマーカーが壊れていても、後方に見つかった正常なレコードは拾う。
        let mut data = record_bytes(0, 1, 2, 0, 0);
        data[0] = 0x00; // 先頭マーカーを破壊 (もう ff ff ではない)
        data.extend(record_bytes(100, 10, 20, 0, 0));

        let records = GpdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].ts, 100);
    }
}
