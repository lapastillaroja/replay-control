use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::process::Command;

use rcgen::{CertificateParams, CertifiedKey, DistinguishedName, DnType, KeyPair};
use replay_control_core::error::{Error, Result};

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

#[derive(Debug, Clone)]
pub struct TlsCertificatePaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CertificateNames {
    dns: BTreeSet<String>,
    ips: BTreeSet<IpAddr>,
}

pub fn ensure_self_signed_certificate(data_dir: &DataDir) -> Result<TlsCertificatePaths> {
    let tls_dir = data_dir.root().join(TLS_SUBDIR);
    let cert = tls_dir.join(CERT_FILE);
    let key = tls_dir.join(KEY_FILE);
    let san_manifest = tls_dir.join(SAN_FILE);
    let desired_names = current_certificate_names();

    if cert.exists() && key.exists() && manifest_covers_current_names(&san_manifest, &desired_names)
    {
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

fn write_certificate_files(
    tls_dir: &Path,
    cert: &Path,
    key: &Path,
    san_manifest: &Path,
    desired_names: &CertificateNames,
) -> Result<TlsCertificatePaths> {
    std::fs::create_dir_all(&tls_dir).map_err(|e| Error::io(&tls_dir, e))?;

    let CertifiedKey {
        cert: cert_pem,
        key_pair,
    } = generate_certificate(&desired_names)?;
    let cert_tmp = temp_path_for(cert);
    let key_tmp = temp_path_for(key);
    let san_tmp = temp_path_for(san_manifest);

    remove_if_exists(&cert_tmp);
    remove_if_exists(&key_tmp);
    remove_if_exists(&san_tmp);

    std::fs::write(&cert_tmp, cert_pem.pem()).map_err(|e| Error::io(&cert_tmp, e))?;
    std::fs::write(&key_tmp, key_pair.serialize_pem()).map_err(|e| Error::io(&key_tmp, e))?;
    set_private_key_permissions(&key_tmp);
    std::fs::write(&san_tmp, names_to_manifest(desired_names))
        .map_err(|e| Error::io(&san_tmp, e))?;

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

fn generate_certificate(names: &CertificateNames) -> Result<CertifiedKey> {
    let subject_alt_names = names
        .dns
        .iter()
        .cloned()
        .chain(names.ips.iter().map(|ip| ip.to_string()))
        .collect::<Vec<_>>();
    let mut params = CertificateParams::new(subject_alt_names).map_err(to_error)?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "Replay Control");
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

fn manifest_covers_current_names(path: &std::path::Path, desired: &CertificateNames) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let stored = names_from_manifest(&content);
    desired.dns.is_subset(&stored.dns) && desired.ips.is_subset(&stored.ips)
}

fn names_to_manifest(names: &CertificateNames) -> String {
    names
        .dns
        .iter()
        .map(|name| format!("dns:{name}\n"))
        .chain(names.ips.iter().map(|ip| format!("ip:{ip}\n")))
        .collect()
}

fn names_from_manifest(content: &str) -> CertificateNames {
    let mut names = CertificateNames {
        dns: BTreeSet::new(),
        ips: BTreeSet::new(),
    };
    for line in content.lines() {
        if let Some(name) = line.strip_prefix("dns:") {
            if !name.trim().is_empty() {
                names.dns.insert(name.trim().to_ascii_lowercase());
            }
        } else if let Some(ip) = line.strip_prefix("ip:")
            && let Ok(ip) = ip.trim().parse()
        {
            names.ips.insert(ip);
        }
    }
    names
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
    let hostname = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_end_matches(".local")
        .to_ascii_lowercase();
    if hostname.is_empty() {
        None
    } else {
        Some(hostname)
    }
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

    use super::{CertificateNames, ip_from_ip_addr_line, names_from_manifest, names_to_manifest};

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
}
