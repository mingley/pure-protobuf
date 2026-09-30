//! Documentation contract and linkage test suite.
//!
//! Validates that critical safety, compatibility, architecture caveats,
//! and link/anchor validity remain intact across documentation files,
//! replacing brittle multi-thousand-line prose equality tests with focused contracts.

#![allow(
    missing_docs,
    clippy::disallowed_methods,
    clippy::let_underscore_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    clippy::unimplemented,
    clippy::match_same_arms,
    clippy::cognitive_complexity,
    unreachable_pub,
    reason = "integration tests validate documentation fixtures and assert contracts"
)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static PROTO_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_workspace_file(rel: &str) -> String {
    let p = workspace_root().join(rel);
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("failed to read {}: {e}", p.display()))
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A contract specification for a crucial caveat that must be present in the documentation.
#[derive(Debug, Clone)]
pub struct CaveatRequirement {
    pub category: &'static str,
    pub title: &'static str,
    pub doc_path: &'static str,
    pub required_all: &'static [&'static str],
    pub description: &'static str,
}

pub const CRITICAL_CAVEATS: &[CaveatRequirement] = &[
    // ------------------------------------------------------------------------
    // 1. Unsupported / untested boundaries
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "unsupported_boundaries",
        title: "edition_2024_unsupported",
        doc_path: "docs/status.md",
        required_all: &["Edition 2024"],
        description: "Status documentation must explicitly document Edition 2024 as an unsupported/omitted boundary.",
    },
    CaveatRequirement {
        category: "unsupported_boundaries",
        title: "tonic_014_supported_older_unsupported",
        doc_path: "protobuf-tonic/README.md",
        required_all: &["Tonic 0.14", "unsupported"],
        description: "protobuf-tonic must record targeting Tonic 0.14+ with older versions unsupported.",
    },
    CaveatRequirement {
        category: "unsupported_boundaries",
        title: "feature_omissions_catalogued",
        doc_path: "docs/status.md",
        required_all: &["xDS", "channelz", "hedging"],
        description: "Status documentation must catalogue explicit feature omissions (xDS, channelz, hedging).",
    },
    // ------------------------------------------------------------------------
    // 2. Transparent retry boundaries
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "transparent_retries",
        title: "transparent_retry_at_most_once",
        doc_path: "docs/grpc.md",
        required_all: &["Transparent retry", "at most once"],
        description: "gRPC documentation must specify transparent retry as bounded to at-most-once.",
    },
    CaveatRequirement {
        category: "transparent_retries",
        title: "service_config_retry_boundary",
        doc_path: "docs/grpc.md",
        required_all: &["Channel::service_config", "Code::is_retryable"],
        description: "gRPC guide must document service-config retry attachment and the call-site fallback without a policy.",
    },
    CaveatRequirement {
        category: "transparent_retries",
        title: "retry_commitment_boundaries",
        doc_path: "docs/retry-contract.md",
        required_all: &["BodyStarted", "MUST NOT RETRY (Committed)"],
        description: "Retry contract must forbid retrying once request body data has started transmitting.",
    },
    CaveatRequirement {
        category: "transparent_retries",
        title: "from_io_no_transparent_retry",
        doc_path: "docs/grpc.md",
        required_all: &["from_io", "no transparent retry"],
        description: "Documentation must state that in-process from_io connections do not perform transparent retry.",
    },
    // ------------------------------------------------------------------------
    // 3. Security / TLS verifier and ALPN requirements
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "security_tls",
        title: "alpn_h2_required",
        doc_path: "docs/grpc.md",
        required_all: &["ALPN", "h2", "Certificate verification is not optional"],
        description: "gRPC TLS documentation must mandate ALPN h2 and state that certificate verification is not optional.",
    },
    CaveatRequirement {
        category: "security_tls",
        title: "caller_rustls_trust_boundary",
        doc_path: "docs/grpc.md",
        required_all: &[
            "Certificate verification is not optional for built-in constructors",
            "trusted application-owned",
            "Callers own custom verifier security",
            "an opaque custom verifier cannot be certified by this wrapper",
            "Disabling verification is unsupported",
        ],
        description: "Caller rustls configs require the application to own verifier security; the wrapper's built-in verification guarantee must remain scoped and disabling verification unsupported.",
    },
    CaveatRequirement {
        category: "security_tls",
        title: "tls_no_skip_verify",
        doc_path: "docs/status.md",
        required_all: &["skip-verify constructor", "ClientTls::webpki"],
        description: "Status documentation must document that there is no skip-verify constructor.",
    },
    CaveatRequirement {
        category: "security_tls",
        title: "mtls_requires_client_cert",
        doc_path: "docs/architecture.md",
        required_all: &["mTLS", "peer_identity"],
        description: "Architecture documentation must note mTLS client identity verification on peer_identity.",
    },
    // ------------------------------------------------------------------------
    // 4. HTTP/2 transport mapping caveats
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "http2_transport",
        title: "prior_knowledge_http2",
        doc_path: "docs/architecture.md",
        required_all: &["prior-knowledge HTTP/2"],
        description: "Architecture must document prior-knowledge HTTP/2 execution without HTTP/1.1 fallback.",
    },
    CaveatRequirement {
        category: "http2_transport",
        title: "rapid_reset_cve_mitigation",
        doc_path: "docs/grpc.md",
        required_all: &[
            "rapid reset",
            "ServerConfig::max_pending_accept_reset_streams",
        ],
        description: "gRPC documentation must document mitigation for HTTP/2 Rapid Reset (CVE-2023-44487).",
    },
    CaveatRequirement {
        category: "http2_transport",
        title: "continuation_flood_and_header_caps",
        doc_path: "docs/grpc.md",
        required_all: &["CONTINUATION", "max_header_list_size"],
        description: "gRPC documentation must document CONTINUATION flood defense and header list caps.",
    },
    // ------------------------------------------------------------------------
    // 5. License and crate naming notes
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "license_and_naming",
        title: "dual_license_mit_apache",
        doc_path: "README.md",
        required_all: &["Apache License, Version 2.0", "MIT License"],
        description: "Root README must document dual licensing under MIT and Apache-2.0.",
    },
    CaveatRequirement {
        category: "license_and_naming",
        title: "cargo_manifest_license",
        doc_path: "Cargo.toml",
        required_all: &["MIT OR Apache-2.0"],
        description: "Cargo.toml must declare MIT OR Apache-2.0 license.",
    },
    CaveatRequirement {
        category: "license_and_naming",
        title: "workspace_crate_and_repo_naming",
        doc_path: "docs/architecture.md",
        required_all: &[
            "`pbrs`",
            "`protobuf-tonic`",
            "`pbrs-grpc`",
            "mingley/pure-protobuf",
        ],
        description: "Architecture must name workspace crates and GitHub repository.",
    },
    CaveatRequirement {
        category: "license_and_naming",
        title: "v4_traits_not_prost_message",
        doc_path: "protobuf-tonic/README.md",
        required_all: &["Parse", "Serialize", "prost::Message"],
        description: "protobuf-tonic must document using Google protobuf v4 traits, not prost::Message.",
    },
    // ------------------------------------------------------------------------
    // 6. Published performance/support claim provenance (DX-07)
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "claim_provenance",
        title: "benchmark_provenance_framing",
        doc_path: "docs/benchmarks.md",
        required_all: &["dated source", "raw artifact", "benchmark-contract.md"],
        description: "Benchmarks must state that tables lack complete dated source and raw artifacts, and link the benchmark contract for stronger claims.",
    },
    CaveatRequirement {
        category: "claim_provenance",
        title: "status_support_qualification",
        doc_path: "docs/status.md",
        required_all: &[
            "not a production certification",
            "historical results",
            "## Verified",
        ],
        description: "Status must qualify support claims as historical results rather than production certification, under a Verified section.",
    },
    CaveatRequirement {
        category: "claim_provenance",
        title: "contract_forbids_universal_fastest",
        doc_path: "docs/benchmark-contract.md",
        required_all: &["fastest in the world", "forbidden"],
        description: "Benchmark contract must forbid universal fastest-in-the-world claims.",
    },
    // ------------------------------------------------------------------------
    // 7. Evidence links and rustdoc navigation (DX-07)
    // ------------------------------------------------------------------------
    CaveatRequirement {
        category: "evidence_links",
        title: "map_links_evidence_scripts",
        doc_path: "docs/documentation-map.md",
        required_all: &["scripts/conformance.sh", "scripts/grpc-interop.sh"],
        description: "Documentation map must link the runnable conformance and interop evidence scripts.",
    },
    CaveatRequirement {
        category: "rustdoc_navigation",
        title: "map_names_rustdoc_entry",
        doc_path: "docs/documentation-map.md",
        required_all: &["cargo doc", "docs.rs"],
        description: "Documentation map must route API reference readers through rustdoc and docs.rs.",
    },
];

