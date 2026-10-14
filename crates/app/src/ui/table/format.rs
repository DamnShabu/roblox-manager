//! How the table words its numbers: how long a client has run, when an idle
//! account last did, memory, and shares of the CPU.

use std::time::Duration;

use chrono::Utc;
use rbxmgr_core::accounts::relative_time;

/// How long a client has run: "31 s", "48 m", "2 h 14 m", "3 d 4 h".
pub fn uptime(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} m", s / 60),
        3600..86_400 => format!("{} h {} m", s / 3600, s % 3600 / 60),
        _ => format!("{} d {} h", s / 86_400, s % 86_400 / 3600),
    }
}

/// When an idle account last launched: "3 d ago", or "Never".
pub fn last_ran(iso: Option<&str>) -> String {
    match relative_time(iso, Utc::now()).as_str() {
        "never launched" => "Never".to_owned(),
        "just now" => "Just now".to_owned(),
        r => spaced(r),
    }
}

/// "34m ago" as "34 m ago"; a date stays as it is.
fn spaced(r: &str) -> String {
    let digits = r.chars().take_while(char::is_ascii_digit).count();
    match r[digits..].chars().next() {
        Some(c) if digits > 0 && c.is_ascii_alphabetic() => {
            format!("{} {}", &r[..digits], &r[digits..])
        }
        _ => r.to_owned(),
    }
}

/// Memory as the table shows it: "598 MB", "1.4 GB".
pub fn bytes(n: u64) -> String {
    let mb = n as f64 / (1024.0 * 1024.0);
    if mb < 1000.0 { format!("{} MB", mb.round() as u64) } else { format!("{:.1} GB", mb / 1024.0) }
}

/// A share of the CPU: "8.1%".
pub fn percent(p: f32) -> String {
    format!("{p:.1}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_says_the_two_largest_units() {
        let at = |s| uptime(Duration::from_secs(s));
        assert_eq!(at(31), "31 s");
        assert_eq!(at(48 * 60 + 10), "48 m");
        assert_eq!(at(2 * 3600 + 14 * 60 + 8), "2 h 14 m");
        assert_eq!(at(3 * 86_400 + 4 * 3600 + 59), "3 d 4 h");
    }

    #[test]
    fn last_ran_reads_as_words() {
        assert_eq!(last_ran(None), "Never");
        let ago = |secs: i64| (Utc::now() - chrono::Duration::seconds(secs)).to_rfc3339();
        assert_eq!(last_ran(Some(&ago(3 * 86_400 + 30))), "3 d ago");
        assert_eq!(last_ran(Some(&ago(34 * 60 + 5))), "34 m ago");
        assert_eq!(last_ran(Some("2020-03-04T00:00:00Z")), "2020-03-04");
    }

    #[test]
    fn memory_is_megabytes_until_a_gigabyte() {
        assert_eq!(bytes(598 * 1024 * 1024), "598 MB");
        assert_eq!(bytes(1434 * 1024 * 1024), "1.4 GB");
        assert_eq!(bytes(0), "0 MB");
    }

    #[test]
    fn a_share_has_one_decimal() {
        assert_eq!(percent(8.123), "8.1%");
        assert_eq!(percent(38.0), "38.0%");
    }
}
