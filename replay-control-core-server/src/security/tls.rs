use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rcgen::{
    CertificateParams, CertifiedKey, DistinguishedName, DnType, ExtendedKeyUsagePurpose, KeyPair,
    KeyUsagePurpose,
};
use replay_control_core::error::{Error, Result};
use ring::digest;
use rustls::crypto::ring::sign::any_supported_type;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::sign::CertifiedKey as RustlsCertifiedKey;
use time::{Duration, OffsetDateTime};

use crate::data_dir::DataDir;

const TLS_SUBDIR: &str = "tls";
const CERT_FILE: &str = "replay-control-self-signed.crt";
const KEY_FILE: &str = "replay-control-self-signed.key";
const SAN_FILE: &str = "replay-control-self-signed.sans";
const DEFAULT_DNS_NAMES: &[&str] = &["replay.local", "localhost"];
const DEFAULT_IPS: &[IpAddr] = &[
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];
const CERT_POLICY_VERSION: u32 = 3;
const CERT_VALIDITY_DAYS: i64 = 397;
const CERT_RENEW_BEFORE_SECONDS: u64 = 30 * 24 * 60 * 60;

pub fn install_default_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Debug, Clone)]
pub struct TlsCertificatePaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TlsCertificateStatus {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub dns_names: Vec<String>,
    pub ip_addresses: Vec<IpAddr>,
    pub current_dns_names: Vec<String>,
    pub current_ip_addresses: Vec<IpAddr>,
    pub missing_dns_names: Vec<String>,
    pub missing_ip_addresses: Vec<IpAddr>,
    pub fingerprint_sha256: Option<String>,
    pub generated_at_unix: Option<u64>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertificateNames {
    dns: BTreeSet<String>,
    ips: BTreeSet<IpAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertificateManifest {
    names: CertificateNames,
    policy_version: Option<u32>,
    generated_at_unix: Option<u64>,
    expires_at_unix: Option<u64>,
    expires_at: Option<String>,
}

pub fn ensure_self_signed_certificate(data_dir: &DataDir) -> Result<TlsCertificatePaths> {
    let tls_dir = data_dir.root().join(TLS_SUBDIR);
    let cert = tls_dir.join(CERT_FILE);
    let key = tls_dir.join(KEY_FILE);
    let san_manifest = tls_dir.join(SAN_FILE);

    let desired_names = current_certificate_names();
    let manifest = std::fs::read_to_string(&san_manifest)
        .ok()
        .map(|content| manifest_from_str(&content))
        .unwrap_or_else(empty_manifest);

    if cert.exists()
        && key.exists()
        && reusable_certificate_manifest(&manifest, &desired_names)
        && certificate_files_are_parseable(&cert, &key)
    {
        set_private_key_permissions(&key);
        return Ok(TlsCertificatePaths { cert, key });
    }

    write_certificate_files(&tls_dir, &cert, &key, &san_manifest, &desired_names)
}

pub fn regenerate_self_signed_certificate(data_dir: &DataDir) -> Result<TlsCertificatePaths> {
    let tls_dir = data_dir.root().join(TLS_SUBDIR);
    let cert = tls_dir.join(CERT_FILE);
    let key = tls_dir.join(KEY_FILE);
    let san_manifest = tls_dir.join(SAN_FILE);
    let desired_names = current_certificate_names();

    write_certificate_files(&tls_dir, &cert, &key, &san_manifest, &desired_names)
}

pub fn tls_certificate_status(data_dir: &DataDir) -> TlsCertificateStatus {
    let tls_dir = data_dir.root().join(TLS_SUBDIR);
    let cert = tls_dir.join(CERT_FILE);
    let key = tls_dir.join(KEY_FILE);
    let san_manifest = tls_dir.join(SAN_FILE);
    let current_names = current_certificate_names();
    let manifest = std::fs::read_to_string(&san_manifest)
        .ok()
        .map(|content| manifest_from_str(&content))
        .unwrap_or_else(empty_manifest);
    let missing_dns_names = current_names
        .dns
        .difference(&manifest.names.dns)
        .cloned()
        .collect();
    let missing_ip_addresses = current_names
        .ips
        .difference(&manifest.names.ips)
        .copied()
        .collect();

    TlsCertificateStatus {
        cert_path: cert.clone(),
        key_path: key,
        dns_names: manifest.names.dns.iter().cloned().collect(),
        ip_addresses: manifest.names.ips.iter().copied().collect(),
        current_dns_names: current_names.dns.iter().cloned().collect(),
        current_ip_addresses: current_names.ips.iter().copied().collect(),
        missing_dns_names,
        missing_ip_addresses,
        fingerprint_sha256: certificate_fingerprint_sha256(&cert),
        generated_at_unix: manifest.generated_at_unix,
        expires_at: manifest.expires_at,
    }
}

fn write_certificate_files(
    tls_dir: &Path,
    cert: &Path,
    key: &Path,
    san_manifest: &Path,
    desired_names: &CertificateNames,
) -> Result<TlsCertificatePaths> {
    std::fs::create_dir_all(tls_dir).map_err(|e| Error::io(tls_dir, e))?;

    let CertifiedKey {
        cert: cert_pem,
        key_pair,
    } = generate_certificate(desired_names)?;
    let cert_tmp = temp_path_for(cert);
    let key_tmp = temp_path_for(key);
    let san_tmp = temp_path_for(san_manifest);

    remove_if_exists(&cert_tmp);
    remove_if_exists(&key_tmp);
    remove_if_exists(&san_tmp);

    write_synced_file(&cert_tmp, cert_pem.pem().as_bytes())?;
    write_private_key_file(&key_tmp, key_pair.serialize_pem().as_bytes())?;
    write_synced_file(&san_tmp, manifest_to_string(desired_names).as_bytes())?;

    if !certificate_files_are_parseable(&cert_tmp, &key_tmp) {
        remove_if_exists(&cert_tmp);
        remove_if_exists(&key_tmp);
        remove_if_exists(&san_tmp);
        return Err(Error::Other(
            "Generated HTTPS certificate and key failed validation".to_string(),
        ));
    }

    replace_file(&cert_tmp, cert)?;
    replace_file(&key_tmp, key)?;
    replace_file(&san_tmp, san_manifest)?;

    tracing::info!(
        "generated self-signed HTTPS certificate at {}",
        cert.display()
    );

    Ok(TlsCertificatePaths {
        cert: cert.to_path_buf(),
        key: key.to_path_buf(),
    })
}

fn temp_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tls-file");
    path.with_file_name(format!("{file_name}.tmp-{}", std::process::id()))
}

fn replace_file(from: &Path, to: &Path) -> Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(to).map_err(|e| Error::io(to, e))?;
            std::fs::rename(from, to).map_err(|e| Error::io(to, e))
        }
        Err(error) => Err(Error::io(to, error)),
    }
}

