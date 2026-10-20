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

/// A parsed docker image reference.
#[derive(Debug, Clone, PartialEq)]
struct ImageRef {
    registry: String,
    repo: String,
    /// Tag (defaults to "latest"). Ignored when `digest` is set.
    tag: String,
    /// Content digest from an `@sha256:...` reference, if present.
    digest: Option<String>,
}

/// Parse a docker image reference into registry, repository, tag and optional digest.
/// The tag is taken only from the last path segment, so registry ports
/// (`localhost:5000/foo`) are not mistaken for tags.
fn parse_image_ref(image: &str) -> ImageRef {
    let (name_tag, digest) = match image.rfind('@') {
        Some(idx) => (&image[..idx], Some(image[idx + 1..].to_string())),
        None => (image, None),
    };

    let seg_start = name_tag.rfind('/').map(|i| i + 1).unwrap_or(0);
    let (name_part, tag) = match name_tag[seg_start..].rfind(':') {
        Some(idx) => (&name_tag[..seg_start + idx], &name_tag[seg_start + idx + 1..]),
        None => (name_tag, "latest"),
    };

    // Determine registry vs repository
    let (registry, repo) = if let Some(idx) = name_part.find('/') {
        let first = &name_part[..idx];
        // If first component has a dot or colon (or is localhost), it's a registry hostname
        if first.contains('.') || first.contains(':') || first == "localhost" {
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

    ImageRef { registry, repo, tag: tag.to_string(), digest }
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

/// A parsed `WWW-Authenticate: Bearer ...` challenge.
#[derive(Debug, Clone, PartialEq)]
struct BearerChallenge {
    realm: String,
    service: Option<String>,
    scope: Option<String>,
}

/// Parse a `WWW-Authenticate` header value of the form
/// `Bearer realm="...",service="...",scope="..."`. Returns None if it is not
/// a Bearer challenge or has no realm. Quoted values may contain commas.
fn parse_bearer_challenge(header: &str) -> Option<BearerChallenge> {
    let header = header.trim();
    let scheme_end = header.find(char::is_whitespace)?;
    if !header[..scheme_end].eq_ignore_ascii_case("bearer") {
        return None;
    }
    let mut params: HashMap<String, String> = HashMap::new();
    let mut chars = header[scheme_end..].chars().peekable();
    loop {
        while matches!(chars.peek(), Some(c) if c.is_whitespace() || *c == ',') {
            chars.next();
        }
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' { break; }
            key.push(c);
            chars.next();
        }
        if chars.next().is_none() {
            break; // no '=' found: end of input
        }
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => { if let Some(n) = chars.next() { value.push(n); } }
                    '"' => break,
                    _ => value.push(c),
                }
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' { break; }
                value.push(c);
                chars.next();
            }
        }
        params.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    Some(BearerChallenge {
        realm: params.remove("realm").filter(|r| !r.is_empty())?,
        service: params.remove("service"),
        scope: params.remove("scope"),
    })
}

/// Extract the bearer token from a token-endpoint JSON response
/// (`token`, falling back to `access_token`).
fn token_from_response(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    ["token", "access_token"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
        .map(str::to_string)
}

const MANIFEST_ACCEPT: &str = "application/vnd.docker.distribution.manifest.v2+json, \
     application/vnd.oci.image.manifest.v1+json, \
     application/vnd.oci.image.index.v1+json, \
     application/vnd.docker.distribution.manifest.list.v2+json";

/// Resolve a single image tag to its OCI content digest via the registry API.
/// A reference that already pins a digest (`image@sha256:...`) is returned
/// as-is with no network call. Supports the anonymous bearer-token flow
/// (e.g. Docker Hub): on 401, fetch a token from the challenge realm and retry once.
fn resolve_single_oci_digest(image: &str) -> Option<String> {
    let r = parse_image_ref(image);
    if let Some(d) = r.digest {
        return Some(d);
    }
    let url = format!("https://{}/v2/{}/manifests/{}", r.registry, r.repo, r.tag);

    let resp = match ureq::get(&url).set("Accept", MANIFEST_ACCEPT).call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(401, resp)) => {
            let challenge = parse_bearer_challenge(resp.header("WWW-Authenticate")?)?;
            let mut req = ureq::get(&challenge.realm);
            if let Some(service) = &challenge.service {
                req = req.query("service", service);
            }
            let scope = challenge
                .scope
                .clone()
                .unwrap_or_else(|| format!("repository:{}:pull", r.repo));
            req = req.query("scope", &scope);
            let body = req.call().ok()?.into_string().ok()?;
            let token = token_from_response(&body)?;
            ureq::get(&url)
                .set("Accept", MANIFEST_ACCEPT)
                .set("Authorization", &format!("Bearer {}", token))
                .call()
                .ok()?
        }
        Err(_) => return None,
    };

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
// JSON serialization for `crate digest --json`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct CommandDigestReport {
    pub command: String,
    pub image: String,
    pub pair_digest: String,
    pub oci_digest: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct CrateDigestReport {
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub crate_manifest_digest: String,
    pub crate_image_digest: Option<String>,
    pub commands: Vec<CommandDigestReport>,
    /// Imported crates, as listed in the manifest. Their commands are not
    /// included in the digests or `commands`; digest them separately.
    pub imports: Vec<String>,
}

