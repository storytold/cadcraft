//! Who a connection is, and how the server learns it.
//!
//! The token comes in the first message, never in the URL or a log (ADR-0008). A verifier turns it
//! into claims: the participant, the drawing and the role. Roles are checked on every operation,
//! because the engine has no read-only mode.

use cadcraft_collab::Role;

/// What a verified token says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claims {
    /// The participant (the token's `sub`).
    pub sub: String,
    /// The drawing the token is for.
    pub doc: String,
    /// What the participant may do.
    pub role: Role,
}

/// Verifies a session token.
pub trait Authenticator: Send + Sync {
    /// The claims of `jwt`, or why it is not acceptable.
    fn verify(&self, jwt: &str) -> Result<Claims, String>;
}

/// A verifier with a fixed token table, for tests and for a single-tenant deployment.
#[derive(Clone, Debug, Default)]
pub struct StaticAuthenticator {
    entries: Vec<(String, Claims)>,
}

impl StaticAuthenticator {
    /// A table of `(token, claims)`.
    pub fn new(entries: impl IntoIterator<Item = (String, Claims)>) -> Self {
        StaticAuthenticator { entries: entries.into_iter().collect() }
    }
}

impl Authenticator for StaticAuthenticator {
    fn verify(&self, jwt: &str) -> Result<Claims, String> {
        self.entries.iter().find(|(token, _)| token == jwt).map(|(_, claims)| claims.clone()).ok_or_else(|| "unknown token".into())
    }
}

/// What a signed token carries.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct TokenClaims {
    sub: String,
    doc: String,
    role: Role,
    exp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
}

/// A verifier of HS256 tokens signed with the cabinet's session secret.
#[derive(Clone, Debug)]
pub struct JwtAuthenticator {
    secret: Vec<u8>,
    issuer: Option<String>,
    audience: Option<String>,
}

impl JwtAuthenticator {
    /// A verifier for tokens signed with `secret`.
    pub fn hs256(secret: impl Into<Vec<u8>>) -> Self {
        JwtAuthenticator { secret: secret.into(), issuer: None, audience: None }
    }

    /// Require the `iss` claim.
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Require the `aud` claim.
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }
}

impl Authenticator for JwtAuthenticator {
    fn verify(&self, jwt: &str) -> Result<Claims, String> {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.set_required_spec_claims(&["exp", "sub"]);
        if let Some(issuer) = &self.issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(audience) = &self.audience {
            validation.set_audience(&[audience]);
        }
        let token = jsonwebtoken::decode::<TokenClaims>(jwt, &jsonwebtoken::DecodingKey::from_secret(&self.secret), &validation)
            .map_err(|e| e.to_string())?
            .claims;
        Ok(Claims { sub: token.sub, doc: token.doc, role: token.role })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header};

    fn token(secret: &[u8], claims: &TokenClaims) -> String {
        jsonwebtoken::encode(&Header::new(jsonwebtoken::Algorithm::HS256), claims, &EncodingKey::from_secret(secret)).unwrap()
    }

    fn claims() -> TokenClaims {
        TokenClaims { sub: "u1".into(), doc: "file-1".into(), role: Role::Editor, exp: u64::MAX, iss: None, aud: None }
    }

    #[test]
    fn a_static_table_verifies_known_tokens_only() {
        let auth = StaticAuthenticator::new([("a".to_string(), Claims { sub: "u1".into(), doc: "d".into(), role: Role::Editor })]);
        assert_eq!(auth.verify("a").unwrap().sub, "u1");
        assert!(auth.verify("b").is_err());
    }

    #[test]
    fn an_hs256_token_is_verified() {
        let secret = b"secret";
        let auth = JwtAuthenticator::hs256(secret);
        assert_eq!(auth.verify(&token(secret, &claims())).unwrap(), Claims { sub: "u1".into(), doc: "file-1".into(), role: Role::Editor });
    }

    #[test]
    fn another_secret_is_refused() {
        let auth = JwtAuthenticator::hs256(b"secret");
        assert!(auth.verify(&token(b"other", &claims())).is_err());
    }

    #[test]
    fn the_issuer_and_audience_are_checked() {
        let secret = b"secret";
        let mut claims = claims();
        claims.iss = Some("cabinet".into());
        claims.aud = Some("office-sync".into());
        let auth = JwtAuthenticator::hs256(secret).with_issuer("cabinet").with_audience("office-sync");
        assert!(auth.verify(&token(secret, &claims)).is_ok());
        let wrong = JwtAuthenticator::hs256(secret).with_issuer("someone-else");
        assert!(wrong.verify(&token(secret, &claims)).is_err());
    }
}
