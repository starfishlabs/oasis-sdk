//! Storage imports.
use std::convert::TryInto;

use oasis_contract_sdk_types::storage::StoreKind;
use oasis_runtime_sdk::{
    context::TxContext,
    storage::{self, Store},
};

use super::{memory::Region, OasisV1};
use crate::{
    abi::{gas, ExecutionContext},
    state, Error, MODULE_NAME,
};

/// Create a contract instance store.
fn get_instance_store<'a, C: TxContext>(
    ec: &'a mut ExecutionContext<'_, C>,
    store_kind: u32,
) -> Result<impl Store + 'a, wasm3::Trap> {
    // Determine which store we should be using.
    let store_kind: StoreKind = store_kind.try_into().map_err(|_| wasm3::Trap::Abort)?;

    // Create the given store.
    let store = storage::PrefixStore::new(ec.tx_context.runtime_state(), &MODULE_NAME);
    let instance_prefix = ec.instance_info.id.to_storage_key();
    let contract_state = storage::PrefixStore::new(
        storage::PrefixStore::new(store, &state::INSTANCE_STATE),
        instance_prefix,
    );
    let contract_state = storage::PrefixStore::new(contract_state, store_kind.prefix());

    match store_kind {
        StoreKind::Public => Ok(contract_state),
        StoreKind::Confidential => Err(wasm3::Trap::Abort), // Not yet implemented.
    }
}

impl<'ctx, C: TxContext> OasisV1<'ctx, C> {
    /// Link storage functions.
    pub fn link_storage(
        &mut self,
        instance: &mut wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
    ) -> Result<(), Error> {
        // storage.get(store, key) -> value
        let _ = instance.link_function(
            "storage",
            "get",
            |ctx, (store, key): (u32, (u32, u32))| -> Result<(u32, u32), wasm3::Trap> {
                // Make sure function was called in valid context.
                let ec = ctx.context.ok_or(wasm3::Trap::Abort)?;

                // Charge base gas amount.
                gas::use_gas(ctx.instance, ec.params.gas_costs.wasm_storage_get_base)?;

                // Read from contract state.
                let value = ctx.instance.runtime().try_with_memory(
                    |memory| -> Result<_, wasm3::Trap> {
                        let key = Region::from_arg(key).as_slice(&memory)?;
                        // TODO: Maximum key/value size limits.
                        // TODO: Charge gas per key/value size.
                        Ok(get_instance_store(ec, store)?.get(key))
                    },
                )??;

                let value = match value {
                    Some(value) => value,
                    None => return Ok((0, 0)),
                };

                // Create new region by calling `allocate`.
                //
                // This makes sure that the call context is unset to avoid any potential issues
                // with reentrancy as attempting to re-enter one of the linked function will fail.
                let value_region = Self::allocate_and_copy(ctx.instance, &value)?;

                Ok(value_region.to_arg())
            },
        );

        // storage.insert(store, key, value)
        let _ = instance.link_function(
            "storage",
            "insert",
            |ctx, (store, key, value): (u32, (u32, u32), (u32, u32))| {
                // Make sure function was called in valid context.
                let ec = ctx.context.ok_or(wasm3::Trap::Abort)?;

                // Charge base gas amount.
                gas::use_gas(ctx.instance, ec.params.gas_costs.wasm_storage_insert_base)?;

                // Insert into contract state.
                ctx.instance
                    .runtime()
                    .try_with_memory(|memory| -> Result<(), wasm3::Trap> {
                        let key = Region::from_arg(key).as_slice(&memory)?;
                        let value = Region::from_arg(value).as_slice(&memory)?;
                        // TODO: Maximum key/value size limits.
                        // TODO: Charge gas per key/value size.
                        get_instance_store(ec, store)?.insert(key, value);
                        Ok(())
                    })??;

                Ok(())
            },
        );

        // storage.remove(store, key)
        let _ = instance.link_function(
            "storage",
            "remove",
            |ctx, (store, key): (u32, (u32, u32))| {
                // Make sure function was called in valid context.
                let ec = ctx.context.ok_or(wasm3::Trap::Abort)?;

                // Charge base gas amount.
                gas::use_gas(ctx.instance, ec.params.gas_costs.wasm_storage_remove_base)?;

                // Remove from contract state.
                ctx.instance
                    .runtime()
                    .try_with_memory(|memory| -> Result<(), wasm3::Trap> {
                        let key = Region::from_arg(key).as_slice(&memory)?;
                        // TODO: Maximum key/value size limits.
                        // TODO: Charge gas per key/value size.
                        get_instance_store(ec, store)?.remove(key);
                        Ok(())
                    })??;

                Ok(())
            },
        );

        Ok(())
    }
}
