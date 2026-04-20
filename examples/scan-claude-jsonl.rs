// Throwaway spike: enumerate record types in a Claude Code JSONL.
// Deleted before Phase 2 ships — it exists only to validate the mental
// model in docs/adapter-notes-claude-code.md against real bytes on disk.
//
// Usage: cargo run --example scan-claude-jsonl -- <path-to-jsonl> [more...]
//
// Streams line-by-line so a 50 MB session doesn't blow the heap. Per file:
//   - total records, parse errors
//   - top-level `type` → count
//   - message roles → count
//   - content-block variants → count
//   - distinct sessionIds, cwds, gitBranches, versions
//   - presence counts for parentUuid, uuid, timestamp
//   - path-content probe: /mnt/ and drive-letter occurrences in content
//
// Uses only deps already in Cargo.toml (serde_json, anyhow).

use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

#[derive(Default)]
struct Stats {
    total: usize,
    parse_errors: usize,
    by_type: BTreeMap<String, usize>,
    by_role: BTreeMap<String, usize>,
    by_block: BTreeMap<String, usize>,
    session_ids: BTreeMap<String, usize>,
    cwds: BTreeMap<String, usize>,
    git_branches: BTreeMap<String, usize>,
    versions: BTreeMap<String, usize>,
    has_parent_uuid: usize,
    has_uuid: usize,
    has_timestamp: usize,
    wsl_path_hits: usize,     // "/mnt/" substring in content text
    windows_path_hits: usize, // drive-letter form (e.g. "C:\\")
    top_level_fields: BTreeMap<String, usize>,
    unknown_block_samples: Vec<String>,
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: scan-claude-jsonl <path> [more...]");
        std::process::exit(2);
    }
    for p in &args {
        match scan_one(Path::new(p)) {
            Ok(stats) => print_report(p, &stats),
            Err(e) => eprintln!("ERROR scanning {p}: {e:#}"),
        }
    }
    Ok(())
}

fn scan_one(path: &Path) -> Result<Stats> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let rdr = BufReader::new(f);
    let mut s = Stats::default();
    for line in rdr.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        s.total += 1;
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                s.parse_errors += 1;
                continue;
            }
        };

        if let Some(obj) = v.as_object() {
            for k in obj.keys() {
                *s.top_level_fields.entry(k.clone()).or_default() += 1;
            }
        }

        let ty = v
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("<no-type>")
            .to_string();
        *s.by_type.entry(ty.clone()).or_default() += 1;

        for (field, bucket) in [
            ("sessionId", &mut s.session_ids),
            ("cwd", &mut s.cwds),
            ("gitBranch", &mut s.git_branches),
            ("version", &mut s.versions),
        ] {
            if let Some(val) = v.get(field).and_then(|x| x.as_str()) {
                *bucket.entry(val.to_string()).or_default() += 1;
            }
        }
        if v.get("parentUuid").is_some() {
            s.has_parent_uuid += 1;
        }
        if v.get("uuid").is_some() {
            s.has_uuid += 1;
        }
        if v.get("timestamp").is_some() {
            s.has_timestamp += 1;
        }

        // The event-stream assumption: message-like records nest
        // {role, content[]} under "message". Verify by walking it.
        if let Some(msg) = v.get("message") {
            if let Some(role) = msg.get("role").and_then(|x| x.as_str()) {
                *s.by_role.entry(role.to_string()).or_default() += 1;
            }
            walk_content(msg.get("content"), &mut s);
        } else if matches!(ty.as_str(), "user" | "assistant" | "system") {
            // Some records may flatten role to top level — record that shape.
            if let Some(role) = v.get("role").and_then(|x| x.as_str()) {
                *s.by_role.entry(format!("flat:{role}")).or_default() += 1;
            }
            walk_content(v.get("content"), &mut s);
        }

        // Cheap path probe over the whole raw line — good enough to
        // feed the path-rewrite design, not a true substring-in-content
        // measurement. The notes doc must state this caveat.
        if line.contains("/mnt/") {
            s.wsl_path_hits += 1;
        }
        if contains_drive_letter(&line) {
            s.windows_path_hits += 1;
        }
    }
    Ok(s)
}

