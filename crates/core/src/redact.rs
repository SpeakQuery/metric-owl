use regex::Regex;
use std::sync::LazyLock;
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(authorization|api[_-]?key|access[_-]?token|password|secret|cookie|token)([\s\"':=]+)([^\s,;\"}]+)"#).unwrap()
});
static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)bearer\s+[A-Za-z0-9._~+/=-]+").unwrap());
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}").unwrap());
pub fn redact(input: &str, limit: usize) -> String {
    let x = BEARER.replace_all(input, "Bearer [REDACTED]");
    let x = SECRET.replace_all(&x, "$1$2[REDACTED]");
    let x = EMAIL.replace_all(&x, "[EMAIL]");
    let mut end = x.len().min(limit);
    while !x.is_char_boundary(end) {
        end -= 1;
    }
    x[..end].into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_credentials() {
        let s = redact(
            "Authorization: Bearer abc.secret password=xyz user@test.com",
            2048,
        );
        assert!(!s.contains("abc.secret"));
        assert!(!s.contains("xyz"));
        assert!(!s.contains("user@test.com"));
    }
    #[test]
    fn unicode_bounded() {
        assert!(redact("🛡🛡🛡", 3).len() <= 3);
    }
}
