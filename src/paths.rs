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

#[cfg(test)]
mod tests {
    use super::*;

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