fn walk_content(content: Option<&Value>, s: &mut Stats) {
    let Some(c) = content else {
        *s.by_block.entry("<no-content>".into()).or_default() += 1;
        return;
    };
    match c {
        Value::String(_) => {
            *s.by_block.entry("string".into()).or_default() += 1;
        }
        Value::Array(arr) => {
            for b in arr {
                let bty = b
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("<untyped>")
                    .to_string();
                *s.by_block.entry(bty.clone()).or_default() += 1;
                if matches!(
                    bty.as_str(),
                    "text" | "tool_use" | "tool_result" | "thinking" | "image"
                ) {
                    // known variants — nothing to record
                } else if s.unknown_block_samples.len() < 3 {
                    s.unknown_block_samples.push(bty);
                }
            }
        }
        _ => {
            *s.by_block
                .entry("<non-string-non-array>".into())
                .or_default() += 1;
        }
    }
}

fn contains_drive_letter(haystack: &str) -> bool {
    // Crude: any `<letter>:\\` or `<letter>:/` sequence in the JSON-escaped
    // stream. Matches the 72 C:\ hits claim in the research doc.
    let bytes = haystack.as_bytes();
    for i in 0..bytes.len().saturating_sub(3) {
        let a = bytes[i];
        let b = bytes.get(i + 1);
        let c = bytes.get(i + 2);
        let d = bytes.get(i + 3);
        let is_letter = a.is_ascii_alphabetic();
        let colon = b == Some(&b':');
        // "X:\\" appears as X:\\ in JSON source (two backslashes)
        let esc_backslash = c == Some(&b'\\') && d == Some(&b'\\');
        let fwd_slash = c == Some(&b'/');
        if is_letter && colon && (esc_backslash || fwd_slash) {
            return true;
        }
    }
    false
}

fn print_report(path: &str, s: &Stats) {
    println!("=== {path} ===");
    println!("  total records        : {}", s.total);
    println!("  parse errors         : {}", s.parse_errors);
    println!("  has parentUuid       : {}", s.has_parent_uuid);
    println!("  has uuid             : {}", s.has_uuid);
    println!("  has timestamp        : {}", s.has_timestamp);
    println!("  WSL-path hit lines   : {}", s.wsl_path_hits);
    println!("  Windows-path hit lines: {}", s.windows_path_hits);
    println!("  top-level `type` counts:");
    for (k, v) in &s.by_type {
        println!("    {k:30} {v}");
    }
    println!("  roles seen:");
    for (k, v) in &s.by_role {
        println!("    {k:30} {v}");
    }
    println!("  content-block variants:");
    for (k, v) in &s.by_block {
        println!("    {k:30} {v}");
    }
    println!("  distinct sessionIds  : {}", s.session_ids.len());
    println!("  distinct cwds        : {}", s.cwds.len());
    if s.cwds.len() <= 4 {
        for k in s.cwds.keys() {
            println!("    cwd: {k}");
        }
    }
    println!("  distinct gitBranches : {}", s.git_branches.len());
    if s.git_branches.len() <= 4 {
        for k in s.git_branches.keys() {
            println!("    branch: {k}");
        }
    }
    println!("  distinct versions    : {}", s.versions.len());
    for k in s.versions.keys() {
        println!("    version: {k}");
    }
    if !s.unknown_block_samples.is_empty() {
        println!("  UNKNOWN block types sampled:");
        for k in &s.unknown_block_samples {
            println!("    {k}");
        }
    }
    println!("  top-level fields seen (field → record count):");
    for (k, v) in &s.top_level_fields {
        println!("    {k:30} {v}");
    }
    println!();
}
