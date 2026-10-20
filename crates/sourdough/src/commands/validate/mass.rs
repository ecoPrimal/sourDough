// SPDX-License-Identifier: AGPL-3.0-or-later
//! Stoichiometric mass validator — braid + provenance trio page sort.
//!
//! Composes from existing ecosystem pieces:
//!   - `ContentHash` (Blake3, sourDough-core) → page identity, short hash for topo sort
//!   - Shannon entropy (H) → information density (sweetGrass braid semantic coherence)
//!   - Connectivity + grounding → braid strength model (scan-sweep-sort vocabulary)
//!   - Weight 1-200 → Zola frontmatter for publish ordering
//!
//! Mass formula:
//!   `mass = √(words) × H × (1 + links/50) × (1 + citations/10) × richness`
//!
//! This is the SORT stage applied to static content — pages ARE entities.
//! Each page gets a Blake3 ContentHash (identity) and Shannon entropy (mass).
//! The short hash (first 16 hex chars) is the topo-sort handle.
//!
//! Authorship: Kevin Mok / Tamison / ecoPrimal / attsi, ORCID 0009-0004-2141-0321

use anyhow::{Context, Result};
use sourdough_core::ContentHash;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Evidence citation patterns (case-insensitive).
const CITATION_PATTERNS: &[&str] = &[
    "foia", "lara", "mdoc", "ichat", "pacer", "cfrs", "mde", "docket",
];

/// Frontmatter format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrontmatterFormat {
    Toml,
    Yaml,
}

/// Information mass for a single page — a braid-compatible provenance record.
///
/// The `content_hash` field is Blake3 from sourDough-core (`ContentHash`).
/// The `short_hash` is the first 16 hex chars — the topo-sort handle that
/// loamSpine uses for DataAnchor identity.
#[derive(Debug, Clone)]
struct PageMass {
    path: PathBuf,
    #[allow(dead_code)] // full hash available for anchoring via loamSpine DataAnchor
    content_hash: ContentHash,
    short_hash: String,
    words: usize,
    links: usize,
    citations: usize,
    entropy: f64,
    mass: f64,
    format: FrontmatterFormat,
}

fn shannon_entropy(words: &[String]) -> f64 {
    if words.is_empty() {
        return 0.0;
    }
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for w in words {
        *freq.entry(w.as_str()).or_insert(0) += 1;
    }
    let total = words.len() as f64;
    freq.values()
        .map(|&count| {
            let p = count as f64 / total;
            -p * p.log2()
        })
        .sum()
}

fn extract_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_lowercase())
        .collect()
}

fn count_links(text: &str) -> usize {
    let mut count = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(close) = text[i..].find("](") {
                if text[i + close + 2..].contains(')') {
                    count += 1;
                    i += close + 2;
                    continue;
                }
            }
        }
        i += 1;
    }
    count
}

fn count_citations(text: &str) -> usize {
    let lower = text.to_lowercase();
    CITATION_PATTERNS
        .iter()
        .map(|pat| lower.matches(pat).count())
        .sum::<usize>()
        + lower.matches("case no").count()
}

fn detect_format(content: &str) -> Option<FrontmatterFormat> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("+++") {
        Some(FrontmatterFormat::Toml)
    } else if trimmed.starts_with("---") {
        Some(FrontmatterFormat::Yaml)
    } else {
        None
    }
}

fn strip_frontmatter(content: &str, format: FrontmatterFormat) -> &str {
    let sep = match format {
        FrontmatterFormat::Toml => "+++",
        FrontmatterFormat::Yaml => "---",
    };
    let parts: Vec<&str> = content.splitn(3, sep).collect();
    if parts.len() >= 3 { parts[2] } else { content }
}

/// Blake3 hash of page content → `ContentHash` (sourDough-core type).
fn hash_page(content: &[u8]) -> ContentHash {
    let digest = blake3::hash(content);
    ContentHash::new(*digest.as_bytes())
}