/// Validates whether a given document content satisfies the requirement.
pub fn verify_caveat(requirement: &CaveatRequirement, content: &str) -> Result<(), String> {
    let normalized = normalize_whitespace(content);
    for &pattern in requirement.required_all {
        let normalized_pattern = normalize_whitespace(pattern);
        if !normalized.contains(&normalized_pattern) {
            return Err(format!(
                "Caveat contract '{}' failed in '{}': missing required pattern '{pattern}' ({})",
                requirement.title, requirement.doc_path, requirement.description
            ));
        }
    }
    Ok(())
}

/// Helper that verifies all critical caveats against repository files.
pub fn verify_all_critical_caveats() -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for req in CRITICAL_CAVEATS {
        let content = read_workspace_file(req.doc_path);
        if let Err(err) = verify_caveat(req, &content) {
            errors.push(err);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ============================================================================
// Markdown Link and Anchor Verification Engine
// ============================================================================

/// Converts a markdown heading into a GitHub-compatible anchor slug.
pub fn slugify_heading(heading: &str) -> String {
    let heading = heading.trim().to_ascii_lowercase();
    let mut cleaned = String::with_capacity(heading.len());
    for ch in heading.chars() {
        if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' {
            cleaned.push(ch);
        }
    }
    cleaned.split_whitespace().collect::<Vec<_>>().join("-")
}

/// Extracts all targetable anchors from markdown content:
/// - `# Heading` lines slugified
/// - `<a id="...">` or `<a name="...">` HTML anchors
pub fn extract_anchors(content: &str) -> HashSet<String> {
    let mut anchors = HashSet::new();
    let mut slug_counts: HashMap<String, usize> = HashMap::new();
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix('#') {
            let heading = rest.trim_start_matches('#').trim();
            // Strip inline markdown links if any: [label](url) -> label
            let mut clean_heading = String::new();
            let mut in_label = false;
            let mut in_url = false;
            let mut after_label = false;
            for ch in heading.chars() {
                match ch {
                    '[' => {
                        in_label = true;
                        after_label = false;
                    }
                    ']' if in_label => {
                        in_label = false;
                        after_label = true;
                    }
                    '(' if after_label => {
                        in_url = true;
                        after_label = false;
                    }
                    ')' if in_url => in_url = false,
                    _ if !in_url => {
                        clean_heading.push(ch);
                        after_label = false;
                    }
                    _ => {}
                }
            }
            let slug = slugify_heading(&clean_heading);
            if !slug.is_empty() {
                let count = slug_counts.entry(slug.clone()).or_insert(0);
                if *count == 0 {
                    anchors.insert(slug.clone());
                } else {
                    anchors.insert(format!("{slug}-{count}"));
                }
                *count += 1;
            }
        }

        // HTML anchor tags: <a id="..." or <a name="..."
        let lower = trimmed.to_ascii_lowercase();
        let mut search_idx = 0;
        while let Some(start) = lower[search_idx..].find("<a ") {
            let actual_start = search_idx + start;
            if let Some(end) = lower[actual_start..].find('>') {
                let tag = &trimmed[actual_start..actual_start + end];
                for key in ["id=\"", "id='", "name=\"", "name='"] {
                    if let Some(pos) = tag.find(key) {
                        let val_start = pos + key.len();
                        let quote = if key.contains('\'') { '\'' } else { '"' };
                        if let Some(val_len) = tag[val_start..].find(quote) {
                            anchors.insert(tag[val_start..val_start + val_len].to_string());
                        }
                    }
                }
                search_idx = actual_start + end + 1;
            } else {
                break;
            }
        }
    }
    anchors
}

/// Extracts all markdown links of the form `[label](target)` from text, ignoring code blocks.
pub fn extract_markdown_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut clean_content = String::with_capacity(content.len());
    let mut in_code_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if !in_code_block {
            clean_content.push_str(line);
            clean_content.push('\n');
        }
    }

    let bytes = clean_content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut end = start;
            let mut depth = 1;
            while end < bytes.len() && depth > 0 {
                if bytes[end] == b'(' {
                    depth += 1;
                } else if bytes[end] == b')' {
                    depth -= 1;
                }
                if depth == 0 {
                    break;
                }
                end += 1;
            }
            if depth == 0 {
                let raw_link = &clean_content[start..end];
                // Strip optional title: `path "title"`
                let link_target = raw_link.split_whitespace().next().unwrap_or("");
                if !link_target.is_empty() {
                    links.push(link_target.to_string());
                }
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    links
}

/// Discovers repository-owned `.md` files without following links outside the tree.
pub fn discover_markdown_files() -> Result<Vec<PathBuf>, Vec<String>> {
    discover_markdown_files_in(&workspace_root())
}

fn discover_markdown_files_in(root: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| vec![format!("failed to resolve documentation root: {error}")])?;
    let mut result = Vec::new();
    let mut errors = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let resolved = match dir.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                errors.push(format!(
                    "failed to resolve documentation directory: {error}"
                ));
                continue;
            }
        };
        if !resolved.starts_with(&canonical_root) {
            errors.push(format!(
                "documentation directory '{}' points outside repository",
                dir.strip_prefix(root).unwrap_or(&dir).display()
            ));
            continue;
        }
        if !visited.insert(resolved) {
            continue;
        }
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(format!(
                    "failed to read documentation directory '{}': {error}",
                    dir.strip_prefix(root).unwrap_or(&dir).display()
                ));
                continue;
            }
        };
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    errors.push(format!("failed to inspect documentation entry: {error}"));
                    continue;
                }
            };
            if path.is_dir() {
                let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                if file_name != "target"
                    && file_name != ".git"
                    && file_name != "third_party"
                    && file_name != "vendor"
                    && file_name != ".muse"
                    && file_name != "pure-protobuf.worktrees"
                {
                    stack.push(path);
                }
            } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                match path.canonicalize() {
                    Ok(resolved) if resolved.starts_with(&canonical_root) => result.push(path),
                    Ok(_) => errors.push(format!(
                        "Markdown file '{}' points outside repository",
                        path.strip_prefix(root).unwrap_or(&path).display()
                    )),
                    Err(error) => errors.push(format!(
                        "failed to resolve Markdown file '{}': {error}",
                        path.strip_prefix(root).unwrap_or(&path).display()
                    )),
                }
            }
        }
    }
    result.sort();
    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors)
    }
}

