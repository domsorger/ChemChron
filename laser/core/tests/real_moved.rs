// SPDX-License-Identifier: GPL-3.0-or-later
//! A saved session whose raw data was moved (another computer, a renamed
//! folder) reopens once the new folder is known. Uses the lab's Element
//! session; skips itself where it is not available.

use chemchron_laser_core::model::SessionSource;
use chemchron_laser_core::project::{reopen_session, session_from_paths};
use chemchron_laser_core::testdata::session_folder;

#[test]
fn moved_session_data_is_found_again() {
    let Some(dir) = session_folder("LAICPMS_monazite_session") else { return };
    let original = session_from_paths(std::slice::from_ref(&dir), None).unwrap();
    let text = serde_json::to_string(&original).unwrap();
    // Pretend it was saved on another computer.
    let old_root = dir.parent().unwrap().to_string_lossy().to_string();
    let moved_text = text.replace(&old_root.replace('\\', "\\\\"), "/mnt/moved/laser-data");
    assert_ne!(text, moved_text, "paths were rewritten");
    let moved: chemchron_laser_core::model::Session = serde_json::from_str(&moved_text).unwrap();
    // Without a folder to look in, it can't be reopened.
    assert!(reopen_session(moved.clone(), &[]).is_err());
    // With the folder the data is in now, everything is found and checked.
    let loaded = reopen_session(moved, &[dir.parent().unwrap().to_path_buf()]).unwrap();
    assert!(loaded.changed_files.iter().any(|n| n.contains("moved file")), "{:?}", loaded.changed_files);
    assert!(!loaded.changed_files.iter().any(|n| n.contains("can't be read") || n.contains("has changed")), "{:?}", loaded.changed_files);
    assert_eq!(loaded.session.inputs.time.len(), original.inputs.time.len());
    assert_eq!(loaded.session.files.len(), original.files.len());
    if let SessionSource::Files { paths, laser_log } = &loaded.session.source {
        assert!(paths.iter().all(|p| std::path::Path::new(p).exists()));
        assert!(laser_log.as_ref().is_none_or(|l| std::path::Path::new(l).exists()));
    }
}
