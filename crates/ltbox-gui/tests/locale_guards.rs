use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
enum RustTokenKind<'a> {
    Ident(&'a str),
    StringLiteral(&'a str),
    Punct(u8),
}

#[derive(Debug, Clone, Copy)]
struct RustToken<'a> {
    kind: RustTokenKind<'a>,
    line: usize,
}

#[derive(Debug, Default)]
struct TranslationSourceScan {
    rust_string_literals: BTreeSet<String>,
    called_keys: BTreeMap<String, BTreeSet<String>>,
}

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn workspace_dir() -> &'static Path {
    manifest_dir()
        .ancestors()
        .nth(2)
        .expect("ltbox-gui must live under <workspace>/crates")
}

fn load_locale(locale: &str) -> BTreeMap<String, String> {
    let path = manifest_dir().join("lang").join(format!("{locale}.json"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("{} must parse: {error}", path.display()))
}

fn production_source(source: &str) -> &str {
    // Every in-source test module in this workspace is an end-of-file
    // `#[cfg(test)]` module (plus one Windows-only test module). Cutting at
    // that structural boundary keeps fallback probes and test assertions
    // from masquerading as production translation references.
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed == "#[cfg(test)]" || trimmed.starts_with("#[cfg(all(test,") {
            return &source[..offset];
        }
        offset += line.len();
    }
    source
}

fn rust_tokens(source: &str) -> Vec<RustToken<'_>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;

    while index < bytes.len() {
        if bytes[index] == b'\n' {
            line += 1;
            index += 1;
            continue;
        }
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut depth = 1;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    if bytes[index] == b'\n' {
                        line += 1;
                    }
                    index += 1;
                }
            }
            continue;
        }

        // Locale keys use ordinary literals, but accepting raw strings
        // keeps the guard accurate if a call site changes spelling style.
        if bytes[index] == b'r' {
            let mut quote = index + 1;
            while quote < bytes.len() && bytes[quote] == b'#' {
                quote += 1;
            }
            if quote < bytes.len() && bytes[quote] == b'"' {
                let hashes = quote - index - 1;
                let literal_line = line;
                let body_start = quote + 1;
                index = body_start;
                loop {
                    assert!(
                        index < bytes.len(),
                        "unterminated raw string in Rust source"
                    );
                    if bytes[index] == b'"'
                        && index + 1 + hashes <= bytes.len()
                        && bytes[index + 1..index + 1 + hashes]
                            .iter()
                            .all(|byte| *byte == b'#')
                    {
                        tokens.push(RustToken {
                            kind: RustTokenKind::StringLiteral(&source[body_start..index]),
                            line: literal_line,
                        });
                        index += 1 + hashes;
                        break;
                    }
                    if bytes[index] == b'\n' {
                        line += 1;
                    }
                    index += 1;
                }
                continue;
            }
        }

        if bytes[index] == b'"' {
            let literal_line = line;
            let body_start = index + 1;
            index = body_start;
            while index < bytes.len() && bytes[index] != b'"' {
                if bytes[index] == b'\\' {
                    index += 1;
                    assert!(index < bytes.len(), "unterminated escape in Rust string");
                }
                if bytes[index] == b'\n' {
                    line += 1;
                }
                index += 1;
            }
            assert!(index < bytes.len(), "unterminated string in Rust source");
            tokens.push(RustToken {
                kind: RustTokenKind::StringLiteral(&source[body_start..index]),
                line: literal_line,
            });
            index += 1;
            continue;
        }

        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push(RustToken {
                kind: RustTokenKind::Ident(&source[start..index]),
                line,
            });
            continue;
        }

        if b"!()[]{},".contains(&bytes[index]) {
            tokens.push(RustToken {
                kind: RustTokenKind::Punct(bytes[index]),
                line,
            });
        }
        index += 1;
    }

    tokens
}