fn remove_if_exists(path: &Path) {
    if let Err(error) = std::fs::remove_file(path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            "failed to remove stale TLS temp file {}: {error}",
            path.display()
        );
    }
}

fn write_synced_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Error::io(path, e))?;
    std::io::Write::write_all(&mut file, bytes).map_err(|e| Error::io(path, e))?;
    file.sync_all().map_err(|e| Error::io(path, e))?;
    Ok(())
}

fn write_private_key_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = private_key_create_new_file(path)?;
    std::io::Write::write_all(&mut file, bytes).map_err(|e| Error::io(path, e))?;
    file.sync_all().map_err(|e| Error::io(path, e))?;
    Ok(())
}

#[cfg(unix)]
fn private_key_create_new_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

#[cfg(not(unix))]
fn private_key_create_new_file(path: &Path) -> Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| Error::io(path, e))
}

fn generate_certificate(names: &CertificateNames) -> Result<CertifiedKey> {
    let subject_alt_names = names
        .dns
        .iter()
        .cloned()
        .chain(names.ips.iter().map(|ip| ip.to_string()))
        .collect::<Vec<_>>();
    let mut params = CertificateParams::new(subject_alt_names).map_err(to_error)?;
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::hours(1);
    params.not_after = now + Duration::days(CERT_VALIDITY_DAYS);
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "Replay Control");
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key_pair = KeyPair::generate().map_err(to_error)?;
    let cert = params.self_signed(&key_pair).map_err(to_error)?;
    Ok(CertifiedKey { cert, key_pair })
}

