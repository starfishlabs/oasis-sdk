use evm::{
    executor::stack::{PrecompileFailure, PrecompileOutput},
    Context, ExitError, ExitSucceed,
};
use hmac::{Hmac, Mac, NewMac as _};
use num::{BigUint, ToPrimitive as _};
use oasis_runtime_sdk::core::common::crypto::mrae::deoxysii::{DeoxysII, KEY_SIZE, NONCE_SIZE};

use super::{linear_cost, PrecompileResult};

/// The base setup cost for encryption and decryption.
const DEOXYSII_BASE_COST: u64 = 50_000;
/// The cost for encryption and decryption per word of input.
const DEOXYSII_WORD_COST: u64 = 100;
/// Length of an EVM word, in bytes.
const WORD: usize = 32;

pub(super) fn call_x25519_derive(
    input: &[u8],
    target_gas: Option<u64>,
    _context: &Context,
    _is_static: bool,
) -> PrecompileResult {
    let gas_cost = linear_cost(target_gas, input.len() as u64, 100_000, 0)?;

    // Input encoding: bytes32 public || bytes32 private.
    let mut public = [0u8; WORD];
    let mut private = [0u8; WORD];
    if input.len() != 2 * WORD {
        return Err(PrecompileFailure::Error {
            exit_status: ExitError::Other("input length must be 64 bytes".into()),
        });
    }
    public.copy_from_slice(&input[0..WORD]);
    private.copy_from_slice(&input[WORD..]);

    let public = x25519_dalek::PublicKey::from(public);
    let private = x25519_dalek::StaticSecret::from(private);

    let mut kdf = Hmac::<sha2::Sha512Trunc256>::new_from_slice(b"MRAE_Box_Deoxys-II-256-128")
        .map_err(|_| PrecompileFailure::Error {
            exit_status: ExitError::Other("unable to create key derivation function".into()),
        })?;
    kdf.update(private.diffie_hellman(&public).as_bytes());

    let mut derived_key = [0u8; KEY_SIZE];
    let digest = kdf.finalize();
    derived_key.copy_from_slice(&digest.into_bytes()[..KEY_SIZE]);

    Ok(PrecompileOutput {
        exit_status: ExitSucceed::Returned,
        cost: gas_cost,
        output: derived_key.to_vec(),
        logs: Default::default(),
    })
}

#[allow(clippy::type_complexity)]
fn decode_deoxysii_call_args(
    input: &[u8],
) -> Result<([u8; KEY_SIZE], [u8; NONCE_SIZE], &[u8], &[u8]), PrecompileFailure> {
    // Number of fixed words in the input (key, nonce word, two lengths; see
    // comments in the precompiles).
    const SLOTS: usize = 4;

    if input.len() < SLOTS * WORD {
        return Err(PrecompileFailure::Error {
            exit_status: ExitError::Other("input length must be at least 128 bytes".into()),
        });
    }

    let mut words = input.array_chunks::<WORD>().take(SLOTS);
    let key = words.next().unwrap();
    let nonce_word = words.next().unwrap();
    let text_len = words.next().unwrap();
    let ad_len = words.next().unwrap();

    // Only the initial NONCE_SIZE bytes of the nonce field are used - bytes at
    // lower addresses in the input.
    let mut nonce = [0u8; NONCE_SIZE];
    nonce.copy_from_slice(&nonce_word[..NONCE_SIZE]);

    let text_len_big = BigUint::from_bytes_be(text_len);
    let text_len = text_len_big
        .to_usize()
        .ok_or_else(|| PrecompileFailure::Error {
            exit_status: ExitError::Other("text length out of bounds".into()),
        })?;
    let text_size = text_len.saturating_add(31) & (!0x1f); // Round up to 32 bytes.

    let ad_len_big = BigUint::from_bytes_be(ad_len);
    let ad_len = ad_len_big
        .to_usize()
        .ok_or_else(|| PrecompileFailure::Error {
            exit_status: ExitError::Other("additional data length out of bounds".into()),
        })?;
    let ad_size = ad_len.saturating_add(31) & (!0x1f); // Round up to 32 bytes.
    if input.len() != SLOTS * WORD + ad_size + text_size {
        return Err(PrecompileFailure::Error {
            exit_status: ExitError::Other("input too short".into()),
        });
    }

    let text = &input[(SLOTS * WORD)..(SLOTS * WORD + text_len)];
    let ad = &input[(SLOTS * WORD + text_size)..(SLOTS * WORD + text_size + ad_len)];

    Ok((*key, nonce, text, ad))
}

