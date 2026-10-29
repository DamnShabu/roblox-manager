//! Release versions, compared: dotted numbers, optionally followed by a
//! pre-release (`0.3.0-rc.1`), which comes before the release itself.

use std::cmp::Ordering;

/// Whether `candidate` is a later version than `than`. A version either one
/// cannot be read as is never newer: an update is offered only when it is
/// certainly one.
pub fn is_newer(candidate: &str, than: &str) -> bool {
    match (Version::parse(candidate), Version::parse(than)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Version {
    numbers: Vec<u64>,
    pre: Option<String>,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        // Build metadata (`+...`) does not order versions.
        let text = text.split_once('+').map_or(text, |(v, _)| v);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre.to_owned())),
            None => (text, None),
        };
        let numbers = core.split('.').map(|n| n.parse().ok()).collect::<Option<Vec<u64>>>()?;
        Some(Version { numbers, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.numbers.len().max(other.numbers.len());
        let at = |v: &Version, i: usize| v.numbers.get(i).copied().unwrap_or(0);
        (0..len).map(|i| at(self, i).cmp(&at(other, i))).find(|o| o.is_ne()).unwrap_or_else(|| {
            match (&self.pre, &other.pre) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => pre_release(a, b),
            }
        })
    }
}

/// Two pre-releases, as semver orders them: identifier by identifier,
/// numbers as numbers and before words, and a shorter list first when the
/// rest are equal. As text `rc.10` came before `rc.9`.
fn pre_release(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.split('.'), b.split('.'));
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order.is_ne() {
                    return order;
                }
            }
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_compare_as_numbers() {
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("v1.0", "0.99.99"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2", "0.2.0"));
        assert!(!is_newer("0.1.9", "0.2.0"));
    }

    #[test]
    fn a_pre_release_comes_before_its_release() {
        assert!(is_newer("0.3.0", "0.3.0-rc.1"));
        assert!(!is_newer("0.3.0-rc.1", "0.3.0"));
        assert!(is_newer("0.3.0-rc.2", "0.3.0-rc.1"));
        assert!(is_newer("0.3.0-rc.1", "0.2.9"));
        assert!(is_newer("0.3.0-rc.10", "0.3.0-rc.9"), "numbers as numbers");
        assert!(is_newer("0.3.0-rc.1", "0.3.0-rc"), "a longer list after a shorter");
        assert!(is_newer("0.3.0-rc", "0.3.0-1"), "words after numbers");
        assert!(is_newer("0.3.0-beta", "0.3.0-alpha"));
    }

    #[test]
    fn what_is_no_version_is_never_newer() {
        assert!(!is_newer("nightly", "0.2.0"));
        assert!(!is_newer("0.3.0", "abc-other"));
        assert!(is_newer("0.3.0+build.5", "0.2.0"));
    }
}
