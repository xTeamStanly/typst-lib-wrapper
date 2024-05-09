use std::{fs, path::{Path, PathBuf}, sync::Arc};

use native_tls::{Certificate, TlsConnector};
use parking_lot::{const_mutex, Mutex};
use typst::diag::{eco_format, FileResult, PackageError, PackageResult};
use typst_syntax::package::PackageSpec;

use crate::errors::{WrapperError, WrapperResult};

/// `typst-lib-wrapper` user agent
const USER_AGENT: &str = concat!("typst-lib-wrapper/", env!("CARGO_PKG_VERSION"));

/// Typst package repository location.
const HOST: &str = "https://packages.typst.org";

pub fn create_http_agent(
    certificate: Option<Certificate>
) -> WrapperResult<ureq::Agent> {
    let mut builder = ureq::AgentBuilder::new();
    let mut tls = TlsConnector::builder();

    // Set user agent.
    builder = builder.user_agent(USER_AGENT);

    // Apply a custom CA certificate if present.
    if let Some(certificate) = certificate {
        tls.add_root_certificate(certificate);
    }

    // Configure native TLS.
    let connector = match tls.build() {
        Ok(conn) => conn,
        Err(err) => {
            let io_err = std::io::Error::new(std::io::ErrorKind::Other, err);
            let ureq_err = ureq::Error::from(io_err);
            return Err(WrapperError::from(ureq_err));
        }
    };
    builder = builder.tls_connector(Arc::new(connector));

    return Ok(builder.build());
}

/// Make a package available in the on-disk cache.
pub fn prepare_package(spec: &PackageSpec, http_client: &ureq::Agent) -> PackageResult<PathBuf> {
    let subdir = format!("typst/packages/{}/{}/{}", spec.namespace, spec.name, spec.version);

    // Check `data_dir` first.
    if let Some(data_dir) = dirs::data_dir() {
        let dir = data_dir.join(&subdir);
        if dir.exists() {
            return Ok(dir);
        }
    }

    // Check `cache_dir` and download package if necessary.
    if let Some(cache_dir) = dirs::cache_dir() {
        let dir = cache_dir.join(&subdir);
        if dir.exists() {
            return Ok(dir);
        }

        // Download from network if it doesn't exist yet.
        // The `@preview` namespace is the only namespace that supports on-demand fetching.
        if spec.namespace == "preview" {
            download_package(spec, &dir, http_client)?;
            if dir.exists() {
                return Ok(dir);
            }
        }
    }

    return Err(PackageError::NotFound(spec.clone()));
}

/// Download a package over the network.
fn download_package(
    spec: &PackageSpec,
    package_dir: &Path,
    http_client: &ureq::Agent
) -> PackageResult<()> {

    // Build url and send request.
    let url = format!("{HOST}/preview/{}-{}.tar.gz", spec.name, spec.version);
    let response: ureq::Response = match http_client.get(&url).call() {
        Ok(resp) => resp,
        Err(ureq::Error::Status(404, _)) =>
            return Err(PackageError::NotFound(spec.clone())),
        Err(err) => {
            let message = eco_format!("{err}");
            return Err(PackageError::NetworkFailed(Some(message)));
        }
    };

    // Try to get buffer size from `Content-Length` header.
    // If not present/error use zero. `Vec::with_capacity` can handle zero.
    let content_length: usize = match response.header("Content-Length") {
        None => 0,
        Some(header) => header.parse::<usize>().unwrap_or(0)
    };
    let mut buffer: Vec<u8> = Vec::with_capacity(content_length);

    // Try to read HTTP response to buffer and decompress it.
    response.into_reader().read_to_end(&mut buffer)
        .map_err(|err| PackageError::NetworkFailed(Some(eco_format!("{err}"))))?;

    let decompressed = flate2::read::GzDecoder::new(buffer.as_slice());
    tar::Archive::new(decompressed).unpack(package_dir)
        .map_err(|err| {
            fs::remove_dir_all(package_dir).ok(); // Delete malformed archive.
            PackageError::MalformedArchive(Some(eco_format!("{err}")))
        })?;

    return Ok(());
}
