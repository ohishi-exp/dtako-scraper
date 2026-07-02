//! `net780` (Rust) をブラウザから直接使うための wasm-bindgen ラッパー。
//! NET780 生データ ZIP をアップロードなしでクライアント内完結パースするための
//! バインディング (nuxt-dtako-admin の NET780 タブが consume する)。
//!
//! `ippoan/fc1200-wasm` と同じ規約に揃える: `wasm-pack build --target web` で
//! `pkg/` に npm パッケージを出力し、consumer は `file:../dtako-scraper/crates/net780-wasm/pkg`
//! として参照する。

use std::io::{Cursor, Read};

use net780::dsd::{self, DsdRecord};
use net780::evd::EvdRecord;
use net780::gpd::GpdRecord;
use net780::header::{CommonHeader, HEADER_LEN};
use net780::inf::InfRecord;
use net780::spd;
use serde::Serialize;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct HeaderSummary {
    device_id: String,
    vehicle_code: u32,
    driver_code: u32,
    start_at: String,
    end_at: String,
    distance_km: f64,
}

#[derive(Serialize)]
struct InfSummary {
    operation_date: String,
    vehicle_code: u32,
    driver_code: u32,
    start_at: String,
    end_at: String,
    distance_km: f64,
    storage_path: String,
}

#[derive(Serialize)]
struct SpeedPoint {
    record_start_ts: u32,
    offset_secs: f64,
    speed_kmh: f64,
}

#[derive(Serialize)]
struct GpsPoint {
    ts: u32,
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
struct EventSummary {
    ts: u32,
    code: u8,
    subcode: u8,
    description: Option<String>,
    payload_ascii: Option<String>,
    payload_len: usize,
}

#[derive(Serialize, Default)]
struct ParseResult {
    header: Option<HeaderSummary>,
    inf: Option<InfSummary>,
    distance_total_m: Option<u64>,
    speed: Vec<SpeedPoint>,
    gps: Vec<GpsPoint>,
    events: Vec<EventSummary>,
    /// 見つからない/パース失敗したファイルの理由 (部分的な結果でも返す)。
    warnings: Vec<String>,
}

#[wasm_bindgen(start)]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// NET780 生データ ZIP (バイト列) をパースして JS オブジェクトを返す。
/// `docs/net780-binary-format.md` の ZIP 構造上、対象のバイナリ/テキストファイルは
/// それぞれ 1 個ずつ含まれる前提。見つからないファイルは fatal にせず `warnings` に積む。
#[wasm_bindgen]
pub fn parse_net780_zip(bytes: &[u8]) -> Result<JsValue, JsValue> {
    let mut result = ParseResult::default();

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| JsValue::from_str(&format!("zip open failed: {e}")))?;

