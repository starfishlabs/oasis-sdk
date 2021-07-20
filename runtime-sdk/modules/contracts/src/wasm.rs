//! WASM runtime.
use oasis_contract_sdk_types::{message::Reply, ExecutionOk};
use oasis_runtime_sdk::context::TxContext;

use super::{
    abi::{oasis::OasisV1, ExecutionContext, ABI},
    types, Error, Parameters, MODULE_NAME,
};

/// Everything needed to run a contract.
pub struct Contract<'a> {
    pub code_info: &'a types::Code,
    pub code: &'a [u8],
    pub instance_info: &'a types::Instance,
}

/// Error emitted from within a contract.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ContractError {
    pub module: String,
    pub code: u32,
    pub message: String,
}

impl ContractError {
    /// Create a new error emitted within a contract.
    pub fn new(code_id: types::CodeId, module: &str, code: u32, message: &str) -> Self {
        Self {
            module: if module.is_empty() {
                format!("{}.{}", MODULE_NAME, code_id.as_u64())
            } else {
                format!("{}.{}.{}", MODULE_NAME, code_id.as_u64(), module)
            },
            code,
            message: message.to_string(),
        }
    }
}

impl oasis_runtime_sdk::error::Error for ContractError {
    fn module_name(&self) -> &str {
        &self.module
    }

    fn code(&self) -> u32 {
        self.code
    }
}

/// Validate the passed contract code to make sure it conforms to the given ABI and perform any
/// required transformation passes.
pub(super) fn validate_and_transform<'ctx, C: TxContext>(
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
    code: &[u8],
    abi: types::ABI,
) -> Result<Vec<u8>, Error> {
    // Parse code.
    let mut module = walrus::ModuleConfig::new()
        .generate_producers_section(false)
        .parse(&code)
        .map_err(|_| Error::CodeMalformed)?;

    // Validate ABI selection and make sure the code conforms to the specified ABI.
    let abi = create_abi(ctx, params, abi)?;
    abi.validate(&mut module)?;

    Ok(module.emit_wasm())
}

/// Create a new WASM runtime and link the required functions based on the ABI then run the
/// provided function passing the ABI and module instance.
fn with_runtime<'ctx, C, F, R>(
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
    contract: &Contract<'_>,
    f: F,
) -> Result<R, Error>
where
    C: TxContext,
    F: FnOnce(
        &mut Box<dyn ABI<C> + 'ctx>,
        &wasm3::Instance<'_, '_, ExecutionContext<'_, C>>,
    ) -> Result<R, Error>,
{
    // Create the appropriate ABI.
    let mut abi = create_abi(ctx, params, contract.code_info.abi)?;

    // Create the wasm3 environment, parse and instantiate the module.
    let env = wasm3::Environment::new().expect("creating a new wasm3 environment should succeed");
    let module = env
        .parse_module(contract.code)
        .map_err(|_| Error::ModuleLoadingFailed)?;
    let rt = env
        .new_runtime::<ExecutionContext<'_, C>>(
            params.max_stack_size,
            Some(params.max_memory_pages),
        )
        .expect("creating a new wasm3 runtime should succeed");
    let mut instance = rt
        .load_module(module)
        .map_err(|_| Error::ModuleLoadingFailed)?;

    // Link functions based on the ABI.
    abi.link(&mut instance)?;

    // Run the given function.
    f(&mut abi, &instance)
}

/// Instantiate the contract.
pub(super) fn instantiate<'ctx, C: TxContext>(
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
    contract: &Contract<'_>,
    call: &types::Instantiate,
) -> Result<ExecutionOk, Error> {
    with_runtime(ctx, params, contract, |abi, instance| {
        abi.instantiate(instance, &call.data, &call.tokens, contract.instance_info)
    })
}

/// Call the contract.
pub(super) fn call<'ctx, C: TxContext>(
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
    contract: &Contract<'_>,
    call: &types::Call,
) -> Result<ExecutionOk, Error> {
    with_runtime(ctx, params, contract, |abi, instance| {
        abi.call(instance, &call.data, &call.tokens, contract.instance_info)
    })
}

/// Invoke the contract's reply handler.
pub(super) fn handle_reply<'ctx, C: TxContext>(
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
    contract: &Contract<'_>,
    reply: Reply,
) -> Result<ExecutionOk, Error> {
    with_runtime(ctx, params, contract, move |abi, instance| {
        abi.handle_reply(instance, reply, contract.instance_info)
    })
}

/// Create the appropriate ABI based on contract configuration.
fn create_abi<'ctx, C: TxContext>(
    ctx: &'ctx mut C,
    params: &'ctx Parameters,
    abi: types::ABI,
) -> Result<Box<dyn ABI<C> + 'ctx>, Error> {
    match abi {
        types::ABI::OasisV1 => Ok(Box::new(OasisV1::new(ctx, params))),
    }
}
