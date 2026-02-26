//! Content-addressable digest algorithm for bulker crate manifests.
//!
//! Computes two digests:
//! - **crate-manifest-digest**: from manifest tag strings (always available)
//! - **crate-image-digest**: from OCI content digests (requires network, optional)
//!
//! Uses sha512t24u (SHA-512 truncated to 24 bytes, base64url) and RFC-8785
//! JSON canonicalization, matching the GA4GH seqcol specification.

use serde_json::Value;
use sha2::{Digest, Sha512};
use std::collections::{HashMap, HashSet};

use crate::manifest::Manifest;

// ---------------------------------------------------------------------------
// Core hash functions (from gtars-refget)
// ---------------------------------------------------------------------------

/// Compute GA4GH sha512t24u digest: SHA-512 truncated to 24 bytes, base64url encoded.
pub fn sha512t24u<T: AsRef<[u8]>>(input: T) -> String {
    let hash = Sha512::digest(input.as_ref());
    base64_url::encode(&hash[0..24])
}

/// RFC-8785 JSON Canonicalization Scheme (JCS).
pub fn canonicalize_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 {
                    format!("{:.0}", f)
                } else {
                    format!("{}", f)
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string()
                }
            } else {
                n.to_string()
            }
        }
        Value::String(s) => serde_json::to_string(s).unwrap(),
        Value::Array(arr) => {
            let elements: Vec<String> = arr.iter().map(canonicalize_json).collect();
            format!("[{}]", elements.join(","))
        }
        Value::Object(obj) => {
            let mut sorted_keys: Vec<&String> = obj.keys().collect();
            sorted_keys.sort();
            let pairs: Vec<String> = sorted_keys
                .iter()
                .map(|key| {
                    let key_str = serde_json::to_string(key).unwrap();
                    let value_str = canonicalize_json(&obj[*key]);
                    format!("{}:{}", key_str, value_str)
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
    }
}

// ---------------------------------------------------------------------------
// Level 0: command-image pair digest
// ---------------------------------------------------------------------------

/// Compute the Level 0 digest for a single (command, image) pair.
pub fn digest_pair(command: &str, image: &str) -> String {
    let obj = serde_json::json!({"command": command, "image": image});
    let canonical = canonicalize_json(&obj);
    sha512t24u(canonical)
}

// ---------------------------------------------------------------------------
// Level 1: environment digests
// ---------------------------------------------------------------------------

/// Result of computing a crate digest, including per-command detail.
#[derive(Debug, Clone)]
pub struct CrateDigestResult {
    /// The environment digest (order-preserving).
    pub digest: String,
    /// The sorted environment digest (order-invariant, for set-equality).
    pub sorted_digest: String,
    /// Per-command pair digests in manifest order.
    pub pair_digests: Vec<(String, String, String)>, // (command, image, pair_digest)
}

/// Core digest computation from a list of (command, image_string) pairs.
fn compute_digest_from_pairs(pairs: &[(String, String)]) -> CrateDigestResult {
    let mut pair_digests = Vec::new();
    let mut digest_strings = Vec::new();

    for (cmd, img) in pairs {
        let pd = digest_pair(cmd, img);
        pair_digests.push((cmd.clone(), img.clone(), pd.clone()));
        digest_strings.push(pd);
    }

    let arr = Value::Array(digest_strings.iter().map(|s| Value::String(s.clone())).collect());
    let digest = sha512t24u(canonicalize_json(&arr));

    let mut sorted = digest_strings.clone();
    sorted.sort();
    let sorted_arr = Value::Array(sorted.into_iter().map(Value::String).collect());
    let sorted_digest = sha512t24u(canonicalize_json(&sorted_arr));

    CrateDigestResult {
        digest,
        sorted_digest,
        pair_digests,
    }
}

/// Compute the crate-manifest-digest from tag strings as written in the manifest.
pub fn crate_manifest_digest(manifest: &Manifest) -> CrateDigestResult {
    let pairs: Vec<(String, String)> = manifest.manifest.commands
        .iter()
        .map(|cmd| (cmd.command.clone(), cmd.docker_image.clone()))
        .collect();
    compute_digest_from_pairs(&pairs)
}

/// Compute the crate-image-digest from OCI content digests.
/// Returns `None` if any command's image digest is missing from the map.
pub fn crate_image_digest(
    manifest: &Manifest,
    oci_digests: &HashMap<String, String>,
) -> Option<CrateDigestResult> {
    let pairs: Vec<(String, String)> = manifest.manifest.commands
        .iter()
        .map(|cmd| {
            let oci = oci_digests.get(&cmd.docker_image)?;
            Some((cmd.command.clone(), oci.clone()))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(compute_digest_from_pairs(&pairs))
}

// ---------------------------------------------------------------------------
// OCI digest resolution
// ---------------------------------------------------------------------------

/// Parse a docker image reference into (registry, repository, tag).
fn parse_image_ref(image: &str) -> (String, String, String) {
    let (name_part, tag) = match image.rfind(':') {
        Some(idx) => (&image[..idx], &image[idx + 1..]),
        None => (image, "latest"),
    };

    // Determine registry vs repository
    let (registry, repo) = if let Some(idx) = name_part.find('/') {
        let first = &name_part[..idx];
        // If first component has a dot or colon, it's a registry hostname
        if first.contains('.') || first.contains(':') {
            (first.to_string(), name_part[idx + 1..].to_string())
        } else {
            // Docker Hub with org: docker.io/org/image
            ("registry-1.docker.io".to_string(), name_part.to_string())
        }
    } else {
        // Bare name like "python" -> Docker Hub library image
        (
            "registry-1.docker.io".to_string(),
            format!("library/{}", name_part),
        )
    };

    (registry, repo, tag.to_string())
}

/// Attempt to resolve OCI content digests for all images in a manifest.
/// Returns a map of docker_image tag → sha256:... digest.
/// Best-effort: returns None for images that can't be resolved.
pub fn resolve_oci_digests(manifest: &Manifest) -> HashMap<String, String> {
    let mut result = HashMap::new();

    for cmd in &manifest.manifest.commands {
        if result.contains_key(&cmd.docker_image) {
            continue;
        }
        match resolve_single_oci_digest(&cmd.docker_image) {
            Some(digest) => {
                result.insert(cmd.docker_image.clone(), digest);
            }
            None => {
                log::debug!("Could not resolve OCI digest for: {}", cmd.docker_image);
            }
        }
    }

    result
}

/// Resolve a single image tag to its OCI content digest via the registry API.
fn resolve_single_oci_digest(image: &str) -> Option<String> {
    let (registry, repo, tag) = parse_image_ref(image);
    let url = format!("https://{}/v2/{}/manifests/{}", registry, repo, tag);

    let resp = ureq::get(&url)
        .set(
            "Accept",
            "application/vnd.docker.distribution.manifest.v2+json, \
             application/vnd.oci.image.manifest.v1+json, \
             application/vnd.oci.image.index.v1+json, \
             application/vnd.docker.distribution.manifest.list.v2+json",
        )
        .call()
        .ok()?;

    resp.header("Docker-Content-Digest")
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// A diff between two images for the same command name.
#[derive(Debug, Clone)]
pub struct CommandImageDiff {
    pub command: String,
    pub a_image: String,
    pub b_image: String,
}

/// Structured comparison of two manifests.
#[derive(Debug, Clone)]
pub struct ManifestComparison {
    pub digest_a: String,
    pub digest_b: String,
    pub a_count: usize,
    pub b_count: usize,
    pub a_and_b_count: usize,
    pub a_only: Vec<String>,
    pub b_only: Vec<String>,
    pub same_order: Option<bool>,
    pub image_diffs: Vec<CommandImageDiff>,
}

/// Compare two manifests and produce a structured diff.
pub fn compare_manifests(a: &Manifest, b: &Manifest) -> ManifestComparison {
    let digest_a = crate_manifest_digest(a);
    let digest_b = crate_manifest_digest(b);

    // Build maps: command_name -> docker_image
    let a_map: HashMap<&str, &str> = a
        .manifest
        .commands
        .iter()
        .map(|c| (c.command.as_str(), c.docker_image.as_str()))
        .collect();
    let b_map: HashMap<&str, &str> = b
        .manifest
        .commands
        .iter()
        .map(|c| (c.command.as_str(), c.docker_image.as_str()))
        .collect();

    let a_names: HashSet<&str> = a_map.keys().copied().collect();
    let b_names: HashSet<&str> = b_map.keys().copied().collect();

    let shared: HashSet<&str> = a_names.intersection(&b_names).copied().collect();

    let mut a_only: Vec<String> = a_names.difference(&b_names).map(|s| s.to_string()).collect();
    let mut b_only: Vec<String> = b_names.difference(&a_names).map(|s| s.to_string()).collect();
    a_only.sort();
    b_only.sort();

    // Count identical pairs and find image diffs
    let mut identical_count = 0;
    let mut image_diffs = Vec::new();
    for cmd in &shared {
        let a_img = a_map[cmd];
        let b_img = b_map[cmd];
        if a_img == b_img {
            identical_count += 1;
        } else {
            image_diffs.push(CommandImageDiff {
                command: cmd.to_string(),
                a_image: a_img.to_string(),
                b_image: b_img.to_string(),
            });
        }
    }
    image_diffs.sort_by(|x, y| x.command.cmp(&y.command));

    // Check order of shared commands
    let same_order = if shared.len() >= 2 {
        let a_order: Vec<&str> = a
            .manifest
            .commands
            .iter()
            .filter(|c| shared.contains(c.command.as_str()))
            .map(|c| c.command.as_str())
            .collect();
        let b_order: Vec<&str> = b
            .manifest
            .commands
            .iter()
            .filter(|c| shared.contains(c.command.as_str()))
            .map(|c| c.command.as_str())
            .collect();
        Some(a_order == b_order)
    } else {
        None
    };

    ManifestComparison {
        digest_a: digest_a.digest,
        digest_b: digest_b.digest,
        a_count: a.manifest.commands.len(),
        b_count: b.manifest.commands.len(),
        a_and_b_count: identical_count,
        a_only,
        b_only,
        same_order,
        image_diffs,
    }
}

// ---------------------------------------------------------------------------
// JSON serialization for --json output
// ---------------------------------------------------------------------------

impl ManifestComparison {
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "digest_a": self.digest_a,
            "digest_b": self.digest_b,
            "a_count": self.a_count,
            "b_count": self.b_count,
            "a_and_b_count": self.a_and_b_count,
            "a_only": self.a_only,
            "b_only": self.b_only,
            "same_order": self.same_order,
            "image_diffs": self.image_diffs.iter().map(|d| {
                serde_json::json!({
                    "command": d.command,
                    "a_image": d.a_image,
                    "b_image": d.b_image,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestInner, PackageCommand};

    #[test]
    fn test_sha512t24u_hello_world() {
        let digest = sha512t24u("hello world");
        assert_eq!(digest, "MJ7MSJwS1utMxA9QyQLytNDtd-5RGnx6");
    }

    #[test]
    fn test_canonicalize_json_object() {
        let val = serde_json::json!({"image": "quay.io/x:1", "command": "samtools"});
        let canonical = canonicalize_json(&val);
        // Keys must be sorted: "command" before "image"
        assert_eq!(
            canonical,
            r#"{"command":"samtools","image":"quay.io/x:1"}"#
        );
    }

    #[test]
    fn test_canonicalize_json_array() {
        let val = serde_json::json!(["b", "a", "c"]);
        assert_eq!(canonicalize_json(&val), r#"["b","a","c"]"#);
    }

    #[test]
    fn test_digest_pair_deterministic() {
        let d1 = digest_pair("samtools", "quay.io/biocontainers/samtools:1.9");
        let d2 = digest_pair("samtools", "quay.io/biocontainers/samtools:1.9");
        assert_eq!(d1, d2);
        assert_eq!(d1.len(), 32); // sha512t24u produces 32-char base64url
    }

    #[test]
    fn test_digest_pair_different_image_different_digest() {
        let d1 = digest_pair("samtools", "quay.io/biocontainers/samtools:1.9");
        let d2 = digest_pair("samtools", "quay.io/biocontainers/samtools:1.14");
        assert_ne!(d1, d2);
    }

    fn make_test_manifest(commands: Vec<(&str, &str)>) -> Manifest {
        Manifest {
            manifest: ManifestInner {
                name: Some("test".to_string()),
                version: None,
                commands: commands
                    .into_iter()
                    .map(|(cmd, img)| PackageCommand {
                        command: cmd.to_string(),
                        docker_image: img.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                host_commands: vec![],
                imports: vec![],
            },
        }
    }

    #[test]
    fn test_crate_manifest_digest_deterministic() {
        let m = make_test_manifest(vec![
            ("samtools", "quay.io/samtools:1.9"),
            ("bwa", "quay.io/bwa:0.7"),
        ]);
        let d1 = crate_manifest_digest(&m);
        let d2 = crate_manifest_digest(&m);
        assert_eq!(d1.digest, d2.digest);
        assert_eq!(d1.sorted_digest, d2.sorted_digest);
    }

    #[test]
    fn test_crate_manifest_digest_order_matters() {
        let m1 = make_test_manifest(vec![
            ("samtools", "quay.io/samtools:1.9"),
            ("bwa", "quay.io/bwa:0.7"),
        ]);
        let m2 = make_test_manifest(vec![
            ("bwa", "quay.io/bwa:0.7"),
            ("samtools", "quay.io/samtools:1.9"),
        ]);
        let d1 = crate_manifest_digest(&m1);
        let d2 = crate_manifest_digest(&m2);
        // Order-preserving digest differs
        assert_ne!(d1.digest, d2.digest);
        // Sorted digest is the same
        assert_eq!(d1.sorted_digest, d2.sorted_digest);
    }

    #[test]
    fn test_crate_image_digest_missing_key_returns_none() {
        let m = make_test_manifest(vec![("samtools", "quay.io/samtools:1.9")]);
        let empty: HashMap<String, String> = HashMap::new();
        assert!(crate_image_digest(&m, &empty).is_none());
    }

    #[test]
    fn test_crate_image_digest_with_all_keys() {
        let m = make_test_manifest(vec![("samtools", "quay.io/samtools:1.9")]);
        let mut oci = HashMap::new();
        oci.insert(
            "quay.io/samtools:1.9".to_string(),
            "sha256:abc123".to_string(),
        );
        let result = crate_image_digest(&m, &oci);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.digest.len(), 32);
    }

    #[test]
    fn test_compare_identical_manifests() {
        let m = make_test_manifest(vec![
            ("samtools", "quay.io/samtools:1.9"),
            ("bwa", "quay.io/bwa:0.7"),
        ]);
        let cmp = compare_manifests(&m, &m);
        assert_eq!(cmp.digest_a, cmp.digest_b);
        assert_eq!(cmp.a_count, 2);
        assert_eq!(cmp.b_count, 2);
        assert_eq!(cmp.a_and_b_count, 2);
        assert!(cmp.a_only.is_empty());
        assert!(cmp.b_only.is_empty());
        assert!(cmp.image_diffs.is_empty());
    }

    #[test]
    fn test_compare_different_manifests() {
        let m1 = make_test_manifest(vec![
            ("samtools", "quay.io/samtools:1.9"),
            ("old_tool", "quay.io/old:1.0"),
        ]);
        let m2 = make_test_manifest(vec![
            ("samtools", "quay.io/samtools:1.14"),
            ("new_tool", "quay.io/new:1.0"),
        ]);
        let cmp = compare_manifests(&m1, &m2);
        assert_ne!(cmp.digest_a, cmp.digest_b);
        assert_eq!(cmp.a_only, vec!["old_tool"]);
        assert_eq!(cmp.b_only, vec!["new_tool"]);
        assert_eq!(cmp.image_diffs.len(), 1);
        assert_eq!(cmp.image_diffs[0].command, "samtools");
    }

    #[test]
    fn test_parse_image_ref_docker_hub() {
        let (reg, repo, tag) = parse_image_ref("python:3.7");
        assert_eq!(reg, "registry-1.docker.io");
        assert_eq!(repo, "library/python");
        assert_eq!(tag, "3.7");
    }

    #[test]
    fn test_parse_image_ref_quay() {
        let (reg, repo, tag) = parse_image_ref("quay.io/biocontainers/samtools:1.9");
        assert_eq!(reg, "quay.io");
        assert_eq!(repo, "biocontainers/samtools");
        assert_eq!(tag, "1.9");
    }

    #[test]
    fn test_parse_image_ref_no_tag() {
        let (_, _, tag) = parse_image_ref("python");
        assert_eq!(tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_org_no_registry() {
        let (reg, repo, tag) = parse_image_ref("nsheff/cowsay:latest");
        assert_eq!(reg, "registry-1.docker.io");
        assert_eq!(repo, "nsheff/cowsay");
        assert_eq!(tag, "latest");
    }
}
