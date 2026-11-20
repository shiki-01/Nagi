//! 起動ごとのランダムトークンと、その検証（定数時間比較）。

/// OS の乱数から 16 進文字列を作る（`bytes` バイト分。文字数は 2 倍）
fn random_hex(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| format!("乱数を取得できません: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// アクセストークン（256 bit。URL とヘッダーの両方で検証する）
pub fn generate_token() -> Result<String, String> {
    random_hex(32)
}

/// CSP の nonce（128 bit。レスポンスごとに作る）
pub fn generate_nonce() -> Result<String, String> {
    random_hex(16)
}

/// 定数時間の文字列比較（長さが違うときは長さだけが漏れる。トークンの長さは固定で公開情報）
pub fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_random_hex() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(generate_nonce().unwrap().len(), 32);
    }

    #[test]
    fn compare_matches_only_equal_strings() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "ab"));
        assert!(!constant_time_eq("", "a"));
        assert!(constant_time_eq("", ""));
    }
}
