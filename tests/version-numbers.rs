#[test]
fn cargo_toml_version_matches_current_release() {
    version_sync::assert_contains_substring!("Cargo.toml", "version = \"{version}\"");
}

#[test]
fn cargo_lock_root_package_version_matches_current_release() {
    version_sync::assert_contains_substring!(
        "Cargo.lock",
        "name = \"caesim\"\nversion = \"{version}\""
    );
}

#[test]
fn debian_changelog_mentions_current_release() {
    version_sync::assert_contains_substring!("debian/changelog", "caesim ({version})");
}
