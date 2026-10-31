//! Explicit, authenticated reverse-proxy access. The listener remains loopback-only.
use anyhow::{Context, Result, ensure};
use axum::http::{HeaderMap, Method};
use std::{os::unix::fs::PermissionsExt, path::Path};

#[derive(Clone, Default)]
pub struct WebAccess {
    public: Option<PublicAccess>,
}

#[derive(Clone)]
struct PublicAccess {
    origin: String,
    authority: String,
    proxy_token: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Local,
    Https,
}

impl WebAccess {
    pub fn from_env() -> Result<Self> {
        let origin = match std::env::var("GUARD_PUBLIC_ORIGIN") {
            Ok(origin) => Some(origin),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => return Err(error).context("Invalid GUARD_PUBLIC_ORIGIN"),
        };
        let token_file = std::env::var_os("GUARD_PROXY_TOKEN_FILE");
        match (origin, token_file) {
            (None, None) => Ok(Self::default()),
            (Some(origin), Some(path)) => Self::from_file(&origin, Path::new(&path)),
            _ => anyhow::bail!("Set GUARD_PUBLIC_ORIGIN and GUARD_PROXY_TOKEN_FILE together"),
        }
    }

    pub fn from_file(origin: &str, token_file: &Path) -> Result<Self> {
        let metadata =
            std::fs::metadata(token_file).context("Cannot read proxy credential file")?;
        ensure!(
            metadata.is_file(),
            "Proxy credential must be a regular file"
        );
        // systemd credentials may be owner/group readable. No group write/execute
        // or access for other users is accepted, and the file is strictly bounded.
        ensure!(
            metadata.permissions().mode() & 0o037 == 0 && metadata.len() <= 128,
            "Proxy credential file must be private and at most 128 bytes"
        );
        let token = std::fs::read_to_string(token_file).context("Cannot read proxy credential")?;
        Self::new(origin, token.trim_end_matches(['\r', '\n']))
    }

    fn new(origin: &str, token: &str) -> Result<Self> {
        let url = url::Url::parse(origin).context("Invalid public origin")?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.origin().ascii_serialization() == origin,
            "Public origin must be a canonical HTTPS origin without a path or trailing slash"
        );
        ensure!(
            token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
            "Proxy credential must contain 64 random hexadecimal characters"
        );
        Ok(Self {
            public: Some(PublicAccess {
                origin: origin.into(),
                authority: origin["https://".len()..].into(),
                proxy_token: token.into(),
            }),
        })
    }

    pub fn public_origin(&self) -> Option<&str> {
        self.public.as_ref().map(|p| p.origin.as_str())
    }

    pub fn validate(&self, headers: &HeaderMap, method: &Method) -> Result<Mode, &'static str> {
        let host = one_header(headers, "host")?.ok_or("Host header required")?;
        let origin = one_header(headers, "origin")?;
        let proto = one_header(headers, "x-forwarded-proto")?;
        let token = one_header(headers, "x-guard-proxy-token")?;
        let (mode, expected_origin) = if loopback_authority(host) {
            // Proxy proof is only meaningful on the configured public authority.
            // Never let supplied forwarded headers change the local cookie mode.
            if proto.is_some() || token.is_some() {
                return Err("Proxy headers are not allowed on local requests");
            }
            (Mode::Local, format!("http://{host}"))
        } else if let Some(public) = &self.public {
            if host != public.authority {
                return Err("Host validation failed");
            }
            if proto != Some("https")
                || !crate::auth::constant(token.unwrap_or(""), &public.proxy_token)
            {
                return Err("Trusted HTTPS proxy required");
            }
            (Mode::Https, public.origin.clone())
        } else {
            return Err("Host validation failed");
        };
        if origin.is_some_and(|o| o != expected_origin) {
            return Err("Origin validation failed");
        }
        if !matches!(*method, Method::GET | Method::HEAD) && origin.is_none() {
            return Err("Origin header required");
        }
        Ok(mode)
    }
}

fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, &'static str> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err("Duplicate security header");
    }
    first
        .map(|v| v.to_str().map_err(|_| "Invalid security header"))
        .transpose()
}

