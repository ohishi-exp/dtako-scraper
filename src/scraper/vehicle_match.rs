//! 車輌名のマッチング。
//!
//! email subject 由来の vehicle_name は `(16) 十勝800か16` のような
//! `(号車番号) ナンバープレート` 形式 (全角/半角の揺れあり)。
//! F-VOS3020 の `lblVehicleName` 列は環境により `十勝800か16` だけだったり
//! `(16) 十勝800か16` だったりするため、双方を正規化して比較する。
//!
//! 正規化方針:
//! - NFKC 相当 (全角英数字・記号 → 半角) を最小限自前で行う
//! - 空白を全除去
//! - 先頭の `(数字)` 号車番号プレフィックスを切り離して別途保持
//!   (プレート部の一致を主、号車番号の一致を従の判定材料にする)

/// 全角英数字・全角括弧・全角空白を半角へ寄せ、空白を除去する。
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let c = match ch {
            // 全角数字 → 半角
            '０'..='９' => char::from_u32(ch as u32 - '０' as u32 + '0' as u32).unwrap_or(ch),
            // 全角英大文字 → 半角
            'Ａ'..='Ｚ' => char::from_u32(ch as u32 - 'Ａ' as u32 + 'A' as u32).unwrap_or(ch),
            // 全角英小文字 → 半角
            'ａ'..='ｚ' => char::from_u32(ch as u32 - 'ａ' as u32 + 'a' as u32).unwrap_or(ch),
            '（' => '(',
            '）' => ')',
            '　' => ' ',
            '－' | 'ー' | '―' | '‐' => '-',
            other => other,
        };
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' {
            continue;
        }
        out.push(c);
    }
    out
}

/// `(16)十勝800か16` → (Some("16"), "十勝800か16")
/// `十勝800か16`     → (None, "十勝800か16")
fn split_car_number(normalized: &str) -> (Option<String>, String) {
    if let Some(stripped) = normalized.strip_prefix('(') {
        if let Some(end) = stripped.find(')') {
            let num = &stripped[..end];
            let rest = &stripped[end + 1..];
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                return (Some(num.to_string()), rest.to_string());
            }
        }
    }
    (None, normalized.to_string())
}

/// email の vehicle_name と、ページ上の候補 vehicle_name が同一車輌を指すか判定する。
///
/// - プレート部 (号車番号を除いた残り) が一致すれば true
/// - 双方に号車番号があり、かつ一致する場合も true (プレート表記が異なる環境向けの保険)
pub fn vehicle_matches(email_name: &str, candidate: &str) -> bool {
    let (e_num, e_plate) = split_car_number(&normalize(email_name));
    let (c_num, c_plate) = split_car_number(&normalize(candidate));

    if !e_plate.is_empty() && e_plate == c_plate {
        return true;
    }
    // プレートが空 or 不一致でも、号車番号同士が一致すれば同一車輌とみなす。
    match (e_num, c_num) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_with_car_number_prefix_on_email_only() {
        assert!(vehicle_matches("(16) 十勝800か16", "十勝800か16"));
    }

    #[test]
    fn matches_with_prefix_on_both() {
        assert!(vehicle_matches("(16) 十勝800か16", "(16) 十勝800か16"));
    }

    #[test]
    fn matches_fullwidth_digits() {
        assert!(vehicle_matches("（１６）　十勝８００か１６", "十勝800か16"));
    }

    #[test]
    fn matches_by_car_number_when_plate_differs() {
        // プレート表記が違っても号車番号が一致すれば true
        assert!(vehicle_matches("(16) 十勝800か16", "(16) 別表記"));
    }

    #[test]
    fn rejects_different_vehicle() {
        assert!(!vehicle_matches("(16) 十勝800か16", "(17) 札幌100あ17"));
        assert!(!vehicle_matches("十勝800か16", "札幌100あ17"));
    }

    #[test]
    fn normalize_strips_spaces_and_widths() {
        assert_eq!(normalize("（１６）　十勝８００か１６"), "(16)十勝800か16");
    }

    #[test]
    fn split_extracts_car_number() {
        assert_eq!(
            split_car_number("(16)十勝800か16"),
            (Some("16".to_string()), "十勝800か16".to_string())
        );
        assert_eq!(
            split_car_number("十勝800か16"),
            (None, "十勝800か16".to_string())
        );
    }
}