fn compute_mass(path: &Path) -> Result<PageMass> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read {}", path.display()))?;

    let format = detect_format(&content).unwrap_or(FrontmatterFormat::Toml);
    let body = strip_frontmatter(&content, format);

    let words = extract_words(body);
    let word_count = words.len();
    let unique: std::collections::HashSet<&str> = words.iter().map(|s| s.as_str()).collect();
    let unique_count = unique.len();

    let links = count_links(body);
    let citations = count_citations(body);
    let entropy = shannon_entropy(&words);
    let richness = unique_count as f64 / word_count.max(1) as f64;

    let bulk = (word_count as f64).sqrt();
    let connectivity = 1.0 + links as f64 / 50.0;
    let grounding = 1.0 + citations as f64 / 10.0;
    let mass = bulk * entropy * connectivity * grounding * richness;

    let content_hash = hash_page(content.as_bytes());
    let short_hash = content_hash.to_hex()[..16].to_string();

    Ok(PageMass {
        path: path.to_path_buf(),
        content_hash,
        short_hash,
        words: word_count,
        links,
        citations,
        entropy,
        mass,
        format,
    })
}

fn set_weight(path: &Path, format: FrontmatterFormat, weight: u32) -> Result<()> {
    let content = std::fs::read_to_string(path)?;

    let (sep, weight_line, weight_prefix, date_prefix) = match format {
        FrontmatterFormat::Toml => ("+++", format!("weight = {weight}"), "weight", "date"),
        FrontmatterFormat::Yaml => ("---", format!("weight: {weight}"), "weight:", "date:"),
    };

    let parts: Vec<&str> = content.splitn(3, sep).collect();
    if parts.len() < 3 {
        anyhow::bail!("Cannot parse frontmatter in {}", path.display());
    }

    let lines: Vec<&str> = parts[1].lines().collect();
    let mut new_lines: Vec<String> = Vec::with_capacity(lines.len() + 1);
    let mut found = false;

    for line in &lines {
        if line.trim_start().starts_with(weight_prefix) {
            new_lines.push(weight_line.clone());
            found = true;
        } else {
            new_lines.push((*line).to_string());
        }
    }

    if !found {
        let mut inserted = false;
        let mut result: Vec<String> = Vec::with_capacity(new_lines.len() + 1);
        for line in &new_lines {
            result.push(line.clone());
            if !inserted && line.trim_start().starts_with(date_prefix) {
                result.push(weight_line.clone());
                inserted = true;
            }
        }
        if !inserted {
            result.push(weight_line);
        }
        new_lines = result;
    }

    let new_front = new_lines.join("\n");
    let new_content = format!("{sep}{new_front}{sep}{}", parts[2]);
    std::fs::write(path, new_content)?;
    Ok(())
}

/// Sections that use `sort_by = "weight"` by convention.
const WEIGHT_SORTED_SECTIONS: &[&str] = &[
    "analysis", "books", "evidence", "network", "path", "signal",
    "actors", "braid", "investigate", "synthesis", "coverage",
];

fn collect_pages(content_dir: &Path) -> Vec<PathBuf> {
    let mut pages = Vec::new();
    for section in WEIGHT_SORTED_SECTIONS {
        let section_dir = content_dir.join(section);
        if !section_dir.exists() {
            continue;
        }
        collect_md_recursive(&section_dir, &mut pages);
    }
    pages
}

fn collect_md_recursive(dir: &Path, pages: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md_recursive(&path, pages);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name != "_index.md" {
                pages.push(path);
            }
        }
    }
}

pub(crate) fn validate(content_dir: &Path, json: bool, apply: bool) -> Result<()> {
    let pages_paths = collect_pages(content_dir);
    if pages_paths.is_empty() {
        if json {
            println!("{{\"pages\": 0, \"status\": \"empty\"}}");
        } else {
            crate::warning("No pages found in weight-sorted sections");
        }
        return Ok(());
    }

    let mut pages: Vec<PageMass> = Vec::with_capacity(pages_paths.len());
    for path in &pages_paths {
        match compute_mass(path) {
            Ok(pm) => pages.push(pm),
            Err(e) => {
                crate::warning(&format!("Skipping {}: {e}", path.display()));
            }
        }
    }

    pages.sort_by(|a, b| b.mass.partial_cmp(&a.mass).unwrap_or(std::cmp::Ordering::Equal));

    let max_mass = pages.first().map_or(1.0, |p| p.mass.max(1.0));
    let total_mass: f64 = pages.iter().map(|p| p.mass).sum();
    let mean_entropy = pages.iter().map(|p| p.entropy).sum::<f64>() / pages.len() as f64;

    if json {
        print_json(&pages, max_mass, total_mass, mean_entropy, apply);
    } else {
        print_report(&pages, content_dir, max_mass, total_mass, mean_entropy, apply);
    }

    if apply {
        let mut applied = 0u32;
        for page in &pages {
            let weight = (200.0 * page.mass / max_mass).round().max(1.0) as u32;
            if set_weight(&page.path, page.format, weight).is_ok() {
                applied += 1;
            }
        }
        if !json {
            crate::success(&format!("Applied stoichiometric weights to {applied} pages"));
        }
    }

    Ok(())
}

