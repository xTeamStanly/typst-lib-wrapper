//! Provides a way to [create a http agent](create_http_agent) and
//! [download typst packages from the repository](prepare_package).
//!
//! ### Used internally.

use std::io::Read;
use std::path::{Path, PathBuf};

use typst::diag::{eco_format, PackageError, PackageResult};
use typst_syntax::package::PackageSpec;

/// `typst-lib-wrapper` user agent, used when downloading a package.
const USER_AGENT: &str = concat!("typst-lib-wrapper/", env!("CARGO_PKG_VERSION"));

/// Typst package repository location.
const HOST: &str = "https://packages.typst.org";

/// Creates HTTP `ureq::Agent`.
pub(crate) fn create_http_agent(
    agent: Option<ureq::Agent>
) -> ureq::Agent {
    // Returns provided agent.
    if let Some(http_agent) = agent {
        return http_agent;
    } else {
        // Creates new agent.
        let mut builder = ureq::Agent::config_builder();

        // Set user agent.
        builder = builder.user_agent(USER_AGENT);

        return builder.build().new_agent();
    }
}

/// Tries to resolve package specification (`spec`) to [PathBuf].
///
/// If the package is not available locally then it'll try to download it from the repository
/// using `http_client`. It makes packages available in the on-disk cache.
pub(crate) fn prepare_package(
    spec: &PackageSpec,
    http_client: &ureq::Agent
) -> PackageResult<PathBuf> {
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

/// Downloads a typst package with specification `spec` from the repository using `http_client`,
/// decompresses and saves it to the `package_dir`.
fn download_package(
    spec: &PackageSpec,
    package_dir: &Path,
    http_client: &ureq::Agent
) -> PackageResult<()> {

    // Build url and send request.
    let url = format!("{HOST}/preview/{}-{}.tar.gz", spec.name, spec.version);
    let response = match http_client.get(&url).call() {
        Ok(resp) => resp,
        Err(ureq::Error::StatusCode(404)) =>
            return Err(PackageError::NotFound(spec.clone())),
        Err(err) => {
            let message = eco_format!("{err}");
            return Err(PackageError::NetworkFailed(Some(message)));
        }
    };

    // Try to get buffer size from `Content-Length` header.
    // If not present/error use zero. `Vec::with_capacity` can handle zero.
    let content_length: usize = match response.headers().get("Content-Length") {
        None => 0,
        Some(header) => {
            match header.to_str() {
                Ok(header_str) => {
                    header_str.parse::<usize>().unwrap_or(0)
                },
                Err(_) => 0
            }
        }
    };
    let mut buffer: Vec<u8> = Vec::with_capacity(content_length);

    // Try to read HTTP response to buffer and decompress it.
    response.into_body().as_reader().read_to_end(&mut buffer)
        .map_err(|err| PackageError::NetworkFailed(Some(eco_format!("{err}"))))?;

    let decompressed = flate2::read::GzDecoder::new(buffer.as_slice());

    tar::Archive::new(decompressed).unpack(package_dir)
        .map_err(|err| {
            std::fs::remove_dir_all(package_dir).ok(); // Delete malformed archive.
            PackageError::MalformedArchive(Some(eco_format!("{err}")))
        })?;

    return Ok(());
}
