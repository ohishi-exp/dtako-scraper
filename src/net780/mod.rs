//! NET780 デジタコ生データ (バイナリ ZIP) パーサー。
//!
//! フォーマット解読の詳細・検証結果は `docs/net780-binary-format.md` を SoT とする。
//! ここは pure 関数中心 (bytes in → struct out) で、ZIP 展開やファイル I/O とは分離する
//! (Refs #18)。

mod bcd;
pub mod dsd;
pub mod evd;
pub mod gpd;
pub mod header;
pub mod inf;
mod records;
pub mod spd;

// パース結果のアップロード/API 連携は後続 issue のスコープなので、現時点ではこれらの
// re-export を消費するコードが無い (dead_code 節参照)。
#[allow(unused_imports)]
pub use dsd::DsdRecord;
#[allow(unused_imports)]
pub use evd::EvdRecord;
#[allow(unused_imports)]
pub use gpd::GpdRecord;
#[allow(unused_imports)]
pub use header::CommonHeader;
#[allow(unused_imports)]
pub use inf::InfRecord;
#[allow(unused_imports)]
pub use spd::{SpdRecord, SpeedSample};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Net780Error {
    #[error("buffer too short: expected at least {expected} bytes, got {actual}")]
    TooShort { expected: usize, actual: usize },

    #[error("invalid record marker (expected ff ff) at offset {0}")]
    InvalidMarker(usize),

    #[error("{0} trailing byte(s) could not be parsed as a complete record")]
    TrailingBytes(usize),

    #[error("invalid .inf line: {0}")]
    InvalidInf(String),

    #[error("invalid BCD digit: {0:#04x}")]
    InvalidBcd(u8),
}

#[cfg(test)]
mod tests {
    //! issue #18 の完了条件そのものを検証する結合テスト:
    //! - dsd 距離総和 = .inf 距離
    //! - ヘッダ BCD = .inf 値
    //! - evd が余りなくパースできる
    //!
    //! 実サンプル ZIP は手元に無いため、`docs/net780-binary-format.md` に記載された
    //! レイアウト・検証済みの実例値 (車両3899 / 乗務員1270) と整合する形で、小さな
    //! fixture をこのテスト内で組み立てる (バイナリ blob を repo に置くより diff で
    //! 内容を追える形を優先する)。
    use super::dsd::{total_distance_m, DsdRecord};
    use super::evd::EvdRecord;
    use super::header::{self, CommonHeader};
    use super::inf::InfRecord;
    use super::Net780Error;

    /// 256 byte 共通ヘッダを組み立てる (テスト対象外フィールドは 0 埋め)。
    fn build_header(
        device_id: &str,
        vehicle_code: u32,
        driver_code: u32,
        end_odometer_raw: u64,
    ) -> Vec<u8> {
        use super::bcd;
        let mut buf = vec![0u8; header::HEADER_LEN];
        buf[0x00..0x04].copy_from_slice(&[0x01, 0x00, 0x00, 0x93]);
        let id_bytes = device_id.as_bytes();
        buf[0x04..0x04 + id_bytes.len()].copy_from_slice(id_bytes);
        buf[0x12..0x14].copy_from_slice(&bcd::encode_u32(vehicle_code, 2));
        buf[0x16..0x18].copy_from_slice(&bcd::encode_u32(driver_code, 2));
        buf[0x18..0x1E].copy_from_slice(&bcd::encode_datetime(2026, 7, 1, 6, 2, 39));
        buf[0x1E..0x24].copy_from_slice(&bcd::encode_datetime(2026, 7, 1, 16, 37, 10));
        buf[0x24..0x2A].copy_from_slice(&bcd::encode_u64(0, 6));
        buf[0x2A..0x30].copy_from_slice(&bcd::encode_u64(end_odometer_raw, 6));
        buf
    }

    #[test]
    fn header_bcd_matches_inf_value_and_dsd_total_matches_inf_distance() {
        // 30.0 m (= 0.030 km) の走行という、小さな自己完結 fixture。
        // header の odometer diff (0.1m 単位) = 300 → distance_km() = 0.030
        let end_odometer_raw: u64 = 300;
        let header_bytes = build_header("nrbn1Sk07T", 3899, 1270, end_odometer_raw);
        let header = CommonHeader::parse(&header_bytes).expect("header parse");

        let inf_line = "0001/01/01 12:00:00,2026/07/01,0000003899,0000001270,2026/07/01 06:02:39,\
             2026/07/01 16:37:10,0.030,000:00:00,000:00:00,0.00,,,\
             6E72626E31536B3037540000,,27324455\\1\\2026\\3899\\20260701_060239-0-0-3899\r\n";
        let inf = InfRecord::parse(inf_line).expect("inf parse");

        // ヘッダ BCD = .inf 値
        assert_eq!(header.vehicle_code, inf.vehicle_code);
        assert_eq!(header.driver_code, inf.driver_code);
        assert!((header.distance_km() - inf.distance_km).abs() < 1e-9);

        // dsd 距離総和 = .inf 距離
        let dsd_bytes = {
            let mut b = Vec::new();
            b.extend_from_slice(&[0xFF, 0xFF]);
            b.extend_from_slice(&1782_900_000u32.to_le_bytes()); // ts (任意値)
            b.extend_from_slice(&0u32.to_le_bytes()); // odometer (m)
            b.extend_from_slice(&[10, 5, 7, 3, 5]); // samples, sum = 30
            b
        };
        let dsd_records = DsdRecord::parse_all(&dsd_bytes).expect("dsd parse");
        let total_m = total_distance_m(&dsd_records);
        assert_eq!(total_m, 30);
        assert!((total_m as f64 / 1000.0 - inf.distance_km).abs() < 1e-9);
    }

    #[test]
    fn evd_parses_without_remainder() {
        // 2 レコード: 通常イベント (payload 1 byte) + 0xFE/0x0A 診断ログ (ASCII payload)
        let mut buf = Vec::new();
        // record 1: ts, flags, code=0x11, subcode=0x00, len=1, payload=[0x01]
        buf.extend_from_slice(&1782_900_000u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.push(0x11);
        buf.push(0x00);
        buf.push(1);
        buf.push(0x01);
        // record 2: ts, flags, code=0xFE, subcode=0x0A, len=ascii.len(), payload=ascii
        let ascii = b"AT^SWWAN=1,1,1";
        buf.extend_from_slice(&1782_900_010u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.push(0xFE);
        buf.push(0x0A);
        buf.push(ascii.len() as u8);
        buf.extend_from_slice(ascii);

        let records = EvdRecord::parse_all(&buf).expect("evd parse without remainder");
        assert_eq!(records.len(), 2);
        assert!(!records[0].is_diagnostic());
        assert!(records[1].is_diagnostic());
        assert_eq!(
            records[1].payload_as_ascii().as_deref(),
            Some("AT^SWWAN=1,1,1")
        );

        // 末尾に不完全な余りバイトを足すと TrailingBytes になる (「余りなくパース」の裏取り)
        let mut truncated = buf.clone();
        truncated.push(0xAB);
        let err = EvdRecord::parse_all(&truncated).unwrap_err();
        assert_eq!(err, Net780Error::TrailingBytes(1));
    }
}
