use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    path: &'static str,
    needle: &'static str,
    text: String,
}

#[test]
fn local_access_execution_drift_is_explicit() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let findings = collect_findings(&manifest);
    let known = known_debt();

    let unexpected = findings.difference(&known).collect::<Vec<_>>();
    let resolved = known.difference(&findings).collect::<Vec<_>>();

    assert!(
        unexpected.is_empty() && resolved.is_empty(),
        "local access execution drift changed\n\nunexpected:\n{unexpected:#?}\n\nresolved:\n{resolved:#?}\n\ncurrent:\n{findings:#?}"
    );
}

fn collect_findings(manifest: &Path) -> BTreeSet<Finding> {
    let scans = [
        ("src/local_access.rs", &["NicheString"] as &[&'static str]),
        (
            "src/stencil/aarch64.rs",
            &["NicheString", "Option<String>", "option_string_layout"] as &[&'static str],
        ),
        (
            "src/stencil/compile.rs",
            &[
                "NicheString",
                "RustOptionStringBytes",
                "LocalBackend::RustFacet",
            ] as &[&'static str],
        ),
        (
            "src/stencil/mod.rs",
            &["RustOptionStringBytes"] as &[&'static str],
        ),
        (
            "src/stencil/runtime.rs",
            &["RustOptionStringBytes", "Option<String>"] as &[&'static str],
        ),
        (
            "src/stencil/types.rs",
            &["NicheString", "RustOptionStringBytes"] as &[&'static str],
        ),
    ];

    let mut findings = BTreeSet::new();
    for (relative, needles) in scans {
        let text = fs::read_to_string(manifest.join(relative)).unwrap();
        for line in text.lines() {
            let trimmed = line.trim();
            for needle in needles {
                if trimmed.contains(needle) {
                    findings.insert(Finding {
                        path: relative,
                        needle,
                        text: trimmed.to_owned(),
                    });
                }
            }
        }
    }
    findings
}

fn known_debt() -> BTreeSet<Finding> {
    BTreeSet::new()
}
