//! The one rule for an `openai-compatible` Account's `base_url` (story 2.3,
//! AD-12, AD-48): the loader applies it to this Mac's user Accounts file (`accounts.yaml`) and
//! `agentic accounts` applies it to `--base-url`, so the two never disagree
//! about what a compatible endpoint is.
//!
//! Two forms are accepted, and nothing else:
//!
//! - `https://<host>[/path]` on port 443, where the host is a DNS name;
//! - `http://127.0.0.1:<port>[/path]` with an explicit port, for a server
//!   on this Mac.
//!
//! An IP literal (in any spelling), `localhost`, https to this Mac, another
//! https port, a user name or password, a query and a fragment are each
//! refused in words. No refusal repeats the URL, so a key someone put in
//! one is never printed.

use std::net::{IpAddr, Ipv4Addr};

/// The two accepted forms, as a refusal or a fix names them.
pub const COMPATIBLE_BASE_URL_FORMS: &str = "`https://<host>[/path]` (a DNS name, port 443), or `http://127.0.0.1:<port>[/path]` for a server on this Mac";

/// How a server on this Mac is named.
const ON_THIS_MAC: &str = "a server on this Mac is named only as `http://127.0.0.1:<port>`";

/// What an accepted compatible `base_url` pins (AD-48): scheme, host, port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibleBaseUrl {
    /// `https`, or `http` for an endpoint on this Mac.
    pub scheme: String,
    /// The DNS name in lowercase, or `127.0.0.1`.
    pub host: String,
    /// 443 for https; the explicit port of an endpoint on this Mac.
    pub port: u16,
    /// True for `http://127.0.0.1:<port>`: the calls stay on this Mac.
    pub loopback: bool,
}

/// Reads `text` as an `openai-compatible` Account's `base_url`: only the
/// two accepted forms pass, and everything else gives its reason in words
/// (never repeating `text`).
pub fn compatible_base_url(text: &str) -> Result<CompatibleBaseUrl, String> {
    let forms = COMPATIBLE_BASE_URL_FORMS;
    if text.chars().any(|c| !c.is_ascii_graphic() || c == '\\') {
        return Err(format!(
            "it holds a space, a backslash or a character outside plain ASCII; give {forms}"
        ));
    }
    let (https, rest) = if let Some(rest) = text.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix("http://") {
        (false, rest)
    } else {
        return Err(format!(
            "it does not start with `https://` (or `http://127.0.0.1:<port>`); give {forms}"
        ));
    };
    if rest.contains('#') {
        return Err("it has a fragment (`#…`), which an endpoint cannot have".into());
    }
    if rest.contains('?') {
        return Err("it has a query (`?…`), which an endpoint cannot have".into());
    }
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.contains('@') {
        return Err(
            "it holds a user name or password; a key is never part of the URL (it is read into the Keychain)"
                .into(),
        );
    }
    if authority.is_empty() {
        return Err(format!("it names no host; give {forms}"));
    }
    // The host and the optional port. A bracketed host is an IPv6 address.
    let bracketed = authority.starts_with('[');
    let (host, port) = match authority.strip_prefix('[') {
        Some(inside) => match inside.split_once(']') {
            Some((address, "")) => (address, None),
            Some((address, port)) => (address, Some(port.strip_prefix(':').unwrap_or(""))),
            None => (inside, None),
        },
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let port = match port {
        None => None,
        Some(digits) => match digits.parse::<u16>() {
            Ok(port)
                if port > 0 && digits.len() <= 5 && digits.bytes().all(|b| b.is_ascii_digit()) =>
            {
                Some(port)
            }
            _ => {
                return Err("its port is not a number from 1 to 65535".into());
            }
        },
    };
    let host = host.to_ascii_lowercase();
    let address = host.parse::<IpAddr>().ok();
    let is_address = address.is_some() || bracketed;
    let this_mac = host == "localhost"
        || host.ends_with(".localhost")
        || address.is_some_and(|ip| ip.is_loopback());

    let accepted = if https {
        if this_mac {
            return Err(format!(
                "https to this Mac is not accepted: {ON_THIS_MAC}, not as an https address"
            ));
        }
        if is_address {
            return Err(format!(
                "its host '{host}' is an IP address; an https endpoint is named by its DNS name, and a private, link-local, LAN or metadata address is never called"
            ));
        }
        if !is_dns_name(&host) {
            return Err(format!(
                "its host '{host}' is not a DNS name (letters, digits and hyphens in dot-separated labels, never an address)"
            ));
        }
        if let Some(port) = port
            && port != 443
        {
            return Err(format!(
                "its port is {port}; an https endpoint is called on port 443 only (leave the port out)"
            ));
        }
        CompatibleBaseUrl {
            scheme: "https".into(),
            host,
            port: 443,
            loopback: false,
        }
    } else {
        if host != "127.0.0.1" {
            return Err(if this_mac {
                format!("{ON_THIS_MAC}, not as '{host}'")
            } else if is_address {
                format!(
                    "plain http is accepted only for the address 127.0.0.1 ({ON_THIS_MAC}); '{host}' is another address, and a private, link-local, LAN or metadata address is never called"
                )
            } else {
                "plain http is accepted only for a server on this Mac (`http://127.0.0.1:<port>`); a remote endpoint must be `https://<host>[/path]`"
                    .to_string()
            });
        }
        let Some(port) = port else {
            return Err(
                "an endpoint on this Mac needs an explicit port: `http://127.0.0.1:<port>[/path]`"
                    .into(),
            );
        };
        CompatibleBaseUrl {
            scheme: "http".into(),
            host,
            port,
            loopback: true,
        }
    };
    // A URL parser must read the same endpoint in it, or it is not one.
    if !parses_as(text, &accepted) {
        return Err(format!(
            "a URL parser reads another scheme, host or port in it than the one written; give {forms}"
        ));
    }
    Ok(accepted)
}

/// True when the `url` crate reads `text` as exactly `accepted`: the same
/// scheme, host and port, with no user name, password, query or fragment.
fn parses_as(text: &str, accepted: &CompatibleBaseUrl) -> bool {
    let Ok(parsed) = url::Url::parse(text) else {
        return false;
    };
    let host = match parsed.host() {
        Some(url::Host::Domain(name)) => !accepted.loopback && name == accepted.host,
        Some(url::Host::Ipv4(ip)) => accepted.loopback && ip == Ipv4Addr::LOCALHOST,
        _ => false,
    };
    host && parsed.scheme() == accepted.scheme
        && parsed.port_or_known_default() == Some(accepted.port)
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

/// True for a DNS name: dot-separated labels of letters, digits and inner
/// hyphens, whose last label starts with a letter (so no spelling of an
/// IPv4 address, decimal or hexadecimal, passes as a name).
fn is_dns_name(host: &str) -> bool {
    let label = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    host.len() <= 253
        && host.split('.').all(label)
        && host
            .rsplit('.')
            .next()
            .is_some_and(|last| last.starts_with(|c: char| c.is_ascii_lowercase()))
}
