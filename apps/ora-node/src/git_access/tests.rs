#![allow(clippy::unwrap_used)]

use super::*;
use pretty_assertions::assert_eq;

/// Writes a CA and a clientAuth leaf signed by it, mirroring the Sandbox Server's launch material.
fn material(root: &std::path::Path) -> GitAccessConfig {
    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let ca = rcgen::CertifiedIssuer::self_signed(ca_params, rcgen::KeyPair::generate().unwrap())
        .unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let client = params.signed_by(&key, &ca).unwrap();
    let config = GitAccessConfig {
        ca_cert: root.join("git-ca.pem"),
        client_cert: root.join("git-client.pem"),
        client_key: root.join("git-client-key.pem"),
    };
    std::fs::write(&config.ca_cert, ca.pem()).unwrap();
    std::fs::write(&config.client_cert, client.pem()).unwrap();
    std::fs::write(&config.client_key, key.serialize_pem()).unwrap();
    config
}

#[test]
fn issued_material_validates() {
    let root = tempfile::tempdir().unwrap();
    material(root.path()).validate().unwrap();
}

/// Every unusable configuration fails closed with a bounded code that names no path or content.
#[test]
fn unusable_material_fails_closed_with_bounded_codes() {
    let root = tempfile::tempdir().unwrap();
    let valid = material(root.path());
    let relative = GitAccessConfig {
        client_key: PathBuf::from("git-client-key.pem"),
        ..valid.clone()
    };
    let missing = GitAccessConfig {
        client_cert: root.path().join("absent.pem"),
        ..valid.clone()
    };
    let combined = root.path().join("combined-ca.pem");
    let mut bytes = std::fs::read(&valid.ca_cert).unwrap();
    bytes.extend(std::fs::read(&valid.client_key).unwrap());
    std::fs::write(&combined, bytes).unwrap();
    let combined_ca = GitAccessConfig {
        ca_cert: combined,
        ..valid.clone()
    };
    let mismatched = GitAccessConfig {
        client_key: valid.ca_cert.clone(),
        ..valid.clone()
    };
    let outcomes: Vec<String> = [relative, missing, combined_ca, mismatched]
        .iter()
        .map(|config| config.validate().unwrap_err().to_string())
        .collect();
    assert_eq!(
        outcomes,
        vec![
            "git_access_configuration_invalid",
            "git_access_tls_invalid",
            "git_access_tls_invalid",
            "git_access_tls_invalid",
        ]
    );
}

/// Unknown keys and a deployment-chosen server address are rejected; the address belongs to
/// each execution input.
#[test]
fn configuration_accepts_only_material_paths() {
    let parsed: GitAccessConfig = serde_json::from_value(serde_json::json!({
        "ca_cert": "/run/ora/git-ca.pem",
        "client_cert": "/run/ora/git-client.pem",
        "client_key": "/run/ora/git-client-key.pem",
    }))
    .unwrap();
    assert_eq!(
        parsed,
        GitAccessConfig {
            ca_cert: PathBuf::from("/run/ora/git-ca.pem"),
            client_cert: PathBuf::from("/run/ora/git-client.pem"),
            client_key: PathBuf::from("/run/ora/git-client-key.pem"),
        }
    );
    assert!(
        serde_json::from_value::<GitAccessConfig>(serde_json::json!({
            "server_url": "https://git-server.invalid",
            "ca_cert": "/run/ora/git-ca.pem",
            "client_cert": "/run/ora/git-client.pem",
            "client_key": "/run/ora/git-client-key.pem",
        }))
        .is_err()
    );
}
