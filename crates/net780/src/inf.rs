//! `.inf` — 運行メタデータ。カンマ区切り 1 行 (CRLF 終端)。
//! フィールド番号・意味は `docs/net780-binary-format.md`「.inf — 運行メタデータ」節を参照。

use chrono::{NaiveDate, NaiveDateTime};

use super::Net780Error;

#[derive(Debug, Clone, PartialEq)]
pub struct InfRecord {
    pub operation_date: NaiveDate,
    pub vehicle_code: u32,
    pub driver_code: u32,
    pub start_at: NaiveDateTime,
    pub end_at: NaiveDateTime,
    /// 走行距離 (km)。ヘッダの `distance_km()` と突合するための値。
    pub distance_km: f64,
    /// 機種ID の hex 表現 (フィールド #12)
    pub device_id_hex: String,
    /// サーバ側格納パス (フィールド #14)
    pub storage_path: String,
    /// 全フィールドの生値 (未確定フィールドへのアクセス用)
    pub fields: Vec<String>,
}

impl InfRecord {
    pub fn parse(line: &str) -> Result<Self, Net780Error> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let fields: Vec<String> = trimmed.split(',').map(str::to_string).collect();

        let field = |idx: usize| -> Result<&str, Net780Error> {
            fields
                .get(idx)
                .map(String::as_str)
                .ok_or_else(|| Net780Error::InvalidInf(format!("missing field #{idx}")))
        };

        let operation_date = NaiveDate::parse_from_str(field(1)?, "%Y/%m/%d")
            .map_err(|e| Net780Error::InvalidInf(format!("field #1 (operation_date): {e}")))?;
        let vehicle_code: u32 = field(2)?
            .parse()
            .map_err(|e| Net780Error::InvalidInf(format!("field #2 (vehicle_code): {e}")))?;
        let driver_code: u32 = field(3)?
            .parse()
            .map_err(|e| Net780Error::InvalidInf(format!("field #3 (driver_code): {e}")))?;
        let start_at = NaiveDateTime::parse_from_str(field(4)?, "%Y/%m/%d %H:%M:%S")
            .map_err(|e| Net780Error::InvalidInf(format!("field #4 (start_at): {e}")))?;
        let end_at = NaiveDateTime::parse_from_str(field(5)?, "%Y/%m/%d %H:%M:%S")
            .map_err(|e| Net780Error::InvalidInf(format!("field #5 (end_at): {e}")))?;
        let distance_km: f64 = field(6)?
            .parse()
            .map_err(|e| Net780Error::InvalidInf(format!("field #6 (distance_km): {e}")))?;
        let device_id_hex = field(12)?.to_string();
        let storage_path = field(14)?.to_string();

        Ok(Self {
            operation_date,
            vehicle_code,
            driver_code,
            start_at,
            end_at,
            distance_km,
            device_id_hex,
            storage_path,
            fields,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/net780-binary-format.md の実例 (車両3899、2026/07/01 運行)。
    const SAMPLE_LINE: &str =
        "0001/01/01 12:00:00,2026/07/01,0000003899,0000001270,2026/07/01 06:02:39,\
         2026/07/01 16:37:10,139.90,000:00:00,000:00:00,0.00,,,\
         6E72626E31536B3037540000,,27324455\\1\\2026\\3899\\20260701_060239-0-0-3899\r\n";

    #[test]
    fn parses_documented_example() {
        let inf = InfRecord::parse(SAMPLE_LINE).unwrap();
        assert_eq!(
            inf.operation_date,
            NaiveDate::from_ymd_opt(2026, 7, 1).unwrap()
        );
        assert_eq!(inf.vehicle_code, 3899);
        assert_eq!(inf.driver_code, 1270);
        assert_eq!(
            inf.start_at,
            NaiveDate::from_ymd_opt(2026, 7, 1)
                .unwrap()
                .and_hms_opt(6, 2, 39)
                .unwrap()
        );
        assert_eq!(
            inf.end_at,
            NaiveDate::from_ymd_opt(2026, 7, 1)
                .unwrap()
                .and_hms_opt(16, 37, 10)
                .unwrap()
        );
        assert!((inf.distance_km - 139.90).abs() < 1e-9);
        assert_eq!(inf.device_id_hex, "6E72626E31536B3037540000");
        assert_eq!(
            inf.storage_path,
            "27324455\\1\\2026\\3899\\20260701_060239-0-0-3899"
        );
    }

    #[test]
    fn rejects_missing_fields() {
        let err = InfRecord::parse("only,two").unwrap_err();
        assert!(matches!(err, Net780Error::InvalidInf(_)));
    }
}
