//! Ed25519 signatures on update metadata (audit N1).
//!
//! The SHA-256 sums an update is checked against come from the same GitHub
//! release as the update, so anyone who can publish a release could ship any
//! binary. With a public key built in below, Bridge also requires a signature
//! over that metadata (`latest.json.sig`, `<archive>.sha256.sig`) made with the
//! matching private key, which release.yml holds as `UPDATE_SIGNING_KEY`.
//! A missing or bad signature refuses the update.
//!
//! The key starts empty: upstream's releases are unsigned, and with no key
//! Bridge updates exactly as before.

use anyhow::{Context, Result, bail, ensure};

/// Hex ed25519 public key (64 hex digits) that update metadata must be signed
/// with; empty to skip the check. release.yml refuses to publish a release
/// when this is set but `UPDATE_SIGNING_KEY` is missing or does not match.
pub(super) const UPDATE_PUBLIC_KEY_HEX: &str = "";

/// Appended to a metadata file's URL or asset name to find its signature.
pub(super) const SIGNATURE_SUFFIX: &str = ".sig";

/// First line of what a `.sha256.sig` signs. The tag on the next line binds
/// the checksum to its release, so an older release's signed checksum cannot
/// be replayed under a newer tag (downgrade).
const CHECKSUM_CONTEXT: &str = "openmouse-bridge checksum v1";

/// The bytes a `<archive>.sha256.sig` signs: context line, release tag, then
/// the checksum file as published. release.yml builds the same bytes.
pub(super) fn checksum_message(tag: &str, checksum_file: &[u8]) -> Vec<u8> {
    let mut message = format!("{CHECKSUM_CONTEXT}\n{tag}\n").into_bytes();
    message.extend_from_slice(checksum_file);
    message
}

/// Largest signature file read; a raw ed25519 signature is 64 bytes.
pub(super) const MAX_SIGNATURE_BYTES: usize = 1024;

/// The built-in key, `None` when signatures are not checked. A malformed key
/// is an error, so a bad edit fails closed instead of turning checks off.
pub(super) fn configured_key() -> Result<Option<[u8; 32]>> {
    parse_key(UPDATE_PUBLIC_KEY_HEX)
}

fn parse_key(hex: &str) -> Result<Option<[u8; 32]>> {
    if hex.is_empty() {
        return Ok(None);
    }
    ensure!(
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "the built-in update signing key is not 64 hex digits"
    );
    let mut key = [0u8; 32];
    for (out, pair) in key.iter_mut().zip(hex.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).context("update signing key is not ASCII")?;
        *out = u8::from_str_radix(pair, 16).context("update signing key is not hex")?;
    }
    Ok(Some(key))
}

/// Checks a raw 64-byte ed25519 `signature` over `message`.
pub(super) fn verify(key: &[u8; 32], message: &[u8], signature: &[u8], what: &str) -> Result<()> {
    if signature.len() != 64 {
        bail!(
            "the signature on {what} is {} bytes, not 64; refusing the update",
            signature.len()
        );
    }
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(message, signature)
        .map_err(|_| anyhow::anyhow!("the signature on {what} does not match; refusing the update"))
}

#[cfg(test)]
mod tests {
    use ring::signature::{Ed25519KeyPair, KeyPair};

    use super::*;

    fn key_pair() -> Ed25519KeyPair {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    fn public(pair: &Ed25519KeyPair) -> [u8; 32] {
        pair.public_key().as_ref().try_into().unwrap()
    }

    #[test]
    fn the_built_in_key_is_empty_or_valid() {
        configured_key().expect("UPDATE_PUBLIC_KEY_HEX must be empty or 64 hex digits");
    }

    #[test]
    fn checksum_signatures_are_bound_to_their_tag() {
        let pair = key_pair();
        let key = public(&pair);
        let file = b"abc  openmouse-bridge-linux-x64-full.zip\n";
        let signature = pair.sign(&checksum_message("v1.0.0", file));
        verify(
            &key,
            &checksum_message("v1.0.0", file),
            signature.as_ref(),
            "x",
        )
        .unwrap();
        // The same signed checksum replayed under a newer tag is refused.
        assert!(
            verify(
                &key,
                &checksum_message("v1.0.1", file),
                signature.as_ref(),
                "x"
            )
            .is_err()
        );
        assert_eq!(
            checksum_message("v1.0.0", b"abc"),
            b"openmouse-bridge checksum v1\nv1.0.0\nabc".to_vec()
        );
    }

    #[test]
    fn keys_parse_from_hex() {
        assert_eq!(parse_key("").unwrap(), None);
        let hex = "00ff".repeat(16);
        let key = parse_key(&hex).unwrap().unwrap();
        assert_eq!(key[..2], [0x00, 0xff]);
        assert!(parse_key("abc").is_err());
        assert!(parse_key(&"zz".repeat(32)).is_err());
        assert!(parse_key(&"0".repeat(66)).is_err());
    }

    #[test]
    fn only_a_matching_signature_passes() {
        let pair = key_pair();
        let key = public(&pair);
        let message = br#"{"version":"9.9.9"}"#;
        let signature = pair.sign(message);
        verify(&key, message, signature.as_ref(), "latest.json").unwrap();

        // Changed message, other key, truncated or missing signature.
        assert!(verify(&key, br#"{"version":"9.9.8"}"#, signature.as_ref(), "x").is_err());
        let other = public(&key_pair());
        assert!(verify(&other, message, signature.as_ref(), "x").is_err());
        assert!(verify(&key, message, &signature.as_ref()[..63], "x").is_err());
        assert!(verify(&key, message, b"", "x").is_err());
    }

    /// The format release.yml produces: `openssl pkeyutl -sign -rawin` with an
    /// ed25519 PEM key writes the raw 64-byte signature.
    #[test]
    fn openssl_raw_signatures_verify() {
        // Made once with a throwaway key (OpenSSL 3.0):
        // openssl genpkey -algorithm ed25519 -out k.pem
        // openssl pkeyutl -sign -rawin -inkey k.pem -in m -out m.sig
        // openssl pkey -in k.pem -pubout -outform DER | tail -c 32
        let key = parse_key("8412b75b5599775d1b5b0256399e9c288edf6bd1248e71f3a1f11a8e175d02e3")
            .unwrap()
            .unwrap();
        let message = b"{\"version\":\"9.9.9\"}\n";
        let signature = hex(concat!(
            "3a431b059430eedbb1d529e2d5ab5957349a365594e3f610dda8159431c80013",
            "deb2ee72d67b2d787fb808b5e12d1b301374f687c13939e33844399fb4f17809",
        ));
        verify(&key, message, &signature, "latest.json").unwrap();
        assert!(verify(&key, b"{\"version\":\"9.9.9\"}", &signature, "latest.json").is_err());
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).unwrap())
            .collect()
    }
}
