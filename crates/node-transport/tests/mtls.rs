#![allow(clippy::unwrap_used)]
use ora_node_transport::{
    Acceptor, FrameReceiver, FrameSender,
    mtls::MutualTlsFiles,
    websocket::{self, MutualTlsWsAcceptor, WsEndpoint},
};
use std::{collections::BTreeMap, fs, path::Path, time::Duration};

fn certificate(
    root: &Path,
    name: &str,
    server: bool,
    ca: &rcgen::CertifiedIssuer<'static, rcgen::KeyPair>,
) -> MutualTlsFiles {
    let key = rcgen::KeyPair::generate().unwrap();
    let mut parameters = rcgen::CertificateParams::new(if server {
        vec!["localhost".into()]
    } else {
        Vec::new()
    })
    .unwrap();
    parameters.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    parameters.extended_key_usages = vec![if server {
        rcgen::ExtendedKeyUsagePurpose::ServerAuth
    } else {
        rcgen::ExtendedKeyUsagePurpose::ClientAuth
    }];
    let certificate = parameters.signed_by(&key, ca).unwrap();
    let cert = root.join(format!("{name}.pem"));
    let private = root.join(format!("{name}.key"));
    let trust = root.join("ca.pem");
    fs::write(&cert, certificate.pem()).unwrap();
    fs::write(&private, key.serialize_pem()).unwrap();
    fs::write(&trust, ca.pem()).unwrap();
    MutualTlsFiles {
        certificate_file: cert,
        private_key_file: private,
        ca_file: trust,
        peer_certificate_file: None,
    }
}

#[tokio::test]
async fn management_tls_authenticates_both_peers_and_refuses_workload_certificate() {
    let root = tempfile::tempdir().unwrap();
    let key = rcgen::KeyPair::generate().unwrap();
    let mut parameters = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
    parameters.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    parameters.key_usages = vec![rcgen::KeyUsagePurpose::KeyCertSign];
    let issuer = rcgen::CertifiedIssuer::self_signed(parameters, key).unwrap();
    let client = certificate(root.path(), "controller", false, &issuer);
    let workload = certificate(root.path(), "workload", false, &issuer);
    let mut server = certificate(root.path(), "node", true, &issuer);
    server.peer_certificate_file = Some(client.certificate_file.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let acceptor = MutualTlsWsAcceptor::new(listener, "/ora-node/v1", &server).unwrap();
    let served = tokio::spawn(async move {
        for accepted in 0..2 {
            let pending = acceptor.accept().await.unwrap();
            let opened = acceptor.open(pending).await;
            if accepted == 0 {
                assert!(opened.is_err());
                continue;
            }
            let (mut read, mut write) = opened.unwrap();
            let frame = read.recv().await.unwrap().unwrap();
            write.send(frame).await.unwrap();
        }
    });
    let mut endpoint = WsEndpoint {
        url: format!("wss://localhost:{}/ora-node/v1", address.port()),
        headers: BTreeMap::new(),
        tls: Some(workload),
    };
    assert!(websocket::connect(&endpoint).await.is_err());
    endpoint.tls = Some(client);
    let (mut read, mut write) = websocket::connect(&endpoint).await.unwrap();
    write.send(vec![1, 2, 3]).await.unwrap();
    assert_eq!(read.recv().await.unwrap(), Some(vec![1, 2, 3]));
    tokio::time::timeout(Duration::from_secs(2), served)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn management_tls_never_falls_back_to_plaintext_even_without_headers() {
    let endpoint = WsEndpoint {
        url: "ws://localhost:9000/ora-node/v1".into(),
        headers: BTreeMap::new(),
        tls: Some(MutualTlsFiles {
            certificate_file: "missing".into(),
            private_key_file: "missing".into(),
            ca_file: "missing".into(),
            peer_certificate_file: None,
        }),
    };
    assert!(endpoint.validate().is_err());
}
