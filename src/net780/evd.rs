//! `.evd` — イベント / エラーログ。マーカー無しで詰めて格納された可変長レコード列。
//! `docs/net780-binary-format.md`「.evd — イベント / エラーログ」節を参照。
//!
//! レコード: `ts:u32le | flags:u16le | code:u8 | subcode:u8 | len:u8 | payload[len]`

use super::Net780Error;

/// レコード先頭の固定長部分 (ts:4 + flags:2 + code:1 + subcode:1 + len:1)。
const HEADER_LEN: usize = 9;

/// 診断 / エラーログのイベントコード (`0xFE`)。subcode で内容が分かれる。
pub const DIAGNOSTIC_CODE: u8 = 0xFE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvdRecord {
    pub ts: u32,
    pub flags: u16,
    pub code: u8,
    pub subcode: u8,
    pub payload: Vec<u8>,
}

/// 判明しているイベント (code, subcode) の説明カタログ。
/// `docs/net780-binary-format.md`「.evd — イベント / エラーログ」の 2 つの表が SoT。
/// subcode を区別しないコードは `subcode: None` にワイルドカードで登録する。
///
/// legacy PHP システム (`yhonda-ohishi/nginx` の `dtako_config_event_bases` /
/// `DtakoConfigEventsController::checkData()`) にも、同じ発想の「既知の
/// (id1, id2) 2 byte コード → 説明文」ルックアップ + 出現回数カウントの仕組みがある
/// (car_id・datetime 単位の config バイナリを byte 列として走査する)。本カタログは
/// それの NET780 `.evd` 版に相当する。
const KNOWN_EVENTS: &[(u8, Option<u8>, &str)] = &[
    (
        0xA0,
        None,
        "定常イベント (運行開始/状態遷移ハートビート系、0xA0-0xAB)",
    ),
    (0x11, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x12, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x16, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x17, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x21, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x22, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x26, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0x27, None, "作業状態 ON/OFF (荷積・荷降ボタン等と推定)"),
    (0xB8, None, "通信断"),
    (0xB9, None, "通信復帰"),
    (0xC1, None, "サーバ交信プロトコルログ"),
    (0xD2, None, "運行終了サマリ"),
    (0xFB, None, "起動時情報"),
    (0xFC, None, "起動時情報"),
    (DIAGNOSTIC_CODE, Some(0x05), "診断: リトライカウンタ類"),
    (DIAGNOSTIC_CODE, Some(0x06), "診断: リトライカウンタ類"),
    (DIAGNOSTIC_CODE, Some(0x07), "診断: リトライカウンタ類"),
    (
        DIAGNOSTIC_CODE,
        Some(0x0A),
        "診断: AT コマンドログ / エラー",
    ),
    (DIAGNOSTIC_CODE, Some(0x0E), "診断: モデム状態"),
    (
        DIAGNOSTIC_CODE,
        Some(0x0F),
        "診断: デバイスパス (モデム再初期化)",
    ),
];

impl EvdRecord {
    pub fn is_diagnostic(&self) -> bool {
        self.code == DIAGNOSTIC_CODE
    }