/// Validates all links and anchors in all repository markdown files.
pub fn validate_all_markdown_links() -> Result<(), Vec<String>> {
    let root = workspace_root();
    let files = discover_markdown_files()?;
    let mut anchor_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    let mut errors = Vec::new();

    // Pre-populate anchor cache for all markdown files
    for file in &files {
        if let Ok(content) = fs::read_to_string(file) {
            anchor_cache.insert(file.clone(), extract_anchors(&content));
        }
    }

    for file in &files {
        match fs::read_to_string(file) {
            Ok(content) => errors.extend(validate_links_in_document(
                file,
                &content,
                &root,
                &mut anchor_cache,
            )),
            Err(e) => errors.push(format!("failed to read {}: {e}", file.display())),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn cached_anchors<'a>(
    path: &Path,
    cache: &'a mut HashMap<PathBuf, HashSet<String>>,
) -> std::io::Result<&'a HashSet<String>> {
    use std::collections::hash_map::Entry;
    match cache.entry(path.to_path_buf()) {
        Entry::Occupied(entry) => Ok(entry.into_mut()),
        Entry::Vacant(entry) => {
            let content = fs::read_to_string(path)?;
            Ok(entry.insert(extract_anchors(&content)))
        }
    }
}

fn validate_links_in_document(
    file: &Path,
    content: &str,
    root: &Path,
    anchor_cache: &mut HashMap<PathBuf, HashSet<String>>,
) -> Vec<String> {
    let mut errors = Vec::new();
    let file_dir = file.parent().unwrap_or(root);
    let canonical_root = match root.canonicalize() {
        Ok(path) => path,
        Err(error) => return vec![format!("failed to resolve documentation root: {error}")],
    };

    for link in extract_markdown_links(content) {
        if link.starts_with("http://")
            || link.starts_with("https://")
            || link.starts_with("mailto:")
        {
            continue;
        }

        let (target_part, anchor_part) = match link.find('#') {
            Some(idx) => (&link[..idx], Some(&link[idx + 1..])),
            None => (link.as_str(), None),
        };

        let raw_path = if target_part.is_empty() {
            file.to_path_buf()
        } else {
            file_dir.join(target_part)
        };
        let target_file_path = match raw_path.canonicalize() {
            Ok(path) => path,
            Err(_) => {
                errors.push(format!(
                    "Broken link in '{}': target file '{}' does not exist",
                    file.strip_prefix(root).unwrap_or(file).display(),
                    target_part
                ));
                continue;
            }
        };
        if !target_file_path.starts_with(&canonical_root) {
            errors.push(format!(
                "Broken link in '{}': target '{}' points outside repository",
                file.strip_prefix(root).unwrap_or(file).display(),
                target_part
            ));
            continue;
        }

        if let Some(anchor) = anchor_part {
            if target_file_path.extension().and_then(|s| s.to_str()) == Some("md") {
                match cached_anchors(&target_file_path, anchor_cache) {
                    Ok(anchors) if !anchors.contains(anchor) => errors.push(format!(
                        "Broken anchor in '{}': anchor '#{}' not found in '{}'",
                        file.strip_prefix(root).unwrap_or(file).display(),
                        anchor,
                        target_file_path
                            .strip_prefix(root)
                            .unwrap_or(&target_file_path)
                            .display()
                    )),
                    Err(error) => errors.push(format!(
                        "Failed to read Markdown anchor target '{}': {error}",
                        target_part
                    )),
                    Ok(_) => {}
                }
            }
        }
    }

    errors
}

/// Validates that all guide and anchor references in `docs/documentation-map.md` resolve to valid files and anchors.
pub fn validate_documentation_map_references() -> Result<(), Vec<String>> {
    let root = workspace_root();
    let map_path = root.join("docs").join("documentation-map.md");
    let content = match fs::read_to_string(&map_path) {
        Ok(c) => c,
        Err(e) => {
            return Err(vec![format!(
                "failed to read docs/documentation-map.md: {e}"
            )]);
        }
    };
    validate_documentation_map_content(&root, &content)
}

fn validate_documentation_map_content(root: &Path, content: &str) -> Result<(), Vec<String>> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| vec![format!("failed to resolve documentation root: {error}")])?;
    let mut errors = Vec::new();
    let mut anchor_cache: HashMap<PathBuf, HashSet<String>> = HashMap::new();

    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let start = i + 1;
            if let Some(end_rel) = content[start..].find('`') {
                let token = content[start..start + end_rel].trim();
                if token.starts_with("docs/") {
                    let (path_part, anchor_part) = match token.find('#') {
                        Some(idx) => (&token[..idx], Some(&token[idx + 1..])),
                        None => (token, None),
                    };

                    let file_path = match root.join(path_part).canonicalize() {
                        Ok(path) => path,
                        Err(_) => {
                            errors.push(format!(
                                "Documentation map references non-existent path: '{path_part}' (from `{token}`)"
                            ));
                            i = start + end_rel + 1;
                            continue;
                        }
                    };
                    if !file_path.starts_with(&canonical_root) {
                        errors.push(format!(
                            "Documentation map reference '{path_part}' points outside repository (from `{token}`)"
                        ));
                    } else if let Some(anchor) = anchor_part {
                        if file_path.is_file()
                            && file_path.extension().and_then(|s| s.to_str()) == Some("md")
                        {
                            match cached_anchors(&file_path, &mut anchor_cache) {
                                Ok(anchors) if !anchors.contains(anchor) => errors.push(format!(
                                    "Documentation map references missing anchor '#{anchor}' in '{path_part}' (from `{token}`)"
                                )),
                                Err(error) => errors.push(format!(
                                    "Failed to read documentation-map anchor target '{path_part}': {error}"
                                )),
                                Ok(_) => {}
                            }
                        }
                    }
                }
                i = start + end_rel + 1;
                continue;
            }
        }
        i += 1;
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ============================================================================
// Code Snippet Syntax Verification Engine
// ============================================================================

/// A fenced code block extracted from markdown documentation.
#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub lang: String,
    pub code: String,
    pub line_number: usize,
}

/// Extracts all fenced code blocks (` ```lang ... ``` `) from markdown text.
pub fn extract_code_blocks(content: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut current_lang = None;
    let mut current_code = Vec::new();
    let mut start_line = 0;

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if let Some(lang) = current_lang.take() {
                blocks.push(CodeBlock {
                    lang,
                    code: current_code.join("\n"),
                    line_number: start_line,
                });
                current_code.clear();
            } else {
                let tag = rest
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                current_lang = Some(tag);
                start_line = line_idx + 1;
            }
        } else if current_lang.is_some() {
            current_code.push(line);
        }
    }
    blocks
}

/// Helper that invokes rustfmt to check Rust syntax.
fn run_rustfmt(input: &str) -> Result<(), String> {
    let mut child = Command::new("rustfmt")
        .arg("--edition")
        .arg("2024")
        .arg("--emit")
        .arg("stdout")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn rustfmt: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait on rustfmt: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).to_string();
        Err(err)
    }
}

