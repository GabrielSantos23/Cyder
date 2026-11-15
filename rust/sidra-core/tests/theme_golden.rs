//! The Rust stylesheet must equal what `src/themeTemplate.ts` emits today, byte for byte.
//! Regenerate the fixtures from the TypeScript sources if the template changes on purpose.

use sidra_core::theme::{build_theme_css, BundledThemeName};
use std::path::Path;

#[test]
fn every_bundled_theme_matches_the_typescript_output() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/themes");
    for name in BundledThemeName::ALL {
        let path = dir.join(format!("{}.css", name.as_str()));
        // A Windows checkout may have turned the fixture's line endings into CRLF.
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .replace("\r\n", "\n");
        let actual = build_theme_css(name.theme());
        if actual != expected {
            let line = actual
                .lines()
                .zip(expected.lines())
                .position(|(a, e)| a != e);
            panic!(
                "{} differs from the TypeScript output at line {:?}",
                name.as_str(),
                line.map(|l| l + 1)
            );
        }
    }
}
