use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
struct ForbiddenPattern {
    pattern: &'static str,
    reason: &'static str,
}

const FORBIDDEN_PATTERNS: &[ForbiddenPattern] = &[
    ForbiddenPattern {
        pattern: "encode_utf16(",
        reason: "use EasyPCWSTR or PWSTRBuffer instead of ad hoc UTF-16 conversion",
    },
    ForbiddenPattern {
        pattern: "encode_wide(",
        reason: "use EasyPCWSTR or PWSTRBuffer instead of ad hoc UTF-16 conversion",
    },
    ForbiddenPattern {
        pattern: "PCWSTR(",
        reason: "construct raw PCWSTR values only inside the string helper layer",
    },
    ForbiddenPattern {
        pattern: "PWSTR(",
        reason: "construct raw PWSTR values only inside the string helper layer",
    },
];

#[test]
fn win32_string_policy_disallows_manual_wide_string_patterns() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let src_dir = manifest_dir.join("src");
    let mut violations = Vec::new();

    collect_rust_files(&src_dir, &mut |path| {
        let relative_path = path
            .strip_prefix(&manifest_dir)
            .expect("source file should stay inside the repository root");
        if relative_path.starts_with(Path::new("src/win32_support/string")) {
            return;
        }

        let contents = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for forbidden in FORBIDDEN_PATTERNS {
            if contents.contains(forbidden.pattern) {
                violations.push(format!(
                    "{} contains `{}`: {}",
                    relative_path.display(),
                    forbidden.pattern,
                    forbidden.reason
                ));
            }
        }
    });

    if !violations.is_empty() {
        panic!(
            "manual Win32 string conversion escaped the helper layer:\n{}",
            violations.join("\n")
        );
    }
}

fn collect_rust_files(root: &Path, visit: &mut impl FnMut(&Path)) {
    let entries = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", root.display()));

    for entry in entries {
        let entry = entry.unwrap_or_else(|error| {
            panic!(
                "failed to enumerate entries under {}: {error}",
                root.display()
            )
        });
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, visit);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            visit(&path);
        }
    }
}