    /// 判明済みイベントカタログから説明文を引く。未登録の (code, subcode) は `None`。
    pub fn known_description(&self) -> Option<&'static str> {
        KNOWN_EVENTS
            .iter()
            .find(|(code, subcode, _)| {
                *code == self.code && subcode.is_none_or(|s| s == self.subcode)
            })
            .map(|(_, _, detail)| *detail)
    }

    /// payload を ASCII として読む (診断ログの AT コマンド / デバイスパス等)。
    /// 非 UTF-8 payload (バイナリ系イベント) では `None`。
    pub fn payload_as_ascii(&self) -> Option<String> {
        std::str::from_utf8(&self.payload).ok().map(str::to_string)
    }

    /// バッファ全体を先頭からレコードとして詰めてパースする。
    /// 1 byte でも余ればエラー (docs: 「1 byte の余りなくパース可能」が構造確定の根拠)。
    pub fn parse_all(data: &[u8]) -> Result<Vec<Self>, Net780Error> {
        let mut records = Vec::new();
        let mut i = 0;
        while i < data.len() {
            if data.len() - i < HEADER_LEN {
                return Err(Net780Error::TrailingBytes(data.len() - i));
            }
            let ts = u32::from_le_bytes(data[i..i + 4].try_into().unwrap());
            let flags = u16::from_le_bytes(data[i + 4..i + 6].try_into().unwrap());
            let code = data[i + 6];
            let subcode = data[i + 7];
            let len = data[i + 8] as usize;
            let payload_start = i + HEADER_LEN;
            let payload_end = payload_start + len;
            if payload_end > data.len() {
                return Err(Net780Error::TrailingBytes(data.len() - i));
            }
            records.push(Self {
                ts,
                flags,
                code,
                subcode,
                payload: data[payload_start..payload_end].to_vec(),
            });
            i = payload_end;
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_bytes(ts: u32, flags: u16, code: u8, subcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&ts.to_le_bytes());
        buf.extend_from_slice(&flags.to_le_bytes());
        buf.push(code);
        buf.push(subcode);
        buf.push(payload.len() as u8);
        buf.extend_from_slice(payload);
        buf
    }

    #[test]
    fn parses_heartbeat_and_diagnostic_records() {
        let mut data = record_bytes(1782_900_000, 0x0001, 0xA0, 0x00, &[]);
        data.extend(record_bytes(
            1782_900_010,
            0x0000,
            DIAGNOSTIC_CODE,
            0x0F,
            b"dev/ttyACM0",
        ));

        let records = EvdRecord::parse_all(&data).unwrap();
        assert_eq!(records.len(), 2);
        assert!(!records[0].is_diagnostic());
        assert_eq!(records[0].payload, Vec::<u8>::new());

        assert!(records[1].is_diagnostic());
        assert_eq!(records[1].subcode, 0x0F);
        assert_eq!(
            records[1].payload_as_ascii().as_deref(),
            Some("dev/ttyACM0")
        );
    }

    #[test]
    fn rejects_trailing_partial_header() {
        let mut data = record_bytes(0, 0, 0x11, 0x00, &[0x01]);
        data.push(0xAB); // 9 byte 未満のヘッダ断片
        let err = EvdRecord::parse_all(&data).unwrap_err();
        assert_eq!(err, Net780Error::TrailingBytes(1));
    }

    #[test]
    fn rejects_payload_length_exceeding_buffer() {
        let mut data = record_bytes(0, 0, 0x11, 0x00, &[0x01, 0x02]);
        data.truncate(data.len() - 1); // len=2 と宣言したのに payload を 1 byte しか積まない
        let err = EvdRecord::parse_all(&data).unwrap_err();
        assert!(matches!(err, Net780Error::TrailingBytes(_)));
    }

    #[test]
    fn known_description_matches_wildcard_and_subcode_specific_entries() {
        let data = record_bytes(0, 0, 0xA0, 0x03, &[]); // 0xA0 系はどの subcode でもワイルドカード一致
        let records = EvdRecord::parse_all(&data).unwrap();
        assert!(records[0]
            .known_description()
            .unwrap()
            .contains("定常イベント"));

        let data = record_bytes(0, 0, DIAGNOSTIC_CODE, 0x0A, &[]);
        let records = EvdRecord::parse_all(&data).unwrap();
        assert_eq!(
            records[0].known_description(),
            Some("診断: AT コマンドログ / エラー")
        );

        let data = record_bytes(0, 0, DIAGNOSTIC_CODE, 0x99, &[]); // 未登録 subcode
        let records = EvdRecord::parse_all(&data).unwrap();
        assert_eq!(records[0].known_description(), None);
    }
}
