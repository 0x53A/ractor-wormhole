// Note: this file was fully LLM generated

use quote::quote;
use std::{collections::HashMap, fs, path::Path};

use proc_macro::TokenStream;

pub fn generate_deps(_input: TokenStream) -> TokenStream {
    // Try to read the Cargo.toml for the crate being compiled.
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&manifest_path).unwrap_or_default();

    let crates = from_cargo_toml(cargo_toml);

    // Build lines of generated module
    let mut lines: Vec<String> = Vec::new();
    lines.push("/// re-export all dependencies".to_string());
    lines.push("pub mod deps {".to_string());
    lines.push("    //! Re-exports of direct dependencies used by ractor_wormhole.".to_string());
    lines.push(
        "    //! These are gated to match Cargo.toml (optional features / target-specific crates)."
            .to_string(),
    );
    lines.push("".to_string());

    for (dep, cfg_expr) in crates {
        // skip the ractor_wormhole_derive path dep
        if dep == "ractor_wormhole_derive" {
            continue;
        }

        if !cfg_expr.is_empty() {
            lines.push(format!("    #[cfg({})]", cfg_expr));
        }
        lines.push(format!("    pub use {};", dep));
        lines.push(String::new());
    }

    lines.push("}".to_string());

    let generated = lines.join("\n");

    // Parse and return as TokenStream
    match generated.parse::<proc_macro2::TokenStream>() {
        Ok(ts) => ts.into(),
        Err(e) => {
            let err_msg = format!(
                "compile_error!({:?});",
                format!("generate_deps parse error: {}", e)
            );
            err_msg
                .parse()
                .unwrap_or_else(|_| TokenStream::from(quote! { /* failed to emit deps */ }))
        }
    }
}