pub(super) fn call_deoxysii_seal(
    input: &[u8],
    target_gas: Option<u64>,
    _context: &Context,
    _is_static: bool,
) -> PrecompileResult {
    let gas_cost = linear_cost(
        target_gas,
        input.len() as u64,
        DEOXYSII_BASE_COST,
        DEOXYSII_WORD_COST,
    )?;

    // Input encoding: bytes32 key || bytes32 nonce || uint plaintext_len || uint ad_len || plaintext || ad.
    let (key, nonce, text, ad) = decode_deoxysii_call_args(input)?;

    let deoxysii = DeoxysII::new(&key);
    let encrypted = deoxysii.seal(&nonce, text, ad);

    Ok(PrecompileOutput {
        exit_status: ExitSucceed::Returned,
        cost: gas_cost,
        output: encrypted,
        logs: Default::default(),
    })
}

pub(super) fn call_deoxysii_open(
    input: &[u8],
    target_gas: Option<u64>,
    _context: &Context,
    _is_static: bool,
) -> PrecompileResult {
    let gas_cost = linear_cost(
        target_gas,
        input.len() as u64,
        DEOXYSII_BASE_COST,
        DEOXYSII_WORD_COST,
    )?;

    // Input encoding: bytes32 key || bytes32 nonce || uint ciphertext_len || uint ad_len || ciphertext || ad.
    let (key, nonce, ciphertext, ad) = decode_deoxysii_call_args(input)?;
    let ciphertext = ciphertext.to_vec();

    let deoxysii = DeoxysII::new(&key);
    match deoxysii.open(&nonce, ciphertext, ad) {
        Ok(decrypted) => Ok(PrecompileOutput {
            exit_status: ExitSucceed::Returned,
            cost: gas_cost,
            output: decrypted,
            logs: Default::default(),
        }),
        Err(_) => Err(PrecompileFailure::Error {
            exit_status: ExitError::Other("decryption error".into()),
        }),
    }
}

#[cfg(test)]
mod test {
    extern crate test;

    use rand::rngs::OsRng;
    use test::Bencher;

    use super::super::test::*;

