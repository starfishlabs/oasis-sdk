//! WASM ABI supported by the contracts module.
use oasis_contract_sdk_types::{message::Reply, ExecutionOk};
use oasis_runtime_sdk::{context::TxContext, types::token};

use super::{types, Error, Parameters};

pub mod gas;
pub mod oasis;

/// Trait for any WASM ABI to implement.
pub trait ABI<C: TxContext> {
    /// Validate that the given WASM module conforms to the ABI.
    fn validate(&self, module: &mut walrus::Module) -> Result<(), Error>;

    /// Link required functions into the WASM module instance.
    fn link(
        &mut self,
        instance: &mut wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
    ) -> Result<(), Error>;

    /// Instantiate a contract.
    fn instantiate(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        request: &[u8],
        deposited_tokens: &[token::BaseUnits],
        instance_info: &types::Instance,
    ) -> Result<ExecutionOk, Error>;

    /// Call a contract.
    fn call(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        request: &[u8],
        deposited_tokens: &[token::BaseUnits],
        instance_info: &types::Instance,
    ) -> Result<ExecutionOk, Error>;

    /// Invoke the contract's reply handler.
    fn handle_reply(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        reply: Reply,
        instance_info: &types::Instance,
    ) -> Result<ExecutionOk, Error>;
}

/// Execution context.
pub struct ExecutionContext<'ctx, C: TxContext> {
    /// Contract instance information.
    pub instance_info: &'ctx types::Instance,
    /// Transaction context.
    pub tx_context: &'ctx mut C,
    /// Module parameters.
    pub params: &'ctx Parameters,
}