    let mut inf_text: Option<String> = None;
    let mut spd_bytes: Option<Vec<u8>> = None;
    let mut dsd_bytes: Option<Vec<u8>> = None;
    let mut gpd_bytes: Option<Vec<u8>> = None;
    let mut evd_bytes: Option<Vec<u8>> = None;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| JsValue::from_str(&format!("zip entry read failed: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_ascii_lowercase();
        let mut buf = Vec::new();
        if name.ends_with(".inf") {
            entry
                .read_to_end(&mut buf)
                .map_err(|e| JsValue::from_str(&format!("read {name} failed: {e}")))?;
            inf_text = Some(String::from_utf8_lossy(&buf).into_owned());
        } else if name.ends_with(".spd") {
            entry
                .read_to_end(&mut buf)
                .map_err(|e| JsValue::from_str(&format!("read {name} failed: {e}")))?;
            spd_bytes = Some(buf);
        } else if name.ends_with(".dsd") {
            entry
                .read_to_end(&mut buf)
                .map_err(|e| JsValue::from_str(&format!("read {name} failed: {e}")))?;
            dsd_bytes = Some(buf);
        } else if name.ends_with(".gpd") {
            entry
                .read_to_end(&mut buf)
                .map_err(|e| JsValue::from_str(&format!("read {name} failed: {e}")))?;
            gpd_bytes = Some(buf);
        } else if name.ends_with(".evd") {
            entry
                .read_to_end(&mut buf)
                .map_err(|e| JsValue::from_str(&format!("read {name} failed: {e}")))?;
            evd_bytes = Some(buf);
        }
    }

    if let Some(text) = &inf_text {
        match InfRecord::parse(text) {
            Ok(inf) => result.inf = Some(inf_to_summary(&inf)),
            Err(e) => result.warnings.push(format!(".inf parse failed: {e}")),
        }
    } else {
        result
            .warnings
            .push(".inf ファイルが ZIP 内に見つからない".to_string());
    }

    // 共通ヘッダはどのバイナリファイルの先頭にも同じ内容が入っている (docs 参照)。
    // .dsd を優先し、無ければ他のバイナリファイルから読む。
    let header_source = dsd_bytes
        .as_deref()
        .or(spd_bytes.as_deref())
        .or(gpd_bytes.as_deref())
        .or(evd_bytes.as_deref());
    if let Some(bytes) = header_source {
        match CommonHeader::parse(bytes) {
            Ok(header) => result.header = Some(header_to_summary(&header)),
            Err(e) => result.warnings.push(format!("共通ヘッダのパース失敗: {e}")),
        }
    } else {
        result
            .warnings
            .push("バイナリファイル (.spd/.dsd/.gpd/.evd) が ZIP 内に見つからない".to_string());
    }

    if let Some(bytes) = &dsd_bytes {
        match DsdRecord::parse_all(&bytes[HEADER_LEN.min(bytes.len())..]) {
            Ok(records) => result.distance_total_m = Some(dsd::total_distance_m(&records)),
            Err(e) => result.warnings.push(format!(".dsd parse failed: {e}")),
        }
    }

    if let Some(bytes) = &spd_bytes {
        match spd::parse_speed_series(&bytes[HEADER_LEN.min(bytes.len())..]) {
            Ok(series) => {
                result.speed = series
                    .into_iter()
                    .map(|s| SpeedPoint {
                        record_start_ts: s.record_start_ts,
                        offset_secs: s.offset_secs,
                        speed_kmh: s.speed_kmh,
                    })
                    .collect();
            }
            Err(e) => result.warnings.push(format!(".spd parse failed: {e}")),
        }
    }

    if let Some(bytes) = &gpd_bytes {
        match GpdRecord::parse_all(&bytes[HEADER_LEN.min(bytes.len())..]) {
            Ok(records) => {
                result.gps = records
                    .into_iter()
                    .map(|r| GpsPoint {
                        ts: r.ts,
                        lat: r.lat(),
                        lon: r.lon(),
                    })
                    .collect();
            }
            Err(e) => result.warnings.push(format!(".gpd parse failed: {e}")),
        }
    }

    if let Some(bytes) = &evd_bytes {
        match EvdRecord::parse_all(&bytes[HEADER_LEN.min(bytes.len())..]) {
            Ok(records) => {
                result.events = records
                    .into_iter()
                    .map(|r| EventSummary {
                        ts: r.ts,
                        code: r.code,
                        subcode: r.subcode,
                        description: r.known_description().map(str::to_string),
                        payload_ascii: r.payload_as_ascii(),
                        payload_len: r.payload.len(),
                    })
                    .collect();
            }
            Err(e) => result.warnings.push(format!(".evd parse failed: {e}")),
        }
    }

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsValue::from_str(&format!("serialize failed: {e}")))
}

fn header_to_summary(header: &CommonHeader) -> HeaderSummary {
    HeaderSummary {
        device_id: header.device_id.clone(),
        vehicle_code: header.vehicle_code,
        driver_code: header.driver_code,
        start_at: header.start_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        end_at: header.end_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        distance_km: header.distance_km(),
    }
}

fn inf_to_summary(inf: &InfRecord) -> InfSummary {
    InfSummary {
        operation_date: inf.operation_date.format("%Y-%m-%d").to_string(),
        vehicle_code: inf.vehicle_code,
        driver_code: inf.driver_code,
        start_at: inf.start_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        end_at: inf.end_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
        distance_km: inf.distance_km,
        storage_path: inf.storage_path.clone(),
    }
}