/// Build the --json report. Pure: no network, no filesystem.
/// `oci_digests` is None when --resolve was not requested.
/// `crate_image_digest` is the final chosen value (computed or sidecar).
/// `imports` are the manifest's import registry paths, listed as-is.
pub fn crate_digest_report(
    crate_name: &str,
    manifest_result: &CrateDigestResult,
    crate_image_digest: Option<String>,
    oci_digests: Option<&HashMap<String, String>>,
    imports: &[String],
) -> CrateDigestReport {
    CrateDigestReport {
        crate_name: crate_name.to_string(),
        crate_manifest_digest: manifest_result.digest.clone(),
        crate_image_digest,
        commands: manifest_result
            .pair_digests
            .iter()
            .map(|(cmd, img, pd)| CommandDigestReport {
                command: cmd.clone(),
                image: img.clone(),
                pair_digest: pd.clone(),
                oci_digest: oci_digests.and_then(|m| m.get(img).cloned()),
            })
            .collect(),
        imports: imports.to_vec(),
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
    fn test_crate_digest_report_unresolved() {
        let m = make_test_manifest(vec![("samtools", "quay.io/samtools:1.9"), ("bwa", "quay.io/bwa:0.7")]);
        let r = crate_manifest_digest(&m);
        let rep = crate_digest_report("ns/test:1", &r, None, None, &[]);
        assert_eq!(rep.crate_name, "ns/test:1");
        assert_eq!(rep.crate_manifest_digest, r.digest);
        assert!(rep.crate_image_digest.is_none());
        assert_eq!(rep.commands.len(), 2);
        assert_eq!(rep.commands[0].command, "samtools");
        assert_eq!(rep.commands[1].command, "bwa");
        for (c, (_, _, pd)) in rep.commands.iter().zip(&r.pair_digests) {
            assert_eq!(&c.pair_digest, pd);
            assert!(c.oci_digest.is_none());
        }
    }

    #[test]
    fn test_crate_digest_report_resolved() {
        let m = make_test_manifest(vec![("samtools", "quay.io/samtools:1.9"), ("bwa", "quay.io/bwa:0.7")]);
        let r = crate_manifest_digest(&m);
        let mut oci = HashMap::new();
        oci.insert("quay.io/samtools:1.9".to_string(), "sha256:abc".to_string());
        let rep = crate_digest_report("ns/test:1", &r, Some("img-digest".to_string()), Some(&oci), &[]);
        assert_eq!(rep.crate_image_digest.as_deref(), Some("img-digest"));
        assert_eq!(rep.commands[0].oci_digest.as_deref(), Some("sha256:abc"));
        assert!(rep.commands[1].oci_digest.is_none());
    }

    #[test]
    fn test_crate_digest_report_json_shape() {
        let m = make_test_manifest(vec![("samtools", "quay.io/samtools:1.9")]);
        let r = crate_manifest_digest(&m);
        let rep = crate_digest_report("ns/test:1", &r, None, None, &[]);
        let v = serde_json::to_value(&rep).unwrap();
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["commands", "crate", "crate_image_digest", "crate_manifest_digest", "imports"]);
        assert_eq!(obj["imports"], serde_json::json!([]));
        assert_eq!(obj["crate_image_digest"], Value::Null);
        let cmd = obj["commands"][0].as_object().unwrap();
        let mut ck: Vec<&str> = cmd.keys().map(|k| k.as_str()).collect();
        ck.sort();
        assert_eq!(ck, vec!["command", "image", "oci_digest", "pair_digest"]);
        assert_eq!(cmd["oci_digest"], Value::Null);
        assert!(serde_json::to_string(&rep).unwrap().starts_with("{\"crate\":"));
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

    fn img(reg: &str, repo: &str, tag: &str, digest: Option<&str>) -> ImageRef {
        ImageRef {
            registry: reg.to_string(),
            repo: repo.to_string(),
            tag: tag.to_string(),
            digest: digest.map(str::to_string),
        }
    }

    #[test]
    fn test_parse_image_ref_docker_hub() {
        assert_eq!(parse_image_ref("python:3.7"), img("registry-1.docker.io", "library/python", "3.7", None));
    }

    #[test]
    fn test_parse_image_ref_quay() {
        assert_eq!(
            parse_image_ref("quay.io/biocontainers/samtools:1.9"),
            img("quay.io", "biocontainers/samtools", "1.9", None)
        );
    }

    #[test]
    fn test_parse_image_ref_no_tag() {
        assert_eq!(parse_image_ref("python").tag, "latest");
    }

    #[test]
    fn test_parse_image_ref_org_no_registry() {
        assert_eq!(
            parse_image_ref("nsheff/cowsay:latest"),
            img("registry-1.docker.io", "nsheff/cowsay", "latest", None)
        );
    }

    #[test]
    fn test_parse_image_ref_registry_port_no_tag() {
        assert_eq!(parse_image_ref("localhost:5000/foo"), img("localhost:5000", "foo", "latest", None));
        assert_eq!(
            parse_image_ref("localhost:5000/org/foo:1.2"),
            img("localhost:5000", "org/foo", "1.2", None)
        );
        assert_eq!(parse_image_ref("localhost/foo"), img("localhost", "foo", "latest", None));
    }

    #[test]
    fn test_parse_image_ref_digest() {
        let d = "sha256:0123456789abcdef";
        assert_eq!(
            parse_image_ref(&format!("python@{}", d)),
            img("registry-1.docker.io", "library/python", "latest", Some(d))
        );
        assert_eq!(
            parse_image_ref(&format!("quay.io/org/tool:1.0@{}", d)),
            img("quay.io", "org/tool", "1.0", Some(d))
        );
        assert_eq!(
            parse_image_ref(&format!("localhost:5000/foo@{}", d)),
            img("localhost:5000", "foo", "latest", Some(d))
        );
    }

    #[test]
    fn test_resolve_digest_reference_without_network() {
        let d = "sha256:0123456789abcdef";
        assert_eq!(resolve_single_oci_digest(&format!("quay.io/org/tool:1.0@{}", d)).as_deref(), Some(d));
    }

    #[test]
    fn test_parse_bearer_challenge_docker_hub() {
        let h = r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/python:pull""#;
        assert_eq!(
            parse_bearer_challenge(h),
            Some(BearerChallenge {
                realm: "https://auth.docker.io/token".to_string(),
                service: Some("registry.docker.io".to_string()),
                scope: Some("repository:library/python:pull".to_string()),
            })
        );
    }

    #[test]
    fn test_parse_bearer_challenge_variants() {
        // Case-insensitive scheme, spaces after commas, commas inside quotes, unquoted value
        let c = parse_bearer_challenge(
            r#"bearer realm="https://ghcr.io/token", scope="repository:a/b:pull,push", service=ghcr.io"#,
        )
        .unwrap();
        assert_eq!(c.realm, "https://ghcr.io/token");
        assert_eq!(c.scope.as_deref(), Some("repository:a/b:pull,push"));
        assert_eq!(c.service.as_deref(), Some("ghcr.io"));

        // Realm only
        let c = parse_bearer_challenge(r#"Bearer realm="https://x/token""#).unwrap();
        assert_eq!(c.service, None);
        assert_eq!(c.scope, None);
    }

    #[test]
    fn test_parse_bearer_challenge_rejects() {
        assert_eq!(parse_bearer_challenge(r#"Basic realm="x""#), None);
        assert_eq!(parse_bearer_challenge(r#"Bearer service="x""#), None);
        assert_eq!(parse_bearer_challenge("Bearer"), None);
        assert_eq!(parse_bearer_challenge(""), None);
    }

    #[test]
    fn test_token_from_response() {
        assert_eq!(token_from_response(r#"{"token":"abc","access_token":"xyz"}"#).as_deref(), Some("abc"));
        assert_eq!(token_from_response(r#"{"access_token":"xyz"}"#).as_deref(), Some("xyz"));
        assert_eq!(token_from_response(r#"{"token":"","access_token":"xyz"}"#).as_deref(), Some("xyz"));
        assert_eq!(token_from_response(r#"{"other":1}"#), None);
        assert_eq!(token_from_response("not json"), None);
    }
}