/// Verifies whether a Rust code snippet is syntactically valid Rust.
///
/// Tries:
/// 1. As-is (for full items, functions, modules).
/// 2. Wrapped in an async function (for statements, expressions, and local items).
/// 3. Wrapped in a module (for file-level items).
pub fn check_rust_snippet_syntax(code: &str) -> Result<(), String> {
    if code.trim().is_empty() {
        return Ok(());
    }

    let err_as_is = match run_rustfmt(code) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };

    let wrapped_fn = format!(
        "async fn _snippet() -> Result<(), Box<dyn std::error::Error>> {{\n{code}\nOk(())\n}}"
    );
    if run_rustfmt(&wrapped_fn).is_ok() {
        return Ok(());
    }

    let wrapped_mod = format!("mod _snippet {{\n{code}\n}}");
    if run_rustfmt(&wrapped_mod).is_ok() {
        return Ok(());
    }

    Err(err_as_is)
}

/// Verifies whether a Protobuf code snippet is syntactically valid proto3.
///
/// Tries:
/// 1. As-is via `protoc`.
/// 2. Wrapped with `syntax = "proto3";` if syntax header was omitted.
pub fn check_protobuf_snippet_syntax(code: &str) -> Result<(), String> {
    if code.trim().is_empty() {
        return Ok(());
    }

    let temp_dir = std::env::temp_dir();
    let id = std::process::id();
    let count = PROTO_COUNTER.fetch_add(1, Ordering::SeqCst);
    let temp_file = temp_dir.join(format!("test_proto_{id}_{count}.proto"));

    let run_protoc = |content: &str| -> Result<(), String> {
        fs::write(&temp_file, content).map_err(|e| format!("failed to write temp proto: {e}"))?;
        let null_dev = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let output = Command::new("protoc")
            .arg("-o")
            .arg(null_dev)
            .arg(format!("-I{}", temp_dir.display()))
            .arg(&temp_file)
            .output();

        let _ = fs::remove_file(&temp_file);

        match output {
            Ok(out) => {
                if out.status.success() {
                    Ok(())
                } else {
                    let err = String::from_utf8_lossy(&out.stderr).to_string();
                    Err(err)
                }
            }
            Err(e) => Err(format!("failed to execute protoc: {e}")),
        }
    };

    if run_protoc(code).is_ok() {
        return Ok(());
    }

    let wrapped = format!("syntax = \"proto3\";\n{code}");
    if run_protoc(&wrapped).is_ok() {
        return Ok(());
    }

    run_protoc(code)
}

/// Discovers all documentation guides in `docs/guides/` and `docs/grpc.md`.
pub fn discover_guide_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = Vec::new();
    let guides_dir = root.join("docs").join("guides");
    if let Ok(entries) = fs::read_dir(&guides_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }
    let grpc_hub = root.join("docs").join("grpc.md");
    if grpc_hub.exists() {
        files.push(grpc_hub);
    }
    files.sort();
    files
}

