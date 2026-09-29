//! Timestamps as accounts.json keeps them, and as rows show them.

use chrono::{DateTime, Utc};

/// `now` as stored: ISO 8601 to the second, in UTC (`+00:00`, as the
/// Python app wrote them).
pub fn stamp(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

/// "3m ago", "never launched". For display only, never for ordering.
pub fn relative_time(iso: Option<&str>, now: DateTime<Utc>) -> String {
    let Some(then) = iso.and_then(parse) else {
        return "never launched".to_owned();
    };
    let secs = (now - then).num_seconds();
    if secs < 0 {
        return "just now".to_owned();
    }
    for (limit, div, unit) in
        [(60, 1, "s"), (3600, 60, "m"), (86_400, 3600, "h"), (2_592_000, 86_400, "d")]
    {
        if secs < limit {
            return format!("{}{unit} ago", secs / div);
        }
    }
    then.format("%Y-%m-%d").to_string()
}

/// A stored time; one without a zone is taken as UTC.
fn parse(iso: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso).map(|t| t.with_timezone(&Utc)).ok().or_else(|| {
        chrono::NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|t| t.and_utc())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn a_stamp_is_to_the_second_in_utc() {
        assert_eq!(
            stamp(Utc.with_ymd_and_hms(2026, 9, 22, 5, 0, 0).unwrap()),
            "2026-09-22T05:00:00+00:00"
        );
    }

    #[test]
    fn times_read_back_as_how_long_ago() {
        let now = at("2026-09-29T12:00:00+00:00");
        let cases = [
            (Some("2026-09-29T11:59:30+00:00"), "30s ago"),
            (Some("2026-09-29T11:57:00+00:00"), "3m ago"),
            (Some("2026-09-29T07:00:00+00:00"), "5h ago"),
            (Some("2026-09-27T12:00:00+00:00"), "2d ago"),
            (Some("2026-06-01T12:00:00+00:00"), "2026-06-01"),
            (Some("2026-09-29T12:05:00+00:00"), "just now"),
            (Some("2026-09-29T11:59:30Z"), "30s ago"),
            (Some("2026-09-29T11:59:30"), "30s ago"),
            (Some("yesterday"), "never launched"),
            (None, "never launched"),
        ];
        for (iso, want) in cases {
            assert_eq!(relative_time(iso, now), want, "{iso:?}");
        }
    }
}
