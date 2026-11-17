// SPDX-License-Identifier: GPL-3.0-or-later
//! Where the tests find real laser sessions (not part of the repository).
//!
//! `CHEMCHRON_TEST_DATA` names the folder, and then it must be readable: a
//! sandbox makes a blocked folder look missing, and a test that skips itself
//! would pass without checking anything. Without the variable, `testdata/laser`
//! and the folders next to the program folder are tried; where the data is
//! not available the tests skip themselves.

use std::path::{Path, PathBuf};

pub const VARIABLE: &str = "CHEMCHRON_TEST_DATA";

/// A session folder inside the test data, or None to skip on this computer.
pub fn session_folder(name: &str) -> Option<PathBuf> {
    resolve(std::env::var_os(VARIABLE).map(PathBuf::from), std::env::var_os("HOME").map(PathBuf::from), name)
}

fn resolve(required: Option<PathBuf>, _home: Option<PathBuf>, name: &str) -> Option<PathBuf> {
    if let Some(root) = required {
        let path = root.join(name);
        assert!(
            path.is_dir(),
            "{VARIABLE} is set, but {} can't be read. If a sandbox is on, allow reading that folder.",
            path.display()
        );
        return Some(path);
    }
    // testdata/laser/<name> (kept out of the repository)
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for p in [base.join("testdata/laser").join(name)] {
        if p.is_dir() {
            return Some(p);
        }
    }
    eprintln!("skipped: test data {name} is not on this computer (set {VARIABLE})");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_folder_that_cannot_be_read_fails_instead_of_skipping() {
        let missing = std::env::temp_dir().join("chemchron-laser-no-such-root");
        let outcome = std::panic::catch_unwind(|| resolve(Some(missing), None, "session"));
        assert!(outcome.is_err());
    }

    #[test]
    fn without_the_variable_a_missing_folder_skips() {
        let home = std::env::temp_dir().join("chemchron-laser-no-such-home");
        assert_eq!(resolve(None, Some(home), "session"), None);
        assert_eq!(resolve(None, None, "session"), None);
    }

    #[test]
    fn named_folder_that_exists_is_used() {
        let root = std::env::temp_dir();
        let name = format!("chemchron-laser-testdata-{}", std::process::id());
        std::fs::create_dir_all(root.join(&name)).unwrap();
        assert_eq!(resolve(Some(root.clone()), None, &name), Some(root.join(&name)));
        let _ = std::fs::remove_dir(root.join(&name));
    }
}
