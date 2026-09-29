//! Deployment-owned certificate files for management channels, outside workload directories.
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutualTlsFiles {
    pub certificate_file: PathBuf,
    pub private_key_file: PathBuf,
    pub ca_file: PathBuf,
    #[serde(default)]
    pub peer_certificate_file: Option<PathBuf>,
}

impl MutualTlsFiles {
    fn certificate(&self) -> io::Result<Vec<CertificateDer<'static>>> {
        let pem = std::fs::read(&self.certificate_file)?;
        CertificateDer::pem_slice_iter(&pem)
            .map(|v| v.map_err(io::Error::other))
            .collect()
    }
    fn key(&self) -> io::Result<PrivateKeyDer<'static>> {
        PrivateKeyDer::from_pem_slice(&std::fs::read(&self.private_key_file)?)
            .map_err(io::Error::other)
    }
    fn roots(&self) -> io::Result<RootCertStore> {
        let pem = std::fs::read(&self.ca_file)?;
        let mut roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(&pem) {
            roots
                .add(cert.map_err(io::Error::other)?)
                .map_err(io::Error::other)?;
        }
        if roots.is_empty() {
            return Err(io::Error::other("empty management trust roots"));
        }
        Ok(roots)
    }
    pub fn client(&self) -> io::Result<Arc<ClientConfig>> {
        Ok(Arc::new(
            ClientConfig::builder()
                .with_root_certificates(self.roots()?)
                .with_client_auth_cert(self.certificate()?, self.key()?)
                .map_err(io::Error::other)?,
        ))
    }
    pub fn server(&self) -> io::Result<Arc<ServerConfig>> {
        let verify = rustls::server::WebPkiClientVerifier::builder(Arc::new(self.roots()?))
            .build()
            .map_err(io::Error::other)?;
        Ok(Arc::new(
            ServerConfig::builder()
                .with_client_cert_verifier(verify)
                .with_single_cert(self.certificate()?, self.key()?)
                .map_err(io::Error::other)?,
        ))
    }
    pub fn peer_certificate(&self) -> io::Result<CertificateDer<'static>> {
        let path = self
            .peer_certificate_file
            .as_ref()
            .ok_or_else(|| io::Error::other("management peer certificate pin is required"))?;
        CertificateDer::from_pem_slice(&std::fs::read(path)?).map_err(io::Error::other)
    }
}
