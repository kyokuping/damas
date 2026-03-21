use crate::error::DamasError;
use rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::path::Path;
use std::sync::Arc;

pub fn validate_server_config(
    cert_path: &Path,
    key_path: &Path,
) -> Result<Arc<rustls::ServerConfig>, DamasError> {
    let cert_der = CertificateDer::from_pem_file(cert_path)
        .map_err(|e| DamasError::ConfigError(e.to_string()))?;
    let key_der = PrivateKeyDer::from_pem_file(key_path)
        .map_err(|e| DamasError::ConfigError(e.to_string()))?;
    Ok(Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .map_err(|e| DamasError::ConfigError(e.to_string()))?,
    ))
}