    #[test]
    fn test_x25519_derive() {
        let mut rng = OsRng {};
        let static_secret = x25519_dalek::StaticSecret::new(&mut rng);
        let public = x25519_dalek::PublicKey::from(&static_secret);

        let mut blob = [0u8; 64];
        blob[..32].copy_from_slice(public.as_bytes());
        blob[32..].copy_from_slice(&static_secret.to_bytes());

        // Normal try.
        call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
            ]),
            &blob,
            1_000_000,
        )
        .expect("call should return something")
        .expect("call should succeed");

        // Not enough gas.
        call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
            ]),
            &blob,
            10_000,
        )
        .expect("call should return something")
        .expect_err("call should fail");

        // Test with known values.
        blob[..32].copy_from_slice(
            &<[u8; 32] as hex::FromHex>::from_hex(
                "3046db3fa70ce605457dc47c48837ebd8bd0a26abfde5994d033e1ced68e2576",
            )
            .unwrap(),
        );
        blob[32..].copy_from_slice(
            &<[u8; 32] as hex::FromHex>::from_hex(
                "c07b151fbc1e7a11dff926111188f8d872f62eba0396da97c0a24adb75161750",
            )
            .unwrap(),
        );
        let output = call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
            ]),
            &blob,
            1_000_000,
        )
        .expect("call should return something")
        .expect("call should succeed")
        .output;
        assert_eq!(
            hex::encode(&output),
            "e69ac21066a8c2284e8fdc690e579af4513547b9b31dd144792c1904b45cf586"
        );
    }

    #[bench]
    fn bench_x25519_derive(b: &mut Bencher) {
        let mut rng = OsRng {};
        let static_secret = x25519_dalek::StaticSecret::new(&mut rng);
        let public = x25519_dalek::PublicKey::from(&static_secret);

        let mut blob = [0u8; 64];
        blob[..32].copy_from_slice(public.as_bytes());
        blob[32..].copy_from_slice(&static_secret.to_bytes());

        b.iter(|| {
            call_contract(
                H160([
                    0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
                ]),
                &blob,
                1_000_000,
            )
            .expect("call should return something")
            .expect("call should succeed");
        });
    }

    fn get_usize_bytes(u: usize) -> [u8; 32] {
        let short = u.to_be_bytes();
        let mut long = [0u8; 32];
        long[(32 - short.len())..].copy_from_slice(&short);
        long
    }

    #[test]
    fn test_deoxysii() {
        let mut key = [0u8; 32];
        key.copy_from_slice(b"this must be the excelentest key");
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(b"complete noncence, and too long.");
        let plaintext = b"plaintext";
        let plaintext_len = get_usize_bytes(plaintext.len());
        let ad = b"additional data";
        let ad_len = get_usize_bytes(ad.len());

        // Compose the input blob and try calling with partial fragments.
        let mut plain_input: Vec<u8> = Vec::new();
        plain_input.extend_from_slice(&key);
        plain_input.extend_from_slice(&nonce);
        plain_input.extend_from_slice(&plaintext_len);
        call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03,
            ]),
            &plain_input,
            1_000_000,
        )
        .expect("call should return something")
        .expect_err("call should fail");

        plain_input.extend_from_slice(&ad_len);
        call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03,
            ]),
            &plain_input,
            1_000_000,
        )
        .expect("call should return something")
        .expect_err("call should fail");

        plain_input.extend_from_slice(plaintext);
        plain_input.resize((plain_input.len() + 31) & (!31), 0);
        plain_input.extend_from_slice(ad);
        call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03,
            ]),
            &plain_input,
            1_000_000,
        )
        .expect("call should return something")
        .expect_err("call should fail");

        plain_input.resize((plain_input.len() + 31) & (!31), 0);

        // Get ciphertext.
        let result = call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03,
            ]),
            &plain_input,
            1_000_000,
        )
        .expect("call should return something")
        .expect("call should succeed");
        let ciphertext = result.output;
        let ciphertext_len = get_usize_bytes(ciphertext.len());

        // Compose input blob for decryption.
        let mut cipher_input: Vec<u8> = Vec::new();
        cipher_input.extend_from_slice(&key);
        cipher_input.extend_from_slice(&nonce);
        cipher_input.extend_from_slice(&ciphertext_len);
        cipher_input.extend_from_slice(&ad_len);
        cipher_input.extend_from_slice(&ciphertext);
        cipher_input.resize((cipher_input.len() + 31) & (!31), 0);
        cipher_input.extend_from_slice(ad);
        cipher_input.resize((cipher_input.len() + 31) & (!31), 0);

        // Try decrypting and compare.
        let result = call_contract(
            H160([
                0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04,
            ]),
            &cipher_input,
            1_000_000,
        )
        .expect("call should return something")
        .expect("call should succeed");
        assert_eq!(result.output, plaintext);
    }

    #[bench]
    fn bench_deoxysii_short(b: &mut Bencher) {
        let mut key = [0u8; 32];
        key.copy_from_slice(b"this must be the excelentest key");
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(b"complete noncence, and too long.");
        let plaintext = b"01234567890123456789";
        let plaintext_len = get_usize_bytes(plaintext.len());
        let ad = b"additional data";
        let ad_len = get_usize_bytes(ad.len());

        let mut plain_input: Vec<u8> = Vec::new();
        plain_input.extend_from_slice(&key);
        plain_input.extend_from_slice(&nonce);
        plain_input.extend_from_slice(&plaintext_len);
        plain_input.extend_from_slice(&ad_len);
        plain_input.extend_from_slice(plaintext);
        plain_input.resize((plain_input.len() + 31) & (!31), 0);
        plain_input.extend_from_slice(ad);
        plain_input.resize((plain_input.len() + 31) & (!31), 0);

        b.iter(|| {
            call_contract(
                H160([
                    0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03,
                ]),
                &plain_input,
                10_000_000,
            )
            .expect("call should return something")
            .expect("call should succeed");
        });
    }

    #[bench]
    fn bench_deoxysii_long(b: &mut Bencher) {
        let mut key = [0u8; 32];
        key.copy_from_slice(b"this must be the excelentest key");
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(b"complete noncence, and too long.");
        let plaintext = b"0123456789".repeat(200);
        let plaintext_len = get_usize_bytes(plaintext.len());
        let ad = b"additional data";
        let ad_len = get_usize_bytes(ad.len());

        let mut plain_input: Vec<u8> = Vec::new();
        plain_input.extend_from_slice(&key);
        plain_input.extend_from_slice(&nonce);
        plain_input.extend_from_slice(&plaintext_len);
        plain_input.extend_from_slice(&ad_len);
        plain_input.extend_from_slice(&plaintext);
        plain_input.resize((plain_input.len() + 31) & (!31), 0);
        plain_input.extend_from_slice(ad);
        plain_input.resize((plain_input.len() + 31) & (!31), 0);

        b.iter(|| {
            call_contract(
                H160([
                    0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03,
                ]),
                &plain_input,
                10_000_000,
            )
            .expect("call should return something")
            .expect("call should succeed");
        });
    }
}
