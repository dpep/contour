//! Path rendering, in one place so no output surface can quietly forget.

/// A path as a person should read it: `$HOME` shown as `~`.
///
/// **Display only.** `--json` and `--ndjson` keep absolute paths, because a
/// machine consumer that has to expand `~` is one that will forget to.
pub fn pretty(path: &str) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.to_string();
    };
    let home = home.to_string_lossy();
    let home = home.strip_suffix('/').unwrap_or(&home);
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    // Boundary-aware: a home of `/Users/dan` must not claim `/Users/danger/x`,
    // which a bare `strip_prefix` would.
    match path.len() > home.len()
        && path.starts_with(home)
        && path.as_bytes().get(home.len()) == Some(&b'/')
    {
        true => format!("~{}", &path[home.len()..]),
        false => path.to_string(),
    }
}

/// A checkout-relative path made absolute, for a machine consumer.
///
/// JSON carries absolute paths because the consumer is not standing anywhere:
/// a `dupes` result saying `app/models/x.rb` is unresolvable without knowing
/// which of the machine's checkouts it came from.
pub fn absolute(root: &str, path: &str) -> String {
    format!("{}/{path}", root.trim_end_matches('/'))
}

/// The inverse, for human output: a person *is* standing in the checkout, and
/// a full path on every line is noise around the part they need.
pub fn within<'a>(root: &str, path: &'a str) -> &'a str {
    let root = root.trim_end_matches('/');
    path.strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_round_trips_between_the_two_audiences() {
        let abs = absolute("/repo", "app/models/x.rb");
        assert_eq!(abs, "/repo/app/models/x.rb");
        assert_eq!(within("/repo", &abs), "app/models/x.rb");
        // A trailing slash on the root is a real shape and must not double up.
        assert_eq!(absolute("/repo/", "a.rb"), "/repo/a.rb");
        assert_eq!(within("/repo/", "/repo/a.rb"), "a.rb");
        // A path from somewhere else is left alone rather than mangled.
        assert_eq!(within("/repo", "/elsewhere/a.rb"), "/elsewhere/a.rb");
    }

    #[test]
    fn shortens_home_and_nothing_else() {
        // SAFETY: single-threaded test, restored before it returns.
        let before = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", "/Users/dan") };

        assert_eq!(pretty("/Users/dan/code/app.rb"), "~/code/app.rb");
        assert_eq!(pretty("/Users/dan"), "~");
        assert_eq!(pretty("/Users/danger/x.rb"), "/Users/danger/x.rb");
        // A trailing slash on HOME is a real shape and must not double up.
        unsafe { std::env::set_var("HOME", "/Users/dan/") };
        assert_eq!(pretty("/Users/dan/code/app.rb"), "~/code/app.rb");

        match before {
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}
