//! Security Sandbox and Domain Allowlist for Computer Use & Vision RPA.
//! Prevents unauthorized navigation, data exfiltration, and plain credential leaks.

/// Trusted enterprise domain whitelist.
const TRUSTED_DOMAINS: &[&str] = &[
    "pajak.go.id",
    "kemenkeu.go.id",
    "bpjs-kesehatan.go.id",
    "bpjsketenagakerjaan.go.id",
    "klikbca.com",
    "bankmandiri.co.id",
    "bri.co.id",
    "bni.co.id",
    "archzxy.com",
];

/// Verifies whether an external target URL is permitted within the secure RPA boundary.
/// Enforces strict HTTPS, userinfo rejection, and authoritative host matching.
pub fn is_domain_allowed(url: &str) -> bool {
    let clean = url.trim();

    // 1. Mandatory HTTPS enforcement (no plain HTTP, no file://, no javascript:)
    let Some(after_scheme) = clean.strip_prefix("https://") else {
        return false;
    };

    // 2. Extract authority (everything before path, query, or fragment)
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .trim();

    if authority.is_empty() {
        return false;
    }

    // 3. Userinfo (@) strictly forbidden to prevent Host Confusion / SSRF exploits
    // (e.g., https://pajak.go.id:443@127.0.0.1/evil)
    if authority.contains('@') {
        return false;
    }

    // 4. Reject dangerous characters (whitespace, backslashes, control characters)
    if authority
        .chars()
        .any(|c| c.is_whitespace() || c == '\\' || c.is_control())
    {
        return false;
    }

    // 5. Separate hostname and optional port
    let host = if let Some((h, port_str)) = authority.split_once(':') {
        // Port must be purely numeric if present
        if !port_str.chars().all(|c| c.is_ascii_digit()) || port_str.is_empty() {
            return false;
        }
        h
    } else {
        authority
    };

    let host_lower = host.to_ascii_lowercase();

    // 6. Direct IP addresses are never allowed in enterprise RPA whitelist
    if host_lower.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return false;
    }

    // 7. Authoritative domain match: exact match or subdomain with boundary dot
    TRUSTED_DOMAINS
        .iter()
        .any(|allowed| host_lower == *allowed || host_lower.ends_with(&format!(".{}", allowed)))
}

/// Masks sensitive credentials (passwords, PINs, OTP tokens) from logs and audit trails.
pub fn mask_credential(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    "***[ENCRYPTED_SECRET]***".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_allowlist_enforcement() {
        assert!(is_domain_allowed("https://efaktur.pajak.go.id/login"));
        assert!(is_domain_allowed("https://vpn.klikbca.com/corp"));
        assert!(is_domain_allowed("https://edabu.bpjs-kesehatan.go.id"));
        assert!(is_domain_allowed("https://klikbca.com:8443/login"));

        // Hostile domains blocked
        assert!(!is_domain_allowed("https://evil-phishing.com"));
        assert!(!is_domain_allowed("https://fake-pajak.go.id.attacker.com"));
        assert!(!is_domain_allowed("http://pajak.go.id")); // Plain HTTP blocked
        assert!(!is_domain_allowed("http://untrusted-api.io/leak"));

        // S05 Finding Reproduction: Userinfo host confusion blocked!
        assert!(!is_domain_allowed(
            "https://pajak.go.id:443@127.0.0.1/private"
        ));
        assert!(!is_domain_allowed(
            "https://admin:pass@klikbca.com.evil.com/"
        ));
        assert!(!is_domain_allowed("https://127.0.0.1/"));
    }

    #[test]
    fn test_credential_masking() {
        assert_eq!(
            mask_credential("MySecretPassword123!"),
            "***[ENCRYPTED_SECRET]***"
        );
        assert_eq!(mask_credential(""), "");
    }
}