/// Validates Rust and Protobuf syntax, including named executable recipe fences.
/// API type checking and runtime behavior live in the onboarding consumer suite.
pub fn validate_all_guide_code_blocks() -> Result<(), Vec<String>> {
    let root = workspace_root();
    let files = discover_guide_files();
    let mut errors = Vec::new();

    for file in &files {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("failed to read {}: {e}", file.display()));
                continue;
            }
        };

        let blocks = extract_code_blocks(&content);
        for block in blocks {
            let rel_path = file.strip_prefix(&root).unwrap_or(file).display();
            match block.lang.as_str() {
                "rust" => {
                    if let Err(err) = check_rust_snippet_syntax(&block.code) {
                        errors.push(format!(
                            "Invalid Rust syntax in '{rel_path}' at line {}:\n{}\nError:\n{err}",
                            block.line_number, block.code
                        ));
                    }
                }
                "protobuf" | "proto" => {
                    if let Err(err) = check_protobuf_snippet_syntax(&block.code) {
                        errors.push(format!(
                            "Invalid Protobuf syntax in '{rel_path}' at line {}:\n{}\nError:\n{err}",
                            block.line_number, block.code
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ============================================================================
// Focused Integration Tests
// ============================================================================

#[test]
fn test_unsupported_and_untested_boundaries_contract() {
    let status_content = read_workspace_file("docs/status.md");
    let tonic_readme = read_workspace_file("protobuf-tonic/README.md");

    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "unsupported_boundaries")
    {
        let content = if req.doc_path == "docs/status.md" {
            &status_content
        } else {
            &tonic_readme
        };
        verify_caveat(req, content).unwrap_or_else(|err| panic!("{err}"));
    }
}

#[test]
fn test_transparent_retry_boundaries_contract() {
    let grpc_content = read_workspace_file("docs/grpc.md");
    let retry_contract = read_workspace_file("docs/retry-contract.md");

    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "transparent_retries")
    {
        let content = if req.doc_path == "docs/retry-contract.md" {
            &retry_contract
        } else {
            &grpc_content
        };
        verify_caveat(req, content).unwrap_or_else(|err| panic!("{err}"));
    }
}

#[test]
fn test_security_tls_verifier_and_alpn_contract() {
    let grpc_content = read_workspace_file("docs/grpc.md");
    let status_content = read_workspace_file("docs/status.md");
    let arch_content = read_workspace_file("docs/architecture.md");

    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "security_tls")
    {
        let content = match req.doc_path {
            "docs/grpc.md" => &grpc_content,
            "docs/status.md" => &status_content,
            "docs/architecture.md" => &arch_content,
            other => panic!("unexpected doc path: {other}"),
        };
        verify_caveat(req, content).unwrap_or_else(|err| panic!("{err}"));
    }
}

#[test]
fn test_http2_transport_mapping_caveats_contract() {
    let grpc_content = read_workspace_file("docs/grpc.md");
    let arch_content = read_workspace_file("docs/architecture.md");

    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "http2_transport")
    {
        let content = if req.doc_path == "docs/architecture.md" {
            &arch_content
        } else {
            &grpc_content
        };
        verify_caveat(req, content).unwrap_or_else(|err| panic!("{err}"));
    }
}

#[test]
fn test_license_and_crate_naming_notes_contract() {
    let readme_content = read_workspace_file("README.md");
    let cargo_content = read_workspace_file("Cargo.toml");
    let arch_content = read_workspace_file("docs/architecture.md");
    let tonic_readme = read_workspace_file("protobuf-tonic/README.md");

    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "license_and_naming")
    {
        let content = match req.doc_path {
            "README.md" => &readme_content,
            "Cargo.toml" => &cargo_content,
            "docs/architecture.md" => &arch_content,
            "protobuf-tonic/README.md" => &tonic_readme,
            other => panic!("unexpected doc path: {other}"),
        };
        verify_caveat(req, content).unwrap_or_else(|err| panic!("{err}"));
    }
}

#[test]
fn test_all_critical_caveats_pass_on_repository_docs() {
    verify_all_critical_caveats().unwrap_or_else(|errs| {
        panic!(
            "Critical documentation caveat verification failed:\n{}",
            errs.join("\n")
        );
    });
}

#[test]
fn test_removing_or_altering_critical_caveat_fails() {
    // For each requirement, assert that the authentic document passes,
    // and that mutating/removing any required pattern triggers a failure.
    for req in CRITICAL_CAVEATS {
        let raw_content = read_workspace_file(req.doc_path);
        let authentic = normalize_whitespace(&raw_content);

        // 1. Authentic document must pass
        assert!(
            verify_caveat(req, &authentic).is_ok(),
            "Authentic document for '{}' must satisfy requirement",
            req.title
        );

        // 2. Mutating the required pattern must fail
        for &pattern in req.required_all {
            let norm_pattern = normalize_whitespace(pattern);
            assert!(
                authentic.contains(&norm_pattern),
                "Requirement '{}' pattern '{}' must exist in normalized document '{}'",
                req.title,
                norm_pattern,
                req.doc_path
            );
            let corrupted = authentic.replace(&norm_pattern, "__ALTERED_OR_REMOVED_CAVEAT__");
            let result = verify_caveat(req, &corrupted);
            assert!(
                result.is_err(),
                "Requirement '{}' must fail when pattern '{}' is removed or altered",
                req.title,
                pattern
            );
            let err_msg = result.unwrap_err();
            assert!(
                err_msg.contains(req.title),
                "Error message must mention requirement title"
            );
            assert!(
                err_msg.contains(pattern),
                "Error message must mention missing pattern"
            );
        }
    }
}

#[test]
fn test_documentation_links_and_anchors_are_valid() {
    validate_all_markdown_links().unwrap_or_else(|errs| {
        panic!(
            "Documentation link/anchor validation failed with {} errors:\n{}",
            errs.len(),
            errs.join("\n")
        );
    });
}

#[test]
fn test_link_checker_detects_broken_link_and_anchor() {
    // Unit tests for the slugify and anchor extraction logic
    assert_eq!(
        slugify_heading("## 1. Quickstart & Installation"),
        "1-quickstart-installation"
    );
    assert_eq!(
        slugify_heading("### Option A: Using `build.rs`"),
        "option-a-using-buildrs"
    );

    let doc = "# Main Heading\n\n[good](#main-heading)\n[bad](#nonexistent)\n";
    let anchors = extract_anchors(doc);
    assert!(anchors.contains("main-heading"));
    assert!(!anchors.contains("nonexistent"));

    let links = extract_markdown_links(doc);
    assert_eq!(links, vec!["#main-heading", "#nonexistent"]);

    let headings = "## Codegen and downstream compilation (CG-19 diagnostic)\n\
                    ## [gRPC codec](https://example.invalid/guide) (v2)\n";
    let anchors = extract_anchors(headings);
    assert!(anchors.contains("codegen-and-downstream-compilation-cg-19-diagnostic"));
    assert!(anchors.contains("grpc-codec-v2"));
}

#[test]
fn test_link_checker_rejects_missing_examples_and_stale_generated_references() {
    let root = workspace_root();
    let file = root.join("README.md");
    let mut anchor_cache = HashMap::new();
    let good = "[example](examples/greeter/src/lib.rs) [generated](src/generated/empty.rs) \
                [external](https://example.invalid/unreachable)\n";
    assert!(validate_links_in_document(&file, good, &root, &mut anchor_cache).is_empty());

    for (broken, target) in [
        (
            "[example](examples/greeter/src/removed.rs)",
            "examples/greeter/src/removed.rs",
        ),
        (
            "[generated](src/generated/removed.rs)",
            "src/generated/removed.rs",
        ),
        (
            "[heading](docs/grpc.md#missing-heading)",
            "#missing-heading",
        ),
    ] {
        let errors = validate_links_in_document(&file, broken, &root, &mut anchor_cache);
        assert_eq!(errors.len(), 1, "expected one broken reference in {broken}");
        assert!(errors[0].contains(target), "{}", errors[0]);
    }
}

#[test]
fn test_local_link_checker_rejects_repository_escape() {
    let root = workspace_root();
    let file = root.join("README.md");
    let errors = validate_links_in_document(
        &file,
        "[outside this repository](..)",
        &root,
        &mut HashMap::new(),
    );
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("outside repository"), "{errors:?}");

    #[cfg(unix)]
    {
        let link = root.join("target").join(format!(
            "documentation-link-escape-{}-{}.md",
            std::process::id(),
            PROTO_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(link.parent().unwrap()).expect("create ignored link fixture directory");
        std::os::unix::fs::symlink(root.parent().expect("repository has a parent"), &link)
            .expect("create outside-root link fixture");
        let content = format!(
            "[outside through symlink](target/{}#anchor)",
            link.file_name().unwrap().to_string_lossy()
        );
        let errors = validate_links_in_document(&file, &content, &root, &mut HashMap::new());
        fs::remove_file(&link).expect("remove link fixture");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("outside repository"), "{errors:?}");
    }
}

#[test]
fn test_documentation_map_rejects_repository_escape() {
    let errors = validate_documentation_map_content(&workspace_root(), "`docs/../..`")
        .expect_err("a documentation-map reference cannot point outside this repository");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("outside repository"), "{errors:?}");
}

#[test]
fn test_anchor_cache_never_treats_unreadable_targets_as_empty() {
    let mut cache = HashMap::new();
    let directory = workspace_root().join("docs");
    assert!(cached_anchors(&directory, &mut cache).is_err());
    assert!(!cache.contains_key(&directory));
}

#[cfg(unix)]
#[test]
fn test_markdown_discovery_rejects_out_of_repo_directory_links() {
    let scratch = workspace_root().join("target").join(format!(
        "documentation-discovery-{}-{}",
        std::process::id(),
        PROTO_COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let root = scratch.join("repo");
    let outside = scratch.join("outside");
    fs::create_dir_all(&root).expect("create synthetic repo");
    fs::create_dir_all(&outside).expect("create external fixture");
    fs::write(root.join("inside.md"), "# Inside\n").expect("write local page");
    fs::write(outside.join("outside.md"), "# Outside\n").expect("write external page");
    let link = root.join("external");
    std::os::unix::fs::symlink(&outside, &link).expect("create directory link fixture");

    let rejected = discover_markdown_files_in(&root);
    fs::remove_file(&link).expect("remove directory link fixture");
    let file_link = root.join("leak.md");
    std::os::unix::fs::symlink(outside.join("outside.md"), &file_link)
        .expect("create Markdown file link fixture");
    let rejected_file = discover_markdown_files_in(&root);
    fs::remove_file(&file_link).expect("remove Markdown file link fixture");
    let local_files = discover_markdown_files_in(&root).expect("local Markdown files are valid");
    fs::remove_dir_all(&scratch).expect("remove synthetic documentation fixture");

    let errors = rejected.expect_err("out-of-repository directories cannot be scanned");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("outside repository"), "{errors:?}");
    let errors = rejected_file.expect_err("out-of-repository Markdown files cannot be scanned");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("outside repository"), "{errors:?}");
    assert_eq!(local_files, vec![root.join("inside.md")]);
}

#[test]
fn test_documentation_map_references_and_anchors_are_valid() {
    validate_documentation_map_references().unwrap_or_else(|errs| {
        panic!(
            "Documentation map reference validation failed with {} errors:\n{}",
            errs.len(),
            errs.join("\n")
        );
    });
}

#[test]
fn test_guide_code_blocks_are_syntactically_valid() {
    validate_all_guide_code_blocks().unwrap_or_else(|errs| {
        panic!(
            "Guide code block syntax validation failed with {} errors:\n{}",
            errs.len(),
            errs.join("\n\n")
        );
    });
}

