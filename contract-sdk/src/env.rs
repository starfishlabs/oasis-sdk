//! Smart contract environment query interface.
use oasis_contract_sdk_types::address::Address;

use crate::types::{
    env::{QueryRequest, QueryResponse},
    InstanceId,
};

/// Environment query trait.
pub trait Env {
    /// Perform an environment query.
    fn query<Q: Into<QueryRequest>>(&self, query: Q) -> QueryResponse;

    /// Returns an address for the contract instance id.
    fn address_for_instance(&self, instance_id: InstanceId) -> Address;

    /// Prints a message to the console. Useful when debugging.
    #[cfg(feature = "debug-utils")]
    fn debug_print(&self, msg: &str);
}

/// Crypto helpers trait.
pub trait Crypto {
    /// ECDSA public key recovery function.
    fn ecdsa_recover(&self, input: &[u8]) -> [u8; 65];

    /// Verify an ed25519 message signature.
    fn signature_verify_ed25519(&self, key: &[u8], message: &[u8], signature: &[u8]) -> bool;

    /// Verify a secp256k1 message signature.
    fn signature_verify_secp256k1(&self, key: &[u8], message: &[u8], signature: &[u8]) -> bool;

    /// Verify an sr25519 message signature.
    fn signature_verify_sr25519(
        &self,
        key: &[u8],
        context: &[u8],
        message: &[u8],
        signature: &[u8],
    ) -> bool;
}
