// Standalone checks for selected examples in the research synthesis.
// Run: rustc --edition=2024 pattern_checks.rs -o /tmp/rust-pattern-checks && /tmp/rust-pattern-checks
use std::{borrow::Cow, num::ParseIntError};

#[derive(Debug, PartialEq)]
struct Port(u16);
impl TryFrom<u16> for Port {
    type Error = &'static str;
    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value == 0 { Err("port must be nonzero") } else { Ok(Self(value)) }
    }
}
fn parse_all(values: &[&str]) -> Result<Vec<u16>, ParseIntError> {
    values.iter().map(|s| s.parse()).collect()
}
fn normalize(value: &str) -> Cow<'_, str> {
    if value.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(value.to_ascii_lowercase())
    } else {
        Cow::Borrowed(value)
    }
}
fn main() {
    assert_eq!(Port::try_from(443), Ok(Port(443)));
    assert!(Port::try_from(0).is_err());
    assert!(u16::try_from(70_000_u32).is_err());
    assert_eq!(parse_all(&["1", "2"]), Ok(vec![1, 2]));
    assert!(parse_all(&["1", "invalid", "2"]).is_err());
    assert!(matches!(normalize("hello"), Cow::Borrowed("hello")));
    assert!(matches!(normalize("HELLO"), Cow::Owned(s) if s == "hello"));
    let text = "é";
    assert_eq!(text.get(..1), None);
    assert_eq!(text.get(..2), Some("é"));
    let mut slot = Some(String::from("payload"));
    assert_eq!(slot.take().as_deref(), Some("payload"));
    assert!(slot.is_none());
    println!("All selected pattern checks passed.");
}