#[test]
fn test_rust_snippet_syntax_validator_detects_errors() {
    // Valid item
    assert!(check_rust_snippet_syntax("fn hello() { println!(\"world\"); }").is_ok());
    assert!(check_rust_snippet_syntax("fn r#gen() {}").is_ok());
    assert!(check_rust_snippet_syntax("fn gen() {}").is_err());
    // Valid statement
    assert!(check_rust_snippet_syntax("let x = 42;").is_ok());
    // Valid module
    assert!(check_rust_snippet_syntax("pub mod inner { pub struct Item; }").is_ok());

    // Invalid syntax (unclosed delimiter)
    let bad_code = "fn broken( { invalid";
    let res = check_rust_snippet_syntax(bad_code);
    assert!(res.is_err(), "Invalid Rust syntax should be rejected");
}

#[test]
fn test_named_recipe_fences_still_receive_negative_syntax_checks() {
    let blocks = extract_code_blocks(
        "Editorial context.\n```rust recipe=health\nfn broken( { invalid\n```\nMore prose.",
    );
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].lang, "rust");
    assert!(check_rust_snippet_syntax(&blocks[0].code).is_err());
}

#[test]
fn test_protobuf_snippet_syntax_validator_detects_errors() {
    // Valid proto with syntax header
    let valid_proto = "syntax = \"proto3\";\nmessage HelloRequest { string name = 1; }";
    assert!(check_protobuf_snippet_syntax(valid_proto).is_ok());

    // Valid proto snippet without syntax header (should be wrapped and validated)
    let valid_snippet = "message PartialMsg { int32 id = 1; }";
    assert!(check_protobuf_snippet_syntax(valid_snippet).is_ok());

    // Invalid proto syntax
    let bad_proto = "syntax = \"proto3\";\nmessage BadMsg { invalid field definition }";
    let res = check_protobuf_snippet_syntax(bad_proto);
    assert!(res.is_err(), "Invalid Protobuf syntax should be rejected");
}

// ============================================================================
// Published claim, version, and rustdoc navigation contracts (DX-07)
// ============================================================================

/// User-facing documents where performance and support claims appear.
/// Plan cards, decision logs, and closed evidence are historical records,
/// not published claims, and are intentionally excluded.
pub const CLAIM_DOCS: &[&str] = &[
    "README.md",
    "docs/benchmarks.md",
    "docs/status.md",
    "docs/grpc.md",
    "docs/architecture.md",
    "docs/documentation-map.md",
    "pbrs-grpc/README.md",
    "protobuf-tonic/README.md",
];

/// Release and roadmap records additionally cite published versions.
pub const VERSION_DOCS_EXTRA: &[&str] = &["docs/RELEASE.md", "docs/ROADMAP.md"];