fn print_report(
    pages: &[PageMass],
    content_dir: &Path,
    max_mass: f64,
    total_mass: f64,
    mean_entropy: f64,
    apply: bool,
) {
    crate::info(&format!(
        "Stoichiometric mass audit: {} pages in {}",
        pages.len(),
        content_dir.display()
    ));
    println!();
    println!(
        "  Formula: √(words) × H × (1+links/50) × (1+cites/10) × richness"
    );
    println!(
        "  Composed: ContentHash (Blake3) → short hash topo sort → braid strength"
    );
    println!(
        "  Mass range: {:.1} — {:.1}  |  Total: {:.0}  |  Mean H: {:.3} bits",
        pages.last().map_or(0.0, |p| p.mass),
        max_mass,
        total_mass,
        mean_entropy,
    );
    println!();

    println!("{:>4} {:>8} {:>6} {:>5} {:>4} {:>3}  {:>16}  Path",
        "Wt", "Mass", "H", "Words", "Lnk", "Cit", "Blake3");
    for p in pages.iter().take(10) {
        let w = (200.0 * p.mass / max_mass).round().max(1.0) as u32;
        let short = p.path.strip_prefix(content_dir).unwrap_or(&p.path);
        println!(
            "{:>4} {:>8.1} {:>6.3} {:>5} {:>4} {:>3}  {}  {}",
            w, p.mass, p.entropy, p.words, p.links, p.citations,
            p.short_hash, short.display()
        );
    }
    if pages.len() > 10 {
        println!("  ... ({} more)", pages.len() - 10);
    }
    println!();

    if apply {
        crate::info("Writing weights...");
    } else {
        crate::info("Dry run — use --apply to write weights into frontmatter");
    }
}