// SSH forwarding may use a different local port, but no non-loopback authority.
fn loopback_authority(host: &str) -> bool {
    let Some((name, port)) = host.rsplit_once(':') else {
        return false;
    };
    ["127.0.0.1", "localhost", "[::1]"].contains(&name)
        && !port.is_empty()
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|number| number > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    const TOKEN: &str = "0707070707070707070707070707070707070707070707070707070707070707";
    fn public_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        for (name, value) in [
            ("host", "panel.example.com"),
            ("origin", "https://panel.example.com"),
            ("x-forwarded-proto", "https"),
            ("x-guard-proxy-token", TOKEN),
        ] {
            h.insert(name, HeaderValue::from_static(value));
        }
        h
    }
    #[test]
    fn exact_https_proxy_proof_and_origin_are_required() {
        let access = WebAccess::new("https://panel.example.com", TOKEN).unwrap();
        assert_eq!(
            access.validate(&public_headers(), &Method::POST),
            Ok(Mode::Https)
        );
        for name in ["host", "origin", "x-forwarded-proto", "x-guard-proxy-token"] {
            let mut h = public_headers();
            h.remove(name);
            assert!(
                access.validate(&h, &Method::POST).is_err(),
                "missing {name}"
            );
            let mut h = public_headers();
            h.insert(name, HeaderValue::from_static("wrong"));
            assert!(access.validate(&h, &Method::POST).is_err(), "wrong {name}");
            let mut h = public_headers();
            let value = h[name].clone();
            h.append(name, value);
            assert!(
                access.validate(&h, &Method::POST).is_err(),
                "duplicate {name}"
            );
        }
        let mut h = public_headers();
        h.remove("origin");
        assert_eq!(access.validate(&h, &Method::GET), Ok(Mode::Https));
        assert!(
            WebAccess::default()
                .validate(&public_headers(), &Method::GET)
                .is_err()
        );
    }
    #[test]
    fn untrusted_forwarded_values_cannot_promote_local_mode() {
        let access = WebAccess::new("https://panel.example.com", TOKEN).unwrap();
        let mut h = HeaderMap::new();
        h.insert("host", HeaderValue::from_static("127.0.0.1:14877"));
        h.insert("origin", HeaderValue::from_static("http://127.0.0.1:14877"));
        assert_eq!(access.validate(&h, &Method::POST), Ok(Mode::Local));
        h.insert(
            "x-forwarded-host",
            HeaderValue::from_static("panel.example.com"),
        );
        assert_eq!(access.validate(&h, &Method::POST), Ok(Mode::Local));
        h.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(access.validate(&h, &Method::POST).is_err());
    }
    #[test]
    fn public_origin_is_strict_and_token_is_bounded() {
        for origin in [
            "http://panel.example.com",
            "https://panel.example.com/",
            "https://panel.example.com/admin",
            "https://u:p@panel.example.com",
            "https://panel.example.com?x=1",
            "https://panel.example.com#x",
            "https://panel.example.com:443",
            "https://PANEL.example.com",
        ] {
            assert!(WebAccess::new(origin, TOKEN).is_err(), "{origin}");
        }
        assert!(WebAccess::new("https://panel.example.com:8443", TOKEN).is_ok());
        assert!(WebAccess::new("https://panel.example.com", "short").is_err());
    }
    #[test]
    fn proxy_credential_permissions_are_checked() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("token");
        std::fs::write(&p, format!("{TOKEN}\n")).unwrap();
        for mode in [0o400, 0o440, 0o600] {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(WebAccess::from_file("https://panel.example.com", &p).is_ok());
        }
        for mode in [0o644, 0o660, 0o604] {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            assert!(WebAccess::from_file("https://panel.example.com", &p).is_err());
        }
    }
    #[test]
    fn ssh_forwarded_ports_keep_loopback_authorities_only() {
        for host in [
            "127.0.0.1:4877",
            "127.0.0.1:14877",
            "localhost:22222",
            "[::1]:14877",
        ] {
            assert!(loopback_authority(host), "{host}");
        }
        for host in [
            "example.com:4877",
            "localhost.attacker.com:14877",
            "user@localhost:14877",
            "127.0.0.1:0",
            "127.0.0.1:65536",
            "127.0.0.1:abc",
            "127.0.0.1",
            "::1:14877",
            "127.0.0.1:80/path",
            "127.0.0.1:80:90",
        ] {
            assert!(!loopback_authority(host), "{host}");
        }
    }
}
