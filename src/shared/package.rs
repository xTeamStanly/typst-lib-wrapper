use std::sync::Arc;

use native_tls::{Certificate, TlsConnector};
use parking_lot::{const_mutex, Mutex};

/// `typst-lib-wrapper` user agent
const USER_AGENT: &str = concat!("typst-lib-wrapper/", env!("CARGO_PKG_VERSION"));

#[allow(clippy::result_large_err)]
pub fn create_http_agent(certificate: Option<Certificate>) -> Result<ureq::Agent, ureq::Error> {
    let mut builder = ureq::AgentBuilder::new();
    let mut tls = TlsConnector::builder();

    // Set user agent.
    builder = builder.user_agent(USER_AGENT);

    // Apply a custom CA certificate if present.
    if let Some(certificate) = certificate {
        tls.add_root_certificate(certificate);
    }

    // Configure native TLS.
    let connector = tls
        .build()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    builder = builder.tls_connector(Arc::new(connector));

    return Ok(builder.build());
}