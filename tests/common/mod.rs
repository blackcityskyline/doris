//! Fixtures shared by the integration tests.
//!
//! `UiApp::new` takes a TorrServer URL and a theme name, and neither
//! means anything to a test that only wants a state to poke at. Eleven
//! files were each carrying their own copy of that one-liner, which is
//! the shape a change takes when the thing being changed is the
//! constructor: every copy has to be found by reading, and a missed one
//! is a test that panics on a signature that no longer exists.
//!
//! Add a variant here rather than a copy in a test file.

use doris::ui::view::App as UiApp;

/// A URL nothing in a unit test will ever contact.
pub const TEST_URL: &str = "http://127.0.0.1:8090";

/// The default theme, and the state every test starts from.
pub fn make_app() -> UiApp {
    UiApp::new(TEST_URL.into(), None)
}