fn collect_called_literals<'a>(tokens: &'a [RustToken<'a>], open: usize) -> Vec<RustToken<'a>> {
    let mut literals = Vec::new();
    let mut nesting = 0;
    for token in &tokens[open + 1..] {
        match token.kind {
            RustTokenKind::Punct(b'(' | b'[' | b'{') => nesting += 1,
            RustTokenKind::Punct(b')') if nesting == 0 => break,
            RustTokenKind::Punct(b',') if nesting == 0 => break,
            RustTokenKind::Punct(b')' | b']' | b'}') => nesting -= 1,
            RustTokenKind::StringLiteral(_) => literals.push(*token),
            RustTokenKind::Ident(_) | RustTokenKind::Punct(_) => {}
        }
    }
    literals
}

fn collect_rust_sources(dir: &Path, files: &mut Vec<PathBuf>) {
    let mut entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
        .map(|entry| entry.expect("workspace source entry must be readable"))
        .collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn scan_production_translation_sources() -> TranslationSourceScan {
    let workspace = workspace_dir();
    let mut files = Vec::new();
    collect_rust_sources(&workspace.join("crates"), &mut files);

    let mut scan = TranslationSourceScan::default();
    for path in files {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let relative = path
            .strip_prefix(workspace)
            .expect("scanned source must be under the workspace")
            .display()
            .to_string()
            .replace('\\', "/");

        // Orphan detection intentionally follows every Rust literal in
        // the workspace, including static key arrays and assertions. The
        // undefined-call check below is narrower because test fallback
        // probes deliberately call translators with fake keys.
        for token in rust_tokens(&source) {
            if let RustTokenKind::StringLiteral(value) = token.kind {
                scan.rust_string_literals.insert(value.to_string());
            }
        }

        if path
            .strip_prefix(workspace)
            .expect("scanned source must be under the workspace")
            .components()
            .any(|component| component.as_os_str() == "tests")
        {
            continue;
        }
        let tokens = rust_tokens(production_source(&source));
        for (index, token) in tokens.iter().enumerate() {
            let RustTokenKind::Ident(name) = token.kind else {
                continue;
            };
            let (open_offset, is_translation_call) = match name {
                "tr" => (1, true),
                "tr_args" => (2, true),
                "t" if relative.starts_with("crates/ltbox-gui/src/") => (1, true),
                _ => (0, false),
            };
            if !is_translation_call {
                continue;
            }
            let Some(RustToken {
                kind: RustTokenKind::Punct(b'('),
                ..
            }) = tokens.get(index + open_offset)
            else {
                continue;
            };
            if name == "tr_args"
                && !matches!(
                    tokens.get(index + 1),
                    Some(RustToken {
                        kind: RustTokenKind::Punct(b'!'),
                        ..
                    })
                )
            {
                continue;
            }

            for literal in collect_called_literals(&tokens, index + open_offset) {
                let RustTokenKind::StringLiteral(key) = literal.kind else {
                    unreachable!();
                };
                scan.called_keys
                    .entry(key.to_string())
                    .or_default()
                    .insert(format!("{relative}:{}", literal.line));
            }
        }
    }
    scan
}

#[test]
fn locale_files_have_identical_key_sets() {
    let en = load_locale("en");
    let locales = [
        ("ko", load_locale("ko")),
        ("zh", load_locale("zh")),
        ("ru", load_locale("ru")),
        ("ja", load_locale("ja")),
    ];
    let en_keys = en.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let mut differences = Vec::new();

    for (locale, table) in &locales {
        let keys = table.keys().map(String::as_str).collect::<BTreeSet<_>>();
        let missing = en_keys.difference(&keys).copied().collect::<Vec<_>>();
        let extra = keys.difference(&en_keys).copied().collect::<Vec<_>>();
        if !missing.is_empty() {
            differences.push(format!(
                "crates/ltbox-gui/lang/{locale}.json is missing keys present in en.json: {}",
                missing.join(", ")
            ));
        }
        if !extra.is_empty() {
            differences.push(format!(
                "crates/ltbox-gui/lang/{locale}.json has keys absent from en.json: {}",
                extra.join(", ")
            ));
        }
    }

    assert!(
        differences.is_empty(),
        "locale key sets differ; add or remove the named keys so all five lang/*.json files match:\n- {}",
        differences.join("\n- ")
    );
}

#[test]
fn english_locale_keys_match_rust_sources() {
    let en = load_locale("en");
    let scan = scan_production_translation_sources();
    let orphans = en
        .keys()
        .filter(|key| !scan.rust_string_literals.contains(key.as_str()))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut failures = Vec::new();

    if !orphans.is_empty() {
        failures.push(format!(
            "crates/ltbox-gui/lang/en.json has keys with no string-literal reference in Rust under crates/**/*.rs: {}\nRemove each orphan from all five lang/*.json files, or restore its Rust call site or key table.",
            orphans.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let missing = scan
        .called_keys
        .iter()
        .filter(|(key, _)| !en.contains_key(key.as_str()))
        .map(|(key, locations)| {
            format!(
                "{key}: {}",
                locations.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        })
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        failures.push(format!(
            "production t(...), tr(...), or tr_args!(...) calls reference keys absent from crates/ltbox-gui/lang/en.json:\n- {}\nAdd each key to all five lang/*.json files, or correct the named call site.",
            missing.join("\n- ")
        ));
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
