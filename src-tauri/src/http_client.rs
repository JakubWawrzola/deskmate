//! The HTTP agent behind every plain HTTPS request the app makes: the Home
//! Assistant REST channel, notification images and the release check.

use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// An agent on native-tls (Schannel on Windows, the system's OpenSSL on
/// Linux) that trusts the system's root certificates. ureq 3 otherwise uses
/// rustls with Mozilla's roots: rustls pulls in a C toolchain that breaks the
/// ARM64 build, and a Home Assistant behind the user's own certificate
/// authority would stop working.
pub fn agent(timeout: Duration, max_redirects: u32) -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(timeout))
        .max_redirects(max_redirects)
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_uses_native_tls_and_the_system_roots() {
        let agent = agent(Duration::from_secs(3), 0);
        let config = agent.config();

        assert_eq!(config.tls_config().provider(), TlsProvider::NativeTls);
        assert!(matches!(
            config.tls_config().root_certs(),
            RootCerts::PlatformVerifier
        ));
        assert_eq!(config.max_redirects(), 0);
        assert_eq!(config.timeouts().global, Some(Duration::from_secs(3)));
    }
}