fn current_certificate_names() -> CertificateNames {
    CertificateNames {
        dns: dns_names(),
        ips: ip_addresses(),
    }
}

fn manifest_to_string(names: &CertificateNames) -> String {
    let generated_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let expires_at_unix =
        generated_at_unix.saturating_add(CERT_VALIDITY_DAYS as u64 * 24 * 60 * 60);
    let expires_at = OffsetDateTime::from(UNIX_EPOCH + StdDuration::from_secs(expires_at_unix));
    format!(
        "policy_version:{CERT_POLICY_VERSION}\ngenerated_at_unix:{generated_at_unix}\nexpires_at_unix:{expires_at_unix}\nexpires_at:{}\n{}",
        format_certificate_date(expires_at),
        names_to_manifest(names)
    )
}

fn format_certificate_date(value: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        value.year(),
        u8::from(value.month()),
        value.day()
    )
}

fn names_to_manifest(names: &CertificateNames) -> String {
    names
        .dns
        .iter()
        .map(|name| format!("dns:{name}\n"))
        .chain(names.ips.iter().map(|ip| format!("ip:{ip}\n")))
        .collect()
}

fn manifest_from_str(content: &str) -> CertificateManifest {
    let mut manifest = empty_manifest();
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("policy_version:") {
            manifest.policy_version = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("generated_at_unix:") {
            manifest.generated_at_unix = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("expires_at_unix:") {
            manifest.expires_at_unix = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("expires_at:") {
            let value = value.trim();
            if !value.is_empty() {
                manifest.expires_at = Some(value.to_string());
            }
        } else if let Some(name) = line.strip_prefix("dns:") {
            if !name.trim().is_empty() {
                manifest.names.dns.insert(name.trim().to_ascii_lowercase());
            }
        } else if let Some(ip) = line.strip_prefix("ip:")
            && let Ok(ip) = ip.trim().parse()
        {
            manifest.names.ips.insert(ip);
        }
    }
    manifest
}

fn reusable_certificate_manifest(
    manifest: &CertificateManifest,
    desired_names: &CertificateNames,
) -> bool {
    if manifest.policy_version != Some(CERT_POLICY_VERSION) {
        return false;
    }
    if !certificate_manifest_covers_names(manifest, desired_names) {
        return false;
    }
    let Some(expires_at_unix) = manifest.expires_at_unix else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    expires_at_unix > now.saturating_add(CERT_RENEW_BEFORE_SECONDS)
}

fn certificate_manifest_covers_names(
    manifest: &CertificateManifest,
    desired_names: &CertificateNames,
) -> bool {
    desired_names.dns.is_subset(&manifest.names.dns)
        && desired_names.ips.is_subset(&manifest.names.ips)
}

fn empty_manifest() -> CertificateManifest {
    CertificateManifest {
        names: empty_certificate_names(),
        policy_version: None,
        generated_at_unix: None,
        expires_at_unix: None,
        expires_at: None,
    }
}

fn empty_certificate_names() -> CertificateNames {
    CertificateNames {
        dns: BTreeSet::new(),
        ips: BTreeSet::new(),
    }
}

#[cfg(test)]
fn names_from_manifest(content: &str) -> CertificateNames {
    manifest_from_str(content).names
}

fn certificate_fingerprint_sha256(path: &Path) -> Option<String> {
    let pem = std::fs::read_to_string(path).ok()?;
    let der = certificate_der_from_pem(&pem)?;
    let hash = digest::digest(&digest::SHA256, &der);
    Some(hex_fingerprint(hash.as_ref()))
}

fn certificate_files_are_parseable(cert: &Path, key: &Path) -> bool {
    let cert_chain = match certificate_chain_from_pem_file(cert) {
        Ok(cert_chain) if !cert_chain.is_empty() => cert_chain,
        Ok(_) => {
            tracing::warn!(
                "HTTPS certificate {} contains no certificates",
                cert.display()
            );
            return false;
        }
        Err(error) => {
            tracing::warn!(
                "failed to read HTTPS certificate {}: {error}",
                cert.display()
            );
            return false;
        }
    };

    let private_key = match private_key_from_pem_file(key) {
        Ok(Some(private_key)) => private_key,
        Ok(None) => {
            tracing::warn!(
                "HTTPS private key {} contains no private key",
                key.display()
            );
            return false;
        }
        Err(error) => {
            tracing::warn!(
                "failed to read HTTPS private key {}: {error}",
                key.display()
            );
            return false;
        }
    };

    let signing_key = match any_supported_type(&private_key) {
        Ok(signing_key) => signing_key,
        Err(error) => {
            tracing::warn!("HTTPS private key {} is not valid: {error}", key.display());
            return false;
        }
    };

    let certified_key = RustlsCertifiedKey::new(cert_chain, signing_key);
    if let Err(error) = certified_key.keys_match() {
        tracing::warn!(
            "HTTPS certificate {} does not match private key {}: {error}",
            cert.display(),
            key.display()
        );
        return false;
    }

    true
}

fn certificate_chain_from_pem_file(
    path: &Path,
) -> std::result::Result<Vec<CertificateDer<'static>>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::certs(&mut reader).collect()
}

fn private_key_from_pem_file(
    path: &Path,
) -> std::result::Result<Option<PrivateKeyDer<'static>>, std::io::Error> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::private_key(&mut reader)
}

