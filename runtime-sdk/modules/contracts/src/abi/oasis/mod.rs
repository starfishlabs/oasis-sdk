//! The Oasis ABIs.
use std::collections::BTreeSet;

use oasis_contract_sdk_types as contract_sdk;
use oasis_runtime_sdk::{
    context::TxContext,
    modules::core::{self, API as _},
    types::token,
};

use super::{gas, ExecutionContext, ABI};
use crate::{types, wasm::ContractError, Error, Parameters};

mod memory;
mod storage;
#[cfg(test)]
mod test;

const EXPORT_INSTANTIATE: &str = "instantiate";
const EXPORT_CALL: &str = "call";
const EXPORT_HANDLE_REPLY: &str = "handle_reply";

const GAS_SCALING_FACTOR: u64 = 1;

/// The Oasis V1 ABI.
pub struct OasisV1<'ctx, C: TxContext> {
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
}

impl<'ctx, C: TxContext + 'ctx> OasisV1<'ctx, C> {
    /// The set of required exports.
    const REQUIRED_EXPORTS: &'static [&'static str] = &[
        memory::EXPORT_ALLOCATE,
        memory::EXPORT_DEALLOCATE,
        EXPORT_INSTANTIATE,
        EXPORT_CALL,
    ];

    /// The set of reserved exports.
    const RESERVED_EXPORTS: &'static [&'static str] =
        &[gas::EXPORT_GAS_LIMIT, gas::EXPORT_GAS_LIMIT_EXHAUSTED];

    /// Create a new instance of the Oasis V1 ABI.
    pub fn new(ctx: &'ctx mut C, params: &'ctx Parameters) -> Self {
        OasisV1 { ctx, params }
    }

    fn raw_call_with_request_context(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        request: &[u8],
        deposited_tokens: &[token::BaseUnits],
        instance_info: &types::Instance,
        function_name: &str,
    ) -> Result<contract_sdk::ExecutionOk, Error> {
        // Allocate memory for context and request, copy serialized data into the region.
        let context_dst = Self::serialize_and_allocate(
            instance,
            contract_sdk::ExecutionContext {
                instance_id: instance_info.id,
                instance_address: instance_info.address().into(),
                signer_addresses: self
                    .ctx
                    .tx_auth_info()
                    .signer_info
                    .iter()
                    .map(|si| si.address_spec.address().into())
                    .collect(),
                deposited_tokens: deposited_tokens.iter().map(|b| b.into()).collect(),
            },
        )
        .map_err(|err| Error::ExecutionFailed(err.into()))?;
        let request_dst = Self::allocate_and_copy(instance, request)
            .map_err(|err| Error::ExecutionFailed(err.into()))?;

        // Call the corresponding function in the smart contract.
        let mut ec = ExecutionContext {
            instance_info,
            tx_context: self.ctx,
            params: self.params,
        };
        let result = {
            // The high-level function signature of the WASM export is as follows:
            //
            //   fn(ctx: &contract_sdk::ExecutionContext, request: &[u8]) -> contract_sdk::ExecutionResult
            //
            let func = instance
                .find_function::<((u32, u32), (u32, u32)), (u32, u32)>(function_name)
                .map_err(|err| Error::ExecutionFailed(err.into()))?;
            let result = func
                .call_with_context(&mut ec, (context_dst.to_arg(), request_dst.to_arg()))
                .map_err(|err| Error::ExecutionFailed(err.into()))?;
            memory::Region::from_arg(result)
        };

        // Enforce maximum result size limit before attempting to deserialize it.
        if result.length as u32 > self.params.max_result_size_bytes {
            return Err(Error::ResultTooLarge(
                result.length as u32,
                self.params.max_result_size_bytes,
            ));
        }

        // Deserialize region into result structure.
        let result: contract_sdk::ExecutionResult = instance
            .runtime()
            .try_with_memory(|memory| -> Result<_, Error> {
                let data = result
                    .as_slice(&memory)
                    .map_err(|err| Error::ExecutionFailed(err.into()))?;

                cbor::from_slice(data).map_err(|err| Error::ExecutionFailed(err.into()))
            })
            .unwrap()?;

        match result {
            contract_sdk::ExecutionResult::Ok(ok) => Ok(ok),
            contract_sdk::ExecutionResult::Failed {
                module,
                code,
                message,
            } => Err(ContractError::new(instance_info.code_id, &module, code, &message).into()),
        }
    }

    fn call_with_request_context(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        request: &[u8],
        deposited_tokens: &[token::BaseUnits],
        instance_info: &types::Instance,
        function_name: &str,
    ) -> Result<contract_sdk::ExecutionOk, Error> {
        // Fetch initial gas counter value so we can determine how much gas was used.
        let initial_gas = gas::get_remaining_gas(instance).unwrap_or_default();

        let result = self
            .raw_call_with_request_context(
                instance,
                request,
                deposited_tokens,
                instance_info,
                function_name,
            )
            .map_err(|err| {
                // Check if call failed due to gas being exhausted and return a proper error.
                if gas::is_gas_limit_exhausted(instance) {
                    core::Error::OutOfGas.into()
                } else {
                    err
                }
            });

        // Update transaction context gas limit based on how much gas was actually used.
        let final_gas = gas::get_remaining_gas(instance).unwrap_or_default();
        let gas_usage = initial_gas.saturating_sub(final_gas) / GAS_SCALING_FACTOR;
        // The following call should never fail as we accounted for all the gas in advance.
        core::Module::use_tx_gas(self.ctx, gas_usage)?;

        result
    }
}