// New helper: parse Cargo.toml content and return ordered list of (crate_ident, cfg_expression)
fn from_cargo_toml(content: String) -> Vec<(String, String)> {
    // Helpers to find sections
    fn section_by_header<'a>(content: &'a str, header_exact: &str) -> Option<String> {
        for (i, line) in content.lines().enumerate() {
            if line.trim() == header_exact {
                let mut collected = String::new();
                for l in content.lines().skip(i + 1) {
                    if l.trim().starts_with('[') {
                        break;
                    }
                    collected.push_str(l);
                    collected.push('\n');
                }
                return Some(collected);
            }
        }
        None
    }

    // preserve order: return Vec<(name, rhs)>
    fn parse_deps_ordered(section: &str) -> Vec<(String, String)> {
        let mut vec = Vec::new();
        for raw_line in section.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(eq) = line.find('=') {
                let name_part = line[..eq].trim();
                if name_part.starts_with('[') || name_part.starts_with('"') {
                    continue;
                }
                let rhs = line[eq + 1..].to_string();
                vec.push((name_part.to_string(), rhs));
            }
        }
        vec
    }

    // simple features parser: feature = ["a", "b"]
    fn parse_features(section: &str) -> HashMap<String, Vec<String>> {
        let mut features = HashMap::new();
        for raw_line in section.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_string();
                let val = line[eq + 1..].trim();
                if let Some(start) = val.find('[') {
                    if let Some(end) = val.rfind(']') {
                        let inside = &val[start + 1..end];
                        let items = inside
                            .split(',')
                            .map(|s| s.trim().trim_matches('"').to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>();
                        features.insert(key, items);
                    } else {
                        features.insert(key, Vec::new());
                    }
                } else {
                    features.insert(key, Vec::new());
                }
            }
        }
        features
    }

    // which features mention a dep
    fn features_referencing<'a>(
        features_map: &'a HashMap<String, Vec<String>>,
        dep_name: &str,
    ) -> Vec<String> {
        let mut found = Vec::new();
        for (feat, items) in features_map {
            for item in items {
                if item == dep_name || item.starts_with(&format!("{}/", dep_name)) {
                    found.push(feat.clone());
                    break;
                }
            }
        }
        found
    }

    // convert crate name to rust ident
    fn crate_ident(name: &str) -> String {
        name.replace('-', "_")
    }

    // Gather sections
    let main_section = section_by_header(&content, "[dependencies]").unwrap_or_default();
    let main_deps = parse_deps_ordered(&main_section);

    let mut wasm_deps_section = String::new();
    if let Some(s) = section_by_header(
        &content,
        "[target.'cfg(target_arch = \"wasm32\")'.dependencies]",
    ) {
        wasm_deps_section = s;
    } else {
        // heuristic: find a header line containing target_arch = "wasm32"
        for line in content.lines() {
            if line.contains("target_arch = \"wasm32\"") && line.trim().starts_with('[') {
                if let Some(s) = section_by_header(&content, line.trim()) {
                    wasm_deps_section = s;
                    break;
                }
            }
        }
    }
    let wasm_deps = if wasm_deps_section.is_empty() {
        Vec::new()
    } else {
        parse_deps_ordered(&wasm_deps_section)
    };

    let features_section = section_by_header(&content, "[features]").unwrap_or_default();
    let features_map = parse_features(&features_section);

    let mut out: Vec<(String, String)> = Vec::new();

    // main deps (preserve order)
    for (dep, rhs) in main_deps {
        // skip path-dep on derive crate
        if dep == "ractor_wormhole_derive" {
            continue;
        }

        let mut cfg_expr = String::new();
        let feats = features_referencing(&features_map, &dep);
        if !feats.is_empty() {
            if feats.len() == 1 {
                cfg_expr = format!(r#"feature = "{}""#, feats[0]);
            } else {
                let inner = feats
                    .into_iter()
                    .map(|f| format!(r#"feature = "{}""#, f))
                    .collect::<Vec<_>>()
                    .join(", ");
                cfg_expr = format!("any({})", inner);
            }
        } else if rhs.contains("optional") && rhs.contains("true") {
            let fallback = crate_ident(&dep);
            cfg_expr = format!(r#"feature = "{}""#, fallback);
        }
        out.push((crate_ident(&dep), cfg_expr));
    }

    // wasm deps (preserve order)
    for (dep, rhs) in wasm_deps {
        let cfg_expr;
        let feats = features_referencing(&features_map, &dep);
        if !feats.is_empty() {
            if feats.len() == 1 {
                cfg_expr = format!(r#"all(target_arch = "wasm32", feature = "{}")"#, feats[0]);
            } else {
                let inner = feats
                    .into_iter()
                    .map(|f| format!(r#"feature = "{}""#, f))
                    .collect::<Vec<_>>()
                    .join(", ");
                cfg_expr = format!(r#"all(target_arch = "wasm32", any({}))"#, inner);
            }
        } else if rhs.contains("optional") && rhs.contains("true") {
            let fallback = crate_ident(&dep);
            cfg_expr = format!(r#"all(target_arch = "wasm32", feature = "{}")"#, fallback);
        } else {
            cfg_expr = r#"target_arch = "wasm32""#.to_string();
        }
        out.push((crate_ident(&dep), cfg_expr));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::from_cargo_toml;

    #[test]
    fn test_from_cargo_toml_basic() {
        let toml = r#"
[package]
name = "ractor_wormhole"

[dependencies]
ractor_wormhole_derive = { path = "../ractor_wormhole_derive" }
ractor = { version = "0.15.8", features = [] }
tungstenite = { version = "0.26.2", optional = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
wasm-bindgen-futures = "0.4.54"
ewebsock = { version = "0.8.0", features = ["tls"], optional = true }

[features]
websocket_client = ["tungstenite"]
websocket_client_wasm = ["ewebsock"]
"#
        .to_string();

        let vec = from_cargo_toml(toml);
        // ractor should be unconditional
        assert!(vec.iter().any(|(n, c)| n == "ractor" && c.is_empty()));
        // tungstenite should have any(...) or feature guard related to websocket_client feature
        assert!(
            vec.iter()
                .any(|(n, c)| n == "tungstenite" && (c.contains("feature") || c.contains("any(")))
        );
        // wasm-bindgen-futures must be target gated
        assert!(
            vec.iter()
                .any(|(n, c)| n == "wasm_bindgen_futures" && c.contains("target_arch"))
        );
        // ewebsock should be all(target_arch..., feature = "websocket_client_wasm")
        assert!(
            vec.iter()
                .any(|(n, c)| n == "ewebsock" && c.contains("all(target_arch"))
        );
    }

    #[test]
    fn test_from_cargo_toml_ordering_and_skip() {
        let toml = r#"
[dependencies]
first = "1.0"
second = "1.0"
ractor_wormhole_derive = { path = "../ractor_wormhole_derive" }
third = { version = "1.0", optional = true }

[features]
third = []
"#
        .to_string();

        let vec = from_cargo_toml(toml);
        // ordering preserved for first/second/third (derive skipped)
        let names: Vec<String> = vec.iter().map(|(n, _)| n.clone()).collect();
        assert_eq!(
            names,
            vec![
                "first".to_string(),
                "second".to_string(),
                "third".to_string()
            ]
        );
    }
}
