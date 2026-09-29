//! Labels for newly added accounts.

use crate::types::Label;

/// A label for a newly approved account: its Roblox username, numbered when
/// another account already carries it. A username that is no valid label
/// (one starting with `_`, say) is prefixed so it becomes one.
pub fn unique_label(base: &str, taken: &[&str]) -> Label {
    let base = base.trim();
    let base = [base.to_owned(), format!("acct {base}").trim().to_owned()]
        .iter()
        .find_map(|b| Label::parse(b).ok())
        .unwrap_or_else(fallback);
    // A valid label with " <n>" after it is still one, so the parse holds.
    (1..)
        .map(|n| if n == 1 { base.to_string() } else { format!("{base} {n}") })
        .find(|name| !taken.contains(&name.as_str()))
        .and_then(|name| Label::parse(&name).ok())
        .unwrap_or(base)
}

/// The label of last resort, for a username of nothing but control characters.
fn fallback() -> Label {
    Label::parse("acct").unwrap_or_else(|_| unreachable!("\"acct\" is a valid label"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_username_is_the_label() {
        assert_eq!(unique_label("givemegoodname24", &["alt"]).as_str(), "givemegoodname24");
    }

    #[test]
    fn a_taken_label_is_numbered() {
        assert_eq!(unique_label("alt", &["alt", "alt 2"]).as_str(), "alt 3");
    }

    #[test]
    fn a_username_that_is_no_label_still_gets_one() {
        assert_eq!(unique_label("_x", &[]).as_str(), "acct _x");
        assert_eq!(unique_label("  ", &[]).as_str(), "acct");
    }
}
