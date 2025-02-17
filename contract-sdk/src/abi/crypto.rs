//! Crypto helpers ABI.

#[link(wasm_import_module = "crypto")]
extern "C" {
    #[link_name = "ecdsa_recover"]
    pub(crate) fn crypto_ecdsa_recover(
        input_ptr: u32,
        input_len: u32,
        output_ptr: u32,
        output_len: u32,
    );

    #[link_name = "signature_verify"]
    pub(crate) fn crypto_signature_verify(
        kind: u32,
        key_ptr: u32,
        key_len: u32,
        context_ptr: u32,
        context_len: u32,
        message_ptr: u32,
        message_len: u32,
        signature_ptr: u32,
        signature_len: u32,
    ) -> u32;
}