impl<'ctx, C: TxContext> ABI<C> for OasisV1<'ctx, C> {
    fn validate(&self, module: &mut walrus::Module) -> Result<(), Error> {
        // Verify that all required exports are there.
        let exports: BTreeSet<&str> = module
            .exports
            .iter()
            .map(|export| export.name.as_str())
            .collect();
        for required in Self::REQUIRED_EXPORTS {
            if !exports.contains(required) {
                return Err(Error::CodeMissingRequiredExport(required.to_string()));
            }
        }

        for reserved in Self::RESERVED_EXPORTS {
            if exports.contains(reserved) {
                return Err(Error::CodeDeclaresReservedExport(reserved.to_string()));
            }
        }

        // Verify that there is no start function defined.
        if module.start.is_some() {
            return Err(Error::CodeDeclaresStartFunction);
        }

        // Verify that there is at most one memory defined.
        if module.memories.iter().count() > 1 {
            return Err(Error::CodeDeclaresTooManyMemories);
        }

        // Add gas metering instrumentation.
        gas::transform(module);

        Ok(())
    }

    fn link(
        &mut self,
        instance: &mut wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
    ) -> Result<(), Error> {
        // Storage imports.
        self.link_storage(instance)?;
        // Environment query imports.
        // TODO

        // Derive gas limit from remaining transaction gas based on a scaling factor.
        let remaining_gas =
            core::Module::remaining_gas(self.ctx).saturating_mul(GAS_SCALING_FACTOR);
        gas::set_gas_limit(instance, remaining_gas)?;

        Ok(())
    }

    fn instantiate(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        request: &[u8],
        deposited_tokens: &[token::BaseUnits],
        instance_info: &types::Instance,
    ) -> Result<contract_sdk::ExecutionOk, Error> {
        self.call_with_request_context(
            instance,
            request,
            deposited_tokens,
            instance_info,
            EXPORT_INSTANTIATE,
        )
    }

    fn call(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        request: &[u8],
        deposited_tokens: &[token::BaseUnits],
        instance_info: &types::Instance,
    ) -> Result<contract_sdk::ExecutionOk, Error> {
        self.call_with_request_context(
            instance,
            request,
            deposited_tokens,
            instance_info,
            EXPORT_CALL,
        )
    }

    fn handle_reply(
        &mut self,
        instance: &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
        reply: contract_sdk::message::Reply,
        instance_info: &types::Instance,
    ) -> Result<contract_sdk::ExecutionOk, Error> {
        self.call_with_request_context(
            instance,
            &cbor::to_vec(reply),
            &[],
            instance_info,
            EXPORT_HANDLE_REPLY,
        )
    }
}