fn print_json(pages: &[PageMass], max_mass: f64, total_mass: f64, mean_entropy: f64, applied: bool) {
    println!("{{");
    println!("  \"pages\": {},", pages.len());
    println!("  \"max_mass\": {:.1},", max_mass);
    println!("  \"total_mass\": {:.0},", total_mass);
    println!("  \"mean_entropy_bits\": {:.3},", mean_entropy);
    println!("  \"applied\": {},", applied);
    println!("  \"formula\": \"sqrt(words) * H * (1+links/50) * (1+cites/10) * richness\",");
    println!("  \"composed_from\": [\"ContentHash (Blake3)\", \"Shannon H\", \"braid strength\"],");
    println!("  \"top\": [");
    for (i, p) in pages.iter().take(5).enumerate() {
        let w = (200.0 * p.mass / max_mass).round().max(1.0) as u32;
        let comma = if i < 4.min(pages.len() - 1) { "," } else { "" };
        println!(
            "    {{\"weight\": {}, \"mass\": {:.1}, \"entropy\": {:.3}, \"words\": {}, \"blake3\": \"{}\", \"path\": \"{}\"}}{}",
            w, p.mass, p.entropy, p.words,
            p.short_hash, p.path.display(),
            comma
        );
    }
    println!("  ]");
    println!("}}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shannon_entropy_uniform() {
        let words: Vec<String> = vec!["a", "b", "c", "d"]
            .into_iter()
            .map(String::from)
            .collect();
        let h = shannon_entropy(&words);
        assert!((h - 2.0).abs() < 0.001, "4 equiprobable symbols → 2 bits, got {h}");
    }

    #[test]
    fn shannon_entropy_constant() {
        let words: Vec<String> = vec!["a", "a", "a", "a"]
            .into_iter()
            .map(String::from)
            .collect();
        let h = shannon_entropy(&words);
        assert!(h.abs() < 0.001, "constant → 0 bits, got {h}");
    }

    #[test]
    fn shannon_entropy_empty() {
        assert_eq!(shannon_entropy(&[]), 0.0);
    }

    #[test]
    fn extract_words_filters_short() {
        let words = extract_words("I am a big dog");
        assert!(!words.contains(&"i".to_string()));
        assert!(!words.contains(&"a".to_string()));
        assert!(words.contains(&"am".to_string()));
        assert!(words.contains(&"big".to_string()));
        assert!(words.contains(&"dog".to_string()));
    }

    #[test]
    fn count_links_basic() {
        assert_eq!(count_links("[foo](bar) and [baz](qux)"), 2);
        assert_eq!(count_links("no links here"), 0);
    }

    #[test]
    fn count_citations_case_insensitive() {
        assert!(count_citations("FOIA request") > 0);
        assert!(count_citations("lara filing") > 0);
        assert!(count_citations("case no. 123") > 0);
    }

    #[test]
    fn detect_toml_format() {
        assert_eq!(detect_format("+++\ntitle = \"hi\"\n+++"), Some(FrontmatterFormat::Toml));
    }

    #[test]
    fn detect_yaml_format() {
        assert_eq!(detect_format("---\ntitle: hi\n---"), Some(FrontmatterFormat::Yaml));
    }

    #[test]
    fn content_hash_is_blake3() {
        let content = b"test page content";
        let hash = hash_page(content);
        let expected = blake3::hash(content);
        assert_eq!(hash.as_bytes(), expected.as_bytes());
    }

    #[test]
    fn short_hash_is_16_hex_chars() {
        let hash = hash_page(b"anything");
        let short = hash.to_hex()[..16].to_string();
        assert_eq!(short.len(), 16);
        assert!(short.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn mass_increases_with_diverse_content() {
        let tmp = tempfile::tempdir().unwrap();

        let small = tmp.path().join("small.md");
        std::fs::write(
            &small,
            "+++\ntitle = \"Small\"\ndate = 2026-01-01\n+++\nHello world.\n",
        )
        .unwrap();

        let big = tmp.path().join("big.md");
        let sentences = [
            "The criminal enterprise operated across state boundaries.",
            "Financial records revealed systematic fraud patterns.",
            "Investigators documented the organizational hierarchy.",
            "Environmental contamination exceeded regulatory thresholds.",
            "Community residents challenged the corporate defendants.",
            "Pharmaceutical compounds were distributed without oversight.",
            "Regulatory capture undermined enforcement mechanisms.",
            "Whistleblower testimony corroborated documentary evidence.",
            "Statistical analysis demonstrated significant deviation.",
            "Infrastructure failures cascaded through municipal systems.",
        ];
        let big_body: String = sentences.iter().cycle().take(50).copied().collect::<Vec<_>>().join(" ");
        std::fs::write(
            &big,
            format!("+++\ntitle = \"Big\"\ndate = 2026-01-01\n+++\n{big_body}\n"),
        )
        .unwrap();

        let small_mass = compute_mass(&small).unwrap();
        let big_mass = compute_mass(&big).unwrap();

        assert!(big_mass.mass > small_mass.mass, "bigger diverse content should have more mass");
        assert!(big_mass.words > small_mass.words);
    }

    #[test]
    fn different_content_different_hash() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.md");
        let b = tmp.path().join("b.md");
        std::fs::write(&a, "+++\ntitle = \"A\"\ndate = 2026-01-01\n+++\nAlpha.\n").unwrap();
        std::fs::write(&b, "+++\ntitle = \"B\"\ndate = 2026-01-01\n+++\nBravo.\n").unwrap();

        let ma = compute_mass(&a).unwrap();
        let mb = compute_mass(&b).unwrap();

        assert_ne!(ma.content_hash, mb.content_hash, "different pages → different Blake3");
        assert_ne!(ma.short_hash, mb.short_hash);
    }

    #[test]
    fn citations_increase_mass() {
        let tmp = tempfile::tempdir().unwrap();

        let base_text = "The system processes data from the public records database. ".repeat(20);

        let no_cite = tmp.path().join("nocite.md");
        std::fs::write(&no_cite, format!("+++\ntitle = \"No\"\ndate = 2026-01-01\n+++\n{base_text}\n")).unwrap();

        let with_cite = tmp.path().join("withcite.md");
        let cite_text = base_text.replace("data", "FOIA data from MDOC ICHAT LARA PACER");
        std::fs::write(&with_cite, format!("+++\ntitle = \"Yes\"\ndate = 2026-01-01\n+++\n{cite_text}\n")).unwrap();

        let no_mass = compute_mass(&no_cite).unwrap();
        let cite_mass = compute_mass(&with_cite).unwrap();

        assert!(cite_mass.citations > no_mass.citations);
        assert!(cite_mass.mass > no_mass.mass, "citations should increase mass");
    }

    #[test]
    fn weight_sorted_sections_not_empty() {
        assert!(!WEIGHT_SORTED_SECTIONS.is_empty());
        assert!(WEIGHT_SORTED_SECTIONS.contains(&"analysis"));
    }
}
