//! `ff ff` マーカーで区切られた可変長レコード列の共通分割ロジック。
//! `.spd` / `.dsd` (/ `.rvd`) が使う `docs/net780-binary-format.md` のレコード構造:
//! `ff ff | <固定長ヘッダ> | samples:u8[] (次の ff ff まで)`。
//!
//! 固定長ヘッダの直後 (= サンプル領域) にたまたま `ff ff` が現れる可能性は排除
//! できないため、`min_content_len` バイトぶんはマーカー探索をスキップして
//! 少なくとも 1 レコード分の固定長ヘッダを誤検知しないようにする。

use super::Net780Error;

pub fn split_marker_records(
    data: &[u8],
    min_content_len: usize,
) -> Result<Vec<&[u8]>, Net780Error> {
    let mut records = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if i + 2 > data.len() || data[i] != 0xFF || data[i + 1] != 0xFF {
            return Err(Net780Error::InvalidMarker(i));
        }
        let content_start = i + 2;
        let search_from = content_start + min_content_len;
        let mut next = data.len();
        let mut j = search_from;
        while j + 1 < data.len() {
            if data[j] == 0xFF && data[j + 1] == 0xFF {
                next = j;
                break;
            }
            j += 1;
        }
        if next < content_start {
            return Err(Net780Error::TooShort {
                expected: min_content_len,
                actual: next.saturating_sub(content_start),
            });
        }
        records.push(&data[content_start..next]);
        i = next;
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_two_records() {
        let mut data = vec![0xFF, 0xFF];
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7]); // record 1 content (7 bytes)
        data.extend_from_slice(&[0xFF, 0xFF]);
        data.extend_from_slice(&[8, 9, 10, 11, 12, 13, 14, 99]); // record 2 content (8 bytes)

        let records = split_marker_records(&data, 7).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], &[1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(records[1], &[8, 9, 10, 11, 12, 13, 14, 99]);
    }

    #[test]
    fn rejects_missing_leading_marker() {
        let data = [1, 2, 3, 4];
        let err = split_marker_records(&data, 0).unwrap_err();
        assert_eq!(err, Net780Error::InvalidMarker(0));
    }

    #[test]
    fn does_not_split_within_min_content_len() {
        // 固定長ヘッダの中に偶然 ff ff が現れても、min_content_len に達するまでは
        // 次レコードの開始とみなさない。
        let mut data = vec![0xFF, 0xFF];
        data.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0]); // 5 byte 固定ヘッダ (先頭2byteがff ff)
        data.extend_from_slice(&[1, 2, 3]); // samples
        let records = split_marker_records(&data, 5).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0], &[0xFF, 0xFF, 0, 0, 0, 1, 2, 3]);
    }
}