/// Every published document: the fixed claim pages plus every guide.
pub fn published_doc_paths() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for rel in CLAIM_DOCS {
        let path = root.join(rel);
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    for path in discover_guide_files() {
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

/// Removes fenced code blocks and inline code spans so claim scanners only
/// see rendered prose. Newlines are preserved to keep line numbers stable.
pub fn strip_markdown_code(content: &str) -> String {
    let mut prose = String::with_capacity(content.len());
    let mut in_fence = false;
    for line in content.lines() {
        if line.trim().starts_with("```") {
            in_fence = !in_fence;
            prose.push('\n');
            continue;
        }
        if in_fence {
            prose.push('\n');
            continue;
        }
        for (idx, segment) in line.split('`').enumerate() {
            if idx % 2 == 0 {
                prose.push_str(segment);
            } else {
                prose.push(' ');
            }
        }
        prose.push('\n');
    }
    prose
}

/// Removes markdown link destinations (`](target)` -> `]`) so URL slugs
/// never read as prose claims.
fn strip_link_destinations(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ']' && i + 1 < chars.len() && chars[i + 1] == '(' {
            out.push(']');
            i += 2;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '(' {
                    depth += 1;
                } else if chars[i] == ')' {
                    depth -= 1;
                }
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Superlatives that must never appear as unqualified prose in published docs.
pub const UNQUALIFIED_SUPERLATIVES: &[&str] = &[
    "fastest",
    "best in class",
    "best-in-class",
    "world-class",
    "world class",
    "blazing",
    "orders of magnitude",
];

/// Lowercased phrases exempt from the superlative scan (program titles that
/// link to the plan, not performance claims).
pub const SUPERLATIVE_ALLOWLIST: &[&str] = &["world-class grpc program", "world-class program"];

/// Returns `(line_number, matched_pattern)` for every unqualified
/// superlative in prose (code already stripped via [`strip_markdown_code`]).
pub fn find_unqualified_superlatives(prose: &str) -> Vec<(usize, String)> {
    let mut hits = Vec::new();
    for (idx, line) in prose.lines().enumerate() {
        let without_targets = strip_link_destinations(line);
        let mut folded: String = without_targets
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        for allowed in SUPERLATIVE_ALLOWLIST {
            folded = folded.replace(allowed, " ");
        }
        for pattern in UNQUALIFIED_SUPERLATIVES {
            if folded.contains(pattern) {
                hits.push((idx + 1, (*pattern).to_string()));
            }
        }
    }
    hits
}

/// Validates that no published document makes an unqualified superlative claim.
pub fn validate_published_prose() -> Result<(), Vec<String>> {
    let root = workspace_root();
    let mut errors = Vec::new();
    for path in published_doc_paths() {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        match fs::read_to_string(&path) {
            Ok(content) => {
                let prose = strip_markdown_code(&content);
                for (line, pattern) in find_unqualified_superlatives(&prose) {
                    errors.push(format!(
                        "{rel}:{line}: unqualified superlative '{pattern}'; performance claims need dated source and result artifacts per docs/benchmark-contract.md"
                    ));
                }
            }
            Err(error) => errors.push(format!("failed to read {rel}: {error}")),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Reads the `[package] version` from manifest content.
pub fn read_package_version(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_package = trimmed == "[package]";
            continue;
        }
        if !in_package || !trimmed.starts_with("version") {
            continue;
        }
        let after_key = trimmed.strip_prefix("version")?.trim_start();
        if !after_key.starts_with('=') {
            continue;
        }
        let value = after_key.strip_prefix('=')?;
        let start = value.find('"')? + 1;
        let end = value[start..].find('"')? + start;
        return Some(value[start..end].to_string());
    }
    None
}

/// Reads the exact `tonic = ...` dependency requirement from manifest content.
/// `tonic-health` and `tonic-reflection` keys must not match.
pub fn read_tonic_requirement(manifest: &str) -> Option<String> {
    for line in manifest.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("tonic") {
            continue;
        }
        let after_key = trimmed.strip_prefix("tonic")?.trim_start();
        if !after_key.starts_with('=') {
            continue;
        }
        let value = after_key.strip_prefix('=')?;
        let quoted = if let Some(pos) = value.find("version") {
            let rest = value[pos + "version".len()..].trim_start();
            rest.strip_prefix('=')?
        } else {
            value
        };
        let start = quoted.find('"')? + 1;
        let end = quoted[start..].find('"')? + start;
        return Some(quoted[start..end].to_string());
    }
    None
}

/// `0.2.0` -> `0.2`.
pub fn major_minor(version: &str) -> String {
    let mut parts = version.split('.');
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) => format!("{major}.{minor}"),
        _ => version.to_string(),
    }
}

/// Whether the text around `pos` (byte offset, `window` bytes each side)
/// marks the mention as historical or unsupported rather than current.
pub fn has_historical_qualifier(text: &str, pos: usize, window: usize) -> bool {
    let mut start = pos.saturating_sub(window);
    while start < pos && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (pos + window).min(text.len());
    while end > pos && !text.is_char_boundary(end) {
        end -= 1;
    }
    let context = text[start..end].to_ascii_lowercase();
    context.contains("older") || context.contains("predate") || context.contains("unsupported")
}

/// Every `pbrs = "req"` (or `pbrs = { version = "req", ... }`) dependency
/// requirement cited in a document, as `(line_number, requirement)`.
/// `pbrs-grpc`, `pbrs_build`, and `pbrs::` paths never match.
pub fn find_pbrs_requirements(content: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(rel) = content[start..].find("pbrs") {
        let abs = start + rel;
        let after = &content[abs + "pbrs".len()..];
        let value = after
            .trim_start_matches(' ')
            .strip_prefix('=')
            .map(|rest| rest.trim_start_matches(' '));
        let req = value.and_then(|value| {
            if let Some(rest) = value.strip_prefix('"') {
                rest.find('"').map(|end| rest[..end].to_string())
            } else if value.starts_with('{') {
                let pos = value.find("version")?;
                let rest = value[pos + "version".len()..].trim_start();
                let quoted = rest.strip_prefix('=')?.trim_start();
                let inner = quoted.strip_prefix('"')?;
                inner.find('"').map(|end| inner[..end].to_string())
            } else {
                None
            }
        });
        if let Some(req) = req {
            let line = content[..abs].matches('\n').count() + 1;
            out.push((line, req));
        }
        start = abs + "pbrs".len();
    }
    out
}

/// Every `X.Y.Z-alpha.N` token in a document, as
/// `(line_number, byte_offset, token)`.
pub fn find_prerelease_versions(content: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(rel) = content[start..].find("-alpha.") {
        let abs = start + rel;
        let prefix = &content[..abs];
        let mut ver_start = abs;
        for (idx, ch) in prefix.char_indices().rev() {
            if ch.is_ascii_digit() || ch == '.' {
                ver_start = idx;
            } else {
                break;
            }
        }
        let rest = &content[abs + "-alpha.".len()..];
        let digits: usize = rest
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .map(char::len_utf8)
            .sum();
        if digits > 0 && ver_start < abs {
            let token = format!("{}-alpha.{}", &content[ver_start..abs], &rest[..digits]);
            let line = content[..abs].matches('\n').count() + 1;
            out.push((line, abs, token));
        }
        start = abs + "-alpha.".len();
    }
    out
}

/// Every `tonic M.m...` support mention in a document, as
/// `(line_number, byte_offset, major.minor)`. Matches `tonic`/`Tonic` only;
/// bare version numbers without the crate name are not attributed.
pub fn find_tonic_versions(content: &str) -> Vec<(usize, usize, String)> {
    let bytes = content.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + "tonic ".len() < bytes.len() {
        let is_tonic = bytes[i..].starts_with(b"tonic ") || bytes[i..].starts_with(b"Tonic ");
        if !is_tonic {
            i += 1;
            continue;
        }
        let mut j = i + "tonic ".len();
        let major_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let mut minor_end = None;
        if j > major_start && j < bytes.len() && bytes[j] == b'.' {
            j += 1;
            let minor_start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > minor_start {
                minor_end = Some(j);
            }
        }
        if let Some(end) = minor_end {
            let line = content[..i].matches('\n').count() + 1;
            out.push((line, i, content[major_start..end].to_string()));
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Checks `pbrs = "req"` citations in one document against the manifest.
pub fn check_pbrs_requirements(
    rel: &str,
    content: &str,
    expected_minor: &str,
    full_version: &str,
) -> Vec<String> {
    find_pbrs_requirements(content)
        .into_iter()
        .filter(|(_, req)| req != expected_minor && req != full_version)
        .map(|(line, req)| {
            format!(
                "{rel}:{line}: documented `pbrs = \"{req}\"` does not match Cargo.toml version {full_version}"
            )
        })
        .collect()
}

/// Checks adapter pre-release citations in one document: each must equal the
/// current version or be qualified as an older release nearby.
pub fn check_prerelease_mentions(rel: &str, content: &str, current: &str) -> Vec<String> {
    find_prerelease_versions(content)
        .into_iter()
        .filter(|(_, offset, token)| {
            token != current && !has_historical_qualifier(content, *offset, 150)
        })
        .map(|(line, _, token)| {
            format!(
                "{rel}:{line}: documented adapter version `{token}` does not match Cargo.toml version {current} and is not labeled as an older release"
            )
        })
        .collect()
}

/// Checks tonic support mentions in one document against the manifest.
pub fn check_tonic_mentions(rel: &str, content: &str, expected_minor: &str) -> Vec<String> {
    find_tonic_versions(content)
        .into_iter()
        .filter(|(_, offset, token)| {
            token != expected_minor && !has_historical_qualifier(content, *offset, 150)
        })
        .map(|(line, _, token)| {
            format!(
                "{rel}:{line}: documented tonic {token} does not match protobuf-tonic/Cargo.toml requirement {expected_minor} and is not labeled older/unsupported"
            )
        })
        .collect()
}

/// Documents whose version citations must match the Cargo manifests.
pub fn version_doc_paths() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut paths = published_doc_paths();
    for rel in VERSION_DOCS_EXTRA {
        let path = root.join(rel);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    paths
}

/// Validates documented `pbrs` requirements and adapter pre-releases against
/// the Cargo manifests, including adapter version parity.
pub fn validate_documented_versions() -> Result<(), Vec<String>> {
    let root = workspace_root();
    let mut errors = Vec::new();
    let pbrs_version = read_package_version(&read_workspace_file("Cargo.toml"))
        .unwrap_or_else(|| panic!("Cargo.toml has no [package] version"));
    let grpc_version = read_package_version(&read_workspace_file("pbrs-grpc/Cargo.toml"))
        .unwrap_or_else(|| panic!("pbrs-grpc/Cargo.toml has no [package] version"));
    let adapter_version = read_package_version(&read_workspace_file("protobuf-tonic/Cargo.toml"))
        .unwrap_or_else(|| panic!("protobuf-tonic/Cargo.toml has no [package] version"));
    if grpc_version != adapter_version {
        errors.push(format!(
            "adapter version drift: pbrs-grpc is {grpc_version} but protobuf-tonic is {adapter_version}"
        ));
    }
    let pbrs_minor = major_minor(&pbrs_version);
    let mut pbrs_mentions = 0;
    let mut current_adapter_mentions = 0;
    for path in version_doc_paths() {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        match fs::read_to_string(&path) {
            Ok(content) => {
                pbrs_mentions += find_pbrs_requirements(&content).len();
                current_adapter_mentions += find_prerelease_versions(&content)
                    .iter()
                    .filter(|(_, _, token)| token == &grpc_version)
                    .count();
                errors.extend(check_pbrs_requirements(
                    &rel,
                    &content,
                    &pbrs_minor,
                    &pbrs_version,
                ));
                errors.extend(check_prerelease_mentions(&rel, &content, &grpc_version));
            }
            Err(error) => errors.push(format!("failed to read {rel}: {error}")),
        }
    }
    if pbrs_mentions == 0 {
        errors.push("no `pbrs = \"...\"` requirement cited in published docs".to_string());
    }
    if current_adapter_mentions == 0 {
        errors.push(format!(
            "current adapter version {grpc_version} is not cited in published docs"
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validates documented tonic support boundaries against the adapter manifest.
pub fn validate_tonic_support_boundary() -> Result<(), Vec<String>> {
    let root = workspace_root();
    let mut errors = Vec::new();
    let tonic_req = read_tonic_requirement(&read_workspace_file("protobuf-tonic/Cargo.toml"))
        .unwrap_or_else(|| panic!("protobuf-tonic/Cargo.toml has no `tonic =` requirement"));
    let expected = major_minor(&tonic_req);
    let mut current_mentions = 0;
    for path in version_doc_paths() {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        match fs::read_to_string(&path) {
            Ok(content) => {
                current_mentions += find_tonic_versions(&content)
                    .iter()
                    .filter(|(_, _, token)| token == &expected)
                    .count();
                errors.extend(check_tonic_mentions(&rel, &content, &expected));
            }
            Err(error) => errors.push(format!("failed to read {rel}: {error}")),
        }
    }
    if current_mentions == 0 {
        errors.push(format!(
            "supported tonic {expected} is not cited in published docs"
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Published crate roots whose rustdoc landing sections back API navigation.
pub const RUSTDOC_CRATE_ROOTS: &[&str] = &[
    "src/lib.rs",
    "pbrs-grpc/src/lib.rs",
    "protobuf-tonic/src/lib.rs",
];

/// Whether a crate root opens with a crate-level `//!` rustdoc section.
pub fn crate_root_has_landing_docs(content: &str) -> bool {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| line.starts_with("//!"))
}

/// Validates that every published crate root has rustdoc landing docs.
pub fn validate_rustdoc_landing_docs() -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for rel in RUSTDOC_CRATE_ROOTS {
        if !crate_root_has_landing_docs(&read_workspace_file(rel)) {
            errors.push(format!(
                "{rel} has no crate-level `//!` rustdoc landing section for API navigation"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[test]
fn test_claim_provenance_contract() {
    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "claim_provenance")
    {
        let content = read_workspace_file(req.doc_path);
        verify_caveat(req, &content).unwrap_or_else(|err| panic!("{err}"));
    }
}

#[test]
fn test_evidence_links_and_rustdoc_navigation_contract() {
    for req in CRITICAL_CAVEATS
        .iter()
        .filter(|c| c.category == "evidence_links" || c.category == "rustdoc_navigation")
    {
        let content = read_workspace_file(req.doc_path);
        verify_caveat(req, &content).unwrap_or_else(|err| panic!("{err}"));
    }
    validate_rustdoc_landing_docs().unwrap_or_else(|errs| {
        panic!(
            "Rustdoc landing-doc validation failed:\n{}",
            errs.join("\n")
        );
    });
    assert!(crate_root_has_landing_docs(
        "//! Kernel docs.\nfn main() {}"
    ));
    assert!(crate_root_has_landing_docs("\n\n//! Indented start.\n"));
    assert!(!crate_root_has_landing_docs("fn main() {}"));
    assert!(!crate_root_has_landing_docs(""));
}

#[test]
fn test_published_prose_has_no_unqualified_superlatives() {
    validate_published_prose().unwrap_or_else(|errs| {
        panic!(
            "Published prose superlative validation failed with {} errors:\n{}",
            errs.len(),
            errs.join("\n")
        );
    });

    // Synthetic prose claims fail.
    for (prose, pattern) in [
        ("our stack is the fastest grpc implementation", "fastest"),
        (
            "the best-in-class codec for every workload",
            "best-in-class",
        ),
        ("a world-class transport layer", "world-class"),
        ("blazing performance on all hosts", "blazing"),
        ("faster by orders of magnitude", "orders of magnitude"),
    ] {
        let hits = find_unqualified_superlatives(&strip_markdown_code(prose));
        assert_eq!(hits.len(), 1, "expected one hit in {prose:?}");
        assert_eq!(hits[0].1, pattern);
    }

    // Code spans, fenced blocks, link targets, and the plan-program title pass.
    for prose in [
        "map to `ruzstd`'s implemented `Fastest` mode",
        "```rust\n// the fastest path\n```",
        "[World-class gRPC program](docs/plan/world-class/README.md)",
        "[world-class program](plan/world-class/README.md)",
        "ordinary performance prose without claims",
    ] {
        let hits = find_unqualified_superlatives(&strip_markdown_code(prose));
        assert!(hits.is_empty(), "unexpected hits in {prose:?}: {hits:?}");
    }
}

#[test]
fn test_documented_versions_match_cargo_manifests() {
    validate_documented_versions().unwrap_or_else(|errs| {
        panic!(
            "Documented version validation failed with {} errors:\n{}",
            errs.len(),
            errs.join("\n")
        );
    });

    assert_eq!(
        read_package_version("[package]\nname = \"pbrs\"\nversion = \"0.2.0\"\n"),
        Some("0.2.0".to_string())
    );
    assert_eq!(
        read_package_version("[package]\nversion=\"1.4.2\" # comment\n"),
        Some("1.4.2".to_string())
    );
    assert_eq!(
        read_package_version("[dependencies]\nversion = \"9\"\n"),
        None
    );
    assert_eq!(major_minor("0.2.0"), "0.2");

    // Stale citations fail; current and labeled-older citations pass.
    assert!(check_pbrs_requirements("doc.md", "pbrs = \"0.2\"", "0.2", "0.2.0").is_empty());
    assert_eq!(
        check_pbrs_requirements("doc.md", "pbrs = \"0.1\"", "0.2", "0.2.0").len(),
        1
    );
    assert!(
        check_pbrs_requirements("doc.md", "pbrs-grpc = \"0.1.0-alpha.2\"", "0.2", "0.2.0")
            .is_empty()
    );
    assert!(
        check_prerelease_mentions("doc.md", "uses `0.1.0-alpha.2` today", "0.1.0-alpha.2")
            .is_empty()
    );
    assert_eq!(
        check_prerelease_mentions("doc.md", "uses `0.1.0-alpha.9` today", "0.1.0-alpha.2").len(),
        1
    );
    assert!(
        check_prerelease_mentions(
            "doc.md",
            "the older `0.1.0-alpha.1` archives still need it",
            "0.1.0-alpha.2",
        )
        .is_empty()
    );
}

#[test]
fn test_tonic_support_boundary_matches_manifest() {
    validate_tonic_support_boundary().unwrap_or_else(|errs| {
        panic!(
            "Tonic support-boundary validation failed with {} errors:\n{}",
            errs.len(),
            errs.join("\n")
        );
    });

    assert_eq!(
        read_tonic_requirement("tonic = { version = \"0.14\", default-features = false }\n"),
        Some("0.14".to_string())
    );
    assert_eq!(
        read_tonic_requirement("tonic = \"0.14\"\n"),
        Some("0.14".to_string())
    );
    assert_eq!(
        read_tonic_requirement("tonic-health = \"0.14\"\ntonic-reflection = \"0.14\"\n"),
        None
    );

    assert!(check_tonic_mentions("doc.md", "needs tonic 0.14+ today", "0.14").is_empty());
    assert!(check_tonic_mentions("doc.md", "ports of Tonic 0.14.6 examples", "0.14").is_empty());
    assert!(
        check_tonic_mentions("doc.md", "tonic 0.12 and 0.13 are unsupported", "0.14").is_empty()
    );
    assert_eq!(
        check_tonic_mentions("doc.md", "needs tonic 0.13 today", "0.14").len(),
        1
    );
}
