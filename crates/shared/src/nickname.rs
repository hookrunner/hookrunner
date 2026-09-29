//! Display names are validated identically on the client and authoritative server.
pub const MAX_NICKNAME_CHARS: usize = 20;

pub fn validate(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Enter a nickname.");
    }
    if value.chars().count() > MAX_NICKNAME_CHARS {
        return Err("Use at most 20 characters.");
    }
    if !value
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
    {
        return Err("Use letters, numbers, spaces, _ or -.");
    }
    Ok(value.to_owned())
}
