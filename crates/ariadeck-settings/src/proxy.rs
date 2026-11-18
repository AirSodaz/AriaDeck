use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::SettingsError;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadProxyMode {
    #[default]
    Disabled,
    /// Resolve static OS / environment proxy at apply time and push to aria2.
    /// PAC / WPAD scripts are not supported; credentials are not auto-filled
    /// from the OS keychain (use Manual for authenticated proxies).
    System,
    Manual,
}

/// Opaque reference to a download-proxy credential entry in the OS keyring.
pub type ProxyCredentialRef = ariadeck_domain::CredentialId;

#[derive(Debug, Error)]
#[error("system credential store operation failed while {operation}: {message}")]
pub struct ProxyCredentialError {
    operation: &'static str,
    message: String,
}

pub trait ProxyCredentialStore: Send + Sync {
    fn load(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<Option<SecretString>, ProxyCredentialError>;
    fn save(
        &self,
        credential: ProxyCredentialRef,
        password: &SecretString,
    ) -> Result<(), ProxyCredentialError>;
    fn delete(&self, credential: ProxyCredentialRef) -> Result<(), ProxyCredentialError>;
}

#[derive(Clone, Debug)]
pub struct SystemProxyCredentialStore {
    service: String,
}

impl Default for SystemProxyCredentialStore {
    fn default() -> Self {
        Self::new("AriaDeck download proxy")
    }
}

impl SystemProxyCredentialStore {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<keyring::Entry, ProxyCredentialError> {
        keyring::Entry::new(&self.service, &credential.as_uuid().to_string()).map_err(|error| {
            ProxyCredentialError {
                operation: "opening an entry",
                message: error.to_string(),
            }
        })
    }
}

impl ProxyCredentialStore for SystemProxyCredentialStore {
    fn load(
        &self,
        credential: ProxyCredentialRef,
    ) -> Result<Option<SecretString>, ProxyCredentialError> {
        match self.entry(credential)?.get_password() {
            Ok(password) => Ok(Some(SecretString::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(ProxyCredentialError {
                operation: "reading an entry",
                message: error.to_string(),
            }),
        }
    }

    fn save(
        &self,
        credential: ProxyCredentialRef,
        password: &SecretString,
    ) -> Result<(), ProxyCredentialError> {
        use secrecy::ExposeSecret as _;

        self.entry(credential)?
            .set_password(password.expose_secret())
            .map_err(|error| ProxyCredentialError {
                operation: "writing an entry",
                message: error.to_string(),
            })
    }

    fn delete(&self, credential: ProxyCredentialRef) -> Result<(), ProxyCredentialError> {
        match self.entry(credential)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(ProxyCredentialError {
                operation: "deleting an entry",
                message: error.to_string(),
            }),
        }
    }
}

pub(crate) fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DownloadProxySettings {
    pub mode: DownloadProxyMode,
    pub all_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
    pub ftp_proxy: Option<String>,
    pub no_proxy: Vec<String>,
    pub username: Option<String>,
    pub credential: Option<ProxyCredentialRef>,
    /// Verify peer TLS certificates (`check-certificate`). Default true.
    /// Disable only when diagnosing proxy/MITM TLS handshake failures.
    #[serde(default = "default_true")]
    pub check_certificate: bool,
}

impl Default for DownloadProxySettings {
    fn default() -> Self {
        Self {
            mode: DownloadProxyMode::default(),
            all_proxy: None,
            http_proxy: None,
            https_proxy: None,
            ftp_proxy: None,
            no_proxy: Vec::new(),
            username: None,
            credential: None,
            check_certificate: true,
        }
    }
}

impl DownloadProxySettings {
    pub fn validate(&self) -> Result<(), SettingsError> {
        for (label, endpoint) in [
            ("all", self.all_proxy.as_deref()),
            ("HTTP", self.http_proxy.as_deref()),
            ("HTTPS", self.https_proxy.as_deref()),
            ("FTP", self.ftp_proxy.as_deref()),
        ] {
            if let Some(endpoint) = endpoint {
                validate_proxy_endpoint(label, endpoint)?;
            }
        }
        if self.mode == DownloadProxyMode::Manual
            && self.all_proxy.is_none()
            && self.http_proxy.is_none()
            && self.https_proxy.is_none()
            && self.ftp_proxy.is_none()
        {
            return Err(SettingsError::MissingManualProxyEndpoint);
        }
        // Credential refs are only meaningful for Manual mode. System/Disabled may
        // retain a stale ref on disk after a mode switch; do not require a username.
        if self.mode == DownloadProxyMode::Manual
            && self.credential.is_some()
            && self.username.as_deref().is_none_or(str::is_empty)
        {
            return Err(SettingsError::ProxyCredentialWithoutUsername);
        }
        for entry in &self.no_proxy {
            validate_no_proxy_entry(entry)?;
        }
        Ok(())
    }
}

fn validate_proxy_endpoint(label: &'static str, endpoint: &str) -> Result<(), SettingsError> {
    if endpoint.is_empty() || endpoint.trim() != endpoint {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "value must be non-empty and must not have surrounding whitespace".into(),
        });
    }
    let candidate = if endpoint.contains("://") {
        endpoint.to_owned()
    } else {
        format!("http://{endpoint}")
    };
    let parsed = Url::parse(&candidate).map_err(|error| SettingsError::InvalidProxyEndpoint {
        label,
        reason: error.to_string(),
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "only HTTP and HTTPS proxy URLs are supported".into(),
        });
    }
    if parsed.host_str().is_none() {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "host is required".into(),
        });
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "credentials must be stored separately from the proxy URL".into(),
        });
    }
    if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(SettingsError::InvalidProxyEndpoint {
            label,
            reason: "path, query, and fragment components are not allowed".into(),
        });
    }
    Ok(())
}

fn validate_no_proxy_entry(entry: &str) -> Result<(), SettingsError> {
    let invalid = |reason: &str| SettingsError::InvalidNoProxyEntry {
        entry: entry.to_owned(),
        reason: reason.into(),
    };
    if entry.is_empty() || entry.trim() != entry {
        return Err(invalid(
            "value must be non-empty and must not have surrounding whitespace",
        ));
    }
    if entry.contains([',', '@']) || entry.contains("://") || entry.chars().any(char::is_whitespace)
    {
        return Err(invalid(
            "expected a host, domain, IP address, or CIDR network",
        ));
    }
    if let Some((address, prefix)) = entry.rsplit_once('/') {
        let address = address
            .parse::<std::net::IpAddr>()
            .map_err(|_| invalid("CIDR base must be an IPv4 or IPv6 address"))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| invalid("CIDR prefix must be a number"))?;
        let max_prefix = if address.is_ipv4() { 32 } else { 128 };
        if prefix > max_prefix {
            return Err(invalid("CIDR prefix exceeds the address width"));
        }
        return Ok(());
    }
    if !entry
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || ".-:[]".contains(character))
    {
        return Err(invalid("contains unsupported characters"));
    }
    Ok(())
}