fn certificate_der_from_pem(pem: &str) -> Option<Vec<u8>> {
    let mut in_certificate = false;
    let mut encoded = String::new();
    for line in pem.lines() {
        let line = line.trim();
        if line == "-----BEGIN CERTIFICATE-----" {
            in_certificate = true;
            continue;
        }
        if line == "-----END CERTIFICATE-----" {
            break;
        }
        if in_certificate {
            encoded.push_str(line);
        }
    }
    (!encoded.is_empty())
        .then(|| STANDARD.decode(encoded).ok())
        .flatten()
}

fn hex_fingerprint(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn dns_names() -> BTreeSet<String> {
    let mut names: BTreeSet<String> = DEFAULT_DNS_NAMES
        .iter()
        .map(|name| name.to_string())
        .collect();
    if let Some(hostname) = current_hostname() {
        names.insert(hostname.clone());
        names.insert(format!("{hostname}.local"));
    }
    names
}

fn ip_addresses() -> BTreeSet<IpAddr> {
    let mut addresses: BTreeSet<IpAddr> = DEFAULT_IPS.iter().copied().collect();
    addresses.extend(current_lan_ips());
    addresses
}

fn current_hostname() -> Option<String> {
    let output = Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    normalize_hostname_output(&output.stdout)
}

fn normalize_hostname_output(output: &[u8]) -> Option<String> {
    let hostname = String::from_utf8_lossy(output)
        .trim()
        .trim_end_matches(".local")
        .to_ascii_lowercase();
    is_valid_replay_hostname(&hostname).then_some(hostname)
}

fn is_valid_replay_hostname(hostname: &str) -> bool {
    !hostname.is_empty()
        && hostname.len() <= 63
        && !hostname.starts_with('-')
        && !hostname.ends_with('-')
        && hostname
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn current_lan_ips() -> Vec<IpAddr> {
    let output = Command::new("ip")
        .args(["-o", "addr", "show", "scope", "global"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(ip_from_ip_addr_line)
        .collect()
}

fn ip_from_ip_addr_line(line: &str) -> Option<IpAddr> {
    let mut parts = line.split_whitespace();
    while let Some(part) = parts.next() {
        if matches!(part, "inet" | "inet6") {
            let cidr = parts.next()?;
            let ip = cidr.split('/').next()?;
            return ip.parse().ok();
        }
    }
    None
}

#[cfg(unix)]
fn set_private_key_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = std::fs::metadata(path) {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        if let Err(error) = std::fs::set_permissions(path, permissions) {
            tracing::warn!("failed to set TLS private key permissions: {error}");
        }
    }
}

#[cfg(not(unix))]
fn set_private_key_permissions(_path: &std::path::Path) {}

fn to_error(error: impl std::fmt::Display) -> Error {
    Error::Other(format!("generate HTTPS certificate: {error}"))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use crate::data_dir::DataDir;

    use super::{
        CertificateNames, SAN_FILE, TLS_SUBDIR, certificate_der_from_pem,
        certificate_manifest_covers_names, ensure_self_signed_certificate, generate_certificate,
        ip_from_ip_addr_line, manifest_from_str, manifest_to_string, names_from_manifest,
        names_to_manifest, normalize_hostname_output, reusable_certificate_manifest,
    };

    #[test]
    fn parses_ip_addr_lines() {
        assert_eq!(
            ip_from_ip_addr_line(
                "2: eth0    inet 192.168.1.30/24 brd 192.168.1.255 scope global eth0"
            )
            .unwrap()
            .to_string(),
            "192.168.1.30"
        );
        assert_eq!(
            ip_from_ip_addr_line("3: wlan0    inet6 fd00::1/64 scope global dynamic noprefixroute")
                .unwrap()
                .to_string(),
            "fd00::1"
        );
    }

    #[test]
    fn normalizes_hostname_output_before_using_it_as_a_san() {
        assert_eq!(
            normalize_hostname_output(b"Replay-01\n").as_deref(),
            Some("replay-01")
        );
        assert_eq!(
            normalize_hostname_output(b"replay.local\n").as_deref(),
            Some("replay")
        );
        assert_eq!(normalize_hostname_output(b""), None);
        assert_eq!(normalize_hostname_output(b"-replay\n"), None);
        assert_eq!(normalize_hostname_output(b"replay-\n"), None);
        assert_eq!(normalize_hostname_output(b"replay device\n"), None);
        assert_eq!(normalize_hostname_output(b"replay.example.com\n"), None);
    }

    #[test]
    fn san_manifest_round_trips_names() {
        let names = CertificateNames {
            dns: ["localhost".to_string(), "replay.local".to_string()]
                .into_iter()
                .collect(),
            ips: [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "192.168.1.30".parse().unwrap(),
            ]
            .into_iter()
            .collect(),
        };

        assert_eq!(names_from_manifest(&names_to_manifest(&names)), names);
    }

    #[test]
    fn san_manifest_includes_managed_dates() {
        let names = CertificateNames {
            dns: ["localhost".to_string()].into_iter().collect(),
            ips: [IpAddr::V4(Ipv4Addr::LOCALHOST)].into_iter().collect(),
        };
        let parsed = manifest_from_str(&manifest_to_string(&names));

        assert_eq!(parsed.names, names);
        assert_eq!(parsed.policy_version, Some(super::CERT_POLICY_VERSION));
        assert!(parsed.generated_at_unix.is_some());
        assert!(parsed.expires_at_unix.is_some());
        assert!(parsed.expires_at.is_some());
    }

    #[test]
    fn reusable_manifest_requires_current_policy_and_fresh_expiry() {
        let desired = CertificateNames {
            dns: ["localhost".to_string(), "replay.local".to_string()]
                .into_iter()
                .collect(),
            ips: [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "192.168.1.30".parse().unwrap(),
            ]
            .into_iter()
            .collect(),
        };
        let manifest = manifest_from_str(&manifest_to_string(&desired));

        assert!(reusable_certificate_manifest(&manifest, &desired));
        assert!(certificate_manifest_covers_names(&manifest, &desired));

        let old_policy = manifest_from_str(
            "generated_at_unix:1\nexpires_at:2036-01-01\ndns:localhost\nip:127.0.0.1\n",
        );
        assert!(!reusable_certificate_manifest(&old_policy, &desired));
    }

    #[test]
    fn manifest_coverage_detects_missing_names_without_driving_reuse() {
        let stored = CertificateNames {
            dns: ["localhost".to_string()].into_iter().collect(),
            ips: [IpAddr::V4(Ipv4Addr::LOCALHOST)].into_iter().collect(),
        };
        let desired = CertificateNames {
            dns: ["localhost".to_string(), "replay.local".to_string()]
                .into_iter()
                .collect(),
            ips: [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "192.168.1.30".parse().unwrap(),
            ]
            .into_iter()
            .collect(),
        };
        let manifest = manifest_from_str(&manifest_to_string(&stored));

        assert!(!reusable_certificate_manifest(&manifest, &desired));
        assert!(!certificate_manifest_covers_names(&manifest, &desired));
    }

    #[test]
    fn san_manifest_detects_missing_new_ip() {
        let stored = CertificateNames {
            dns: ["localhost".to_string(), "replay.local".to_string()]
                .into_iter()
                .collect(),
            ips: [IpAddr::V4(Ipv4Addr::LOCALHOST)].into_iter().collect(),
        };
        let desired = CertificateNames {
            dns: ["localhost".to_string()].into_iter().collect(),
            ips: [
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "192.168.1.30".parse().unwrap(),
            ]
            .into_iter()
            .collect(),
        };
        let parsed = names_from_manifest(&names_to_manifest(&stored));

        assert!(!desired.ips.is_subset(&parsed.ips));
        assert!(desired.dns.is_subset(&parsed.dns));
    }

    #[test]
    fn existing_certificate_is_rotated_for_stale_policy_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = DataDir::new(temp.path());
        let paths = ensure_self_signed_certificate(&data_dir).unwrap();
        let initial_cert = std::fs::read(&paths.cert).unwrap();
        let manifest = temp.path().join(TLS_SUBDIR).join(SAN_FILE);
        std::fs::write(
            &manifest,
            "generated_at_unix:1\nexpires_at:2036-01-01\ndns:localhost\nip:127.0.0.1\n",
        )
        .unwrap();

        let reused = ensure_self_signed_certificate(&data_dir).unwrap();

        assert_eq!(reused.cert, paths.cert);
        assert_ne!(std::fs::read(&reused.cert).unwrap(), initial_cert);
        assert_eq!(
            manifest_from_str(&std::fs::read_to_string(&manifest).unwrap()).policy_version,
            Some(super::CERT_POLICY_VERSION)
        );
    }

    #[test]
    fn existing_certificate_is_rotated_when_manifest_lacks_current_address_coverage() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = DataDir::new(temp.path());
        let paths = ensure_self_signed_certificate(&data_dir).unwrap();
        let initial_cert = std::fs::read(&paths.cert).unwrap();
        let manifest = temp.path().join(TLS_SUBDIR).join(SAN_FILE);
        let reduced_names = CertificateNames {
            dns: ["localhost".to_string()].into_iter().collect(),
            ips: [IpAddr::V4(Ipv4Addr::LOCALHOST)].into_iter().collect(),
        };
        std::fs::write(&manifest, manifest_to_string(&reduced_names)).unwrap();

        let reused = ensure_self_signed_certificate(&data_dir).unwrap();

        assert_eq!(reused.cert, paths.cert);
        assert_ne!(std::fs::read(&reused.cert).unwrap(), initial_cert);
        let updated_manifest = manifest_from_str(&std::fs::read_to_string(&manifest).unwrap());
        assert!(updated_manifest.names.dns.is_superset(&reduced_names.dns));
        assert!(updated_manifest.names.ips.is_superset(&reduced_names.ips));
    }

    #[test]
    fn existing_certificate_is_rotated_when_pem_files_are_corrupt() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = DataDir::new(temp.path());
        let paths = ensure_self_signed_certificate(&data_dir).unwrap();
        let manifest = temp.path().join(TLS_SUBDIR).join(SAN_FILE);
        let initial_names = names_from_manifest(&std::fs::read_to_string(&manifest).unwrap());

        std::fs::write(&paths.cert, "not a certificate").unwrap();
        std::fs::write(&paths.key, "not a private key").unwrap();

        let reused = ensure_self_signed_certificate(&data_dir).unwrap();

        assert_eq!(reused.cert, paths.cert);
        assert_ne!(
            std::fs::read_to_string(&reused.cert).unwrap(),
            "not a certificate"
        );
        assert_ne!(
            std::fs::read_to_string(&reused.key).unwrap(),
            "not a private key"
        );
        assert_eq!(
            names_from_manifest(&std::fs::read_to_string(&manifest).unwrap()),
            initial_names
        );
    }

    #[test]
    fn existing_certificate_is_rotated_when_cert_and_key_do_not_match() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = DataDir::new(temp.path());
        let paths = ensure_self_signed_certificate(&data_dir).unwrap();
        let manifest = temp.path().join(TLS_SUBDIR).join(SAN_FILE);
        let initial_cert = std::fs::read_to_string(&paths.cert).unwrap();
        let initial_key = std::fs::read_to_string(&paths.key).unwrap();
        let initial_names = names_from_manifest(&std::fs::read_to_string(&manifest).unwrap());
        let different = generate_certificate(&initial_names).unwrap();

        std::fs::write(&paths.key, different.key_pair.serialize_pem()).unwrap();

        let reused = ensure_self_signed_certificate(&data_dir).unwrap();

        assert_eq!(reused.cert, paths.cert);
        assert_ne!(std::fs::read_to_string(&reused.cert).unwrap(), initial_cert);
        assert_ne!(std::fs::read_to_string(&reused.key).unwrap(), initial_key);
        assert_eq!(
            names_from_manifest(&std::fs::read_to_string(&manifest).unwrap()),
            initial_names
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_private_key_file_is_created_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let data_dir = DataDir::new(temp.path());
        let paths = ensure_self_signed_certificate(&data_dir).unwrap();

        let mode = std::fs::metadata(&paths.key).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn generated_certificate_is_valid_for_tls_server_auth() {
        let names = CertificateNames {
            dns: ["localhost".to_string(), "replay.local".to_string()]
                .into_iter()
                .collect(),
            ips: [IpAddr::V4(Ipv4Addr::LOCALHOST)].into_iter().collect(),
        };

        let generated = generate_certificate(&names).unwrap();
        let params = generated.cert.params();

        assert_eq!(
            params.key_usages,
            vec![super::KeyUsagePurpose::DigitalSignature]
        );
        assert_eq!(
            params.extended_key_usages,
            vec![super::ExtendedKeyUsagePurpose::ServerAuth]
        );
    }

    #[test]
    fn extracts_der_from_certificate_pem() {
        let pem = "-----BEGIN CERTIFICATE-----\nAQIDBA==\n-----END CERTIFICATE-----\n";

        assert_eq!(certificate_der_from_pem(pem).unwrap(), vec![1, 2, 3, 4]);
    }
}
