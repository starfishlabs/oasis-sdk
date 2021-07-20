//! Tests for the contracts module.
use std::collections::BTreeMap;

use oasis_runtime_sdk::{
    context,
    error::Error,
    module,
    modules::{
        accounts::{self, Module as Accounts, API as _},
        core::{self, Module as Core},
    },
    testing::{keys, mock},
    types::{
        token::{BaseUnits, Denomination},
        transaction,
    },
    BatchContext, Context, Runtime, Version,
};

use crate::{types, Genesis};

/// Hello contract code.
static HELLO_CONTRACT: &[u8] = include_bytes!("../../../../tests/contracts/hello/hello.wasm");

type Contracts = crate::Module<Accounts>;

#[test]
fn test_hello_contract_call() {
    let mut mock = mock::Mock::default();
    let mut ctx = mock.create_ctx();

    Core::init(
        &mut ctx,
        core::Genesis {
            parameters: core::Parameters {
                max_batch_gas: 10_000_000,
                ..Default::default()
            },
        },
    );

    Accounts::init(
        &mut ctx,
        accounts::Genesis {
            balances: {
                let mut balances = BTreeMap::new();
                // Alice.
                balances.insert(keys::alice::address(), {
                    let mut denominations = BTreeMap::new();
                    denominations.insert(Denomination::NATIVE, 1_000_000);
                    denominations
                });
                balances
            },
            total_supplies: {
                let mut total_supplies = BTreeMap::new();
                total_supplies.insert(Denomination::NATIVE, 1_000_000);
                total_supplies
            },
            ..Default::default()
        },
    );

    Contracts::init(
        &mut ctx,
        Genesis {
            parameters: Default::default(),
        },
    );

    // First upload the contract code.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Upload".to_owned(),
            body: cbor::to_value(types::Upload {
                abi: types::ABI::OasisV1,
                instantiate_policy: types::Policy::Everyone,
                code: HELLO_CONTRACT.to_vec(),
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 1000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        Contracts::tx_upload(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect("upload should succeed");

        tx_ctx.commit();
    });

    // Then instantiate the code.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Instantiate".to_owned(),
            body: cbor::to_value(types::Instantiate {
                code_id: 0.into(),
                upgrades_policy: types::Policy::Nobody,
                data: cbor::to_vec(cbor::cbor_text!("instantiate")), // Needs to conform to contract API.
                tokens: vec![BaseUnits::new(1_000, Denomination::NATIVE)],
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 1_000_000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        let result = Contracts::tx_instantiate(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect("instantiate should succeed");

        // Check caller account balances.
        let bals = Accounts::get_balances(tx_ctx.runtime_state(), keys::alice::address())
            .expect("get_balances should succeed");
        assert_eq!(
            bals.balances[&Denomination::NATIVE],
            999_000, // -1_000
            "balance in caller account should be correct"
        );
        assert_eq!(
            bals.balances.len(),
            1,
            "there should only be one denomination"
        );

        // Check contract account balances.
        let bals = Accounts::get_balances(
            tx_ctx.runtime_state(),
            types::Instance::address_for(result.id),
        )
        .expect("get_balances should succeed");
        assert_eq!(
            bals.balances[&Denomination::NATIVE],
            1_000, // +1_000
            "balance in contract account should be correct"
        );
        assert_eq!(
            bals.balances.len(),
            1,
            "there should only be one denomination"
        );

        tx_ctx.commit();
    });

    // And finally call a method.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Call".to_owned(),
            body: cbor::to_value(types::Call {
                id: 0.into(),
                // Needs to conform to contract API.
                data: cbor::to_vec(cbor::cbor_map! {
                    "say_hello" => cbor::cbor_map!{
                        "who" => cbor::cbor_text!("tester")
                    }
                }),
                tokens: vec![BaseUnits::new(2_000, Denomination::NATIVE)],
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 1_000_000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        let result = Contracts::tx_call(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect("call should succeed");

        let result: cbor::Value =
            cbor::from_slice(&result.0).expect("result should be correctly formatted");
        assert_eq!(
            result,
            cbor::cbor_map! {
                "hello" => cbor::cbor_map!{
                    "greeting" => cbor::cbor_text!("hello tester (1)")
                }
            }
        );

        // Check caller account balances.
        let bals = Accounts::get_balances(tx_ctx.runtime_state(), keys::alice::address())
            .expect("get_balances should succeed");
        assert_eq!(
            bals.balances[&Denomination::NATIVE],
            997_000, // -2_000
            "balance in caller account should be correct"
        );
        assert_eq!(
            bals.balances.len(),
            1,
            "there should only be one denomination"
        );

        // Check contract account balances.
        let bals = Accounts::get_balances(
            tx_ctx.runtime_state(),
            types::Instance::address_for(0.into()),
        )
        .expect("get_balances should succeed");
        assert_eq!(
            bals.balances[&Denomination::NATIVE],
            3_000, // +2_000
            "balance in contract account should be correct"
        );
        assert_eq!(
            bals.balances.len(),
            1,
            "there should only be one denomination"
        );

        tx_ctx.commit();
    });

    // Second call should increment the counter.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Call".to_owned(),
            body: cbor::to_value(types::Call {
                id: 0.into(),
                // Needs to conform to contract API.
                data: cbor::to_vec(cbor::cbor_map! {
                    "say_hello" => cbor::cbor_map!{
                        "who" => cbor::cbor_text!("second")
                    }
                }),
                tokens: vec![],
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 1_000_000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        let result = Contracts::tx_call(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect("call should succeed");

        let result: cbor::Value =
            cbor::from_slice(&result.0).expect("result should be correctly formatted");
        assert_eq!(
            result,
            cbor::cbor_map! {
                "hello" => cbor::cbor_map!{
                    "greeting" => cbor::cbor_text!("hello second (2)")
                }
            }
        );

        // Check caller account balances.
        let bals = Accounts::get_balances(tx_ctx.runtime_state(), keys::alice::address())
            .expect("get_balances should succeed");
        assert_eq!(
            bals.balances[&Denomination::NATIVE],
            997_000, // No change.
            "balance in caller account should be correct"
        );
        assert_eq!(
            bals.balances.len(),
            1,
            "there should only be one denomination"
        );

        // Check contract account balances.
        let bals = Accounts::get_balances(
            tx_ctx.runtime_state(),
            types::Instance::address_for(0.into()),
        )
        .expect("get_balances should succeed");
        assert_eq!(
            bals.balances[&Denomination::NATIVE],
            3_000, // No change.
            "balance in contract account should be correct"
        );
        assert_eq!(
            bals.balances.len(),
            1,
            "there should only be one denomination"
        );

        tx_ctx.commit();
    });
}

/// Contract runtime.
struct ContractRuntime;

impl Runtime for ContractRuntime {
    const VERSION: Version = Version::new(0, 0, 0);

    type Modules = (Core, Accounts, Contracts);

    fn genesis_state() -> <Self::Modules as module::MigrationHandler>::Genesis {
        (
            core::Genesis {
                parameters: core::Parameters {
                    max_batch_gas: 10_000_000,
                    ..Default::default()
                },
            },
            accounts::Genesis {
                ..Default::default()
            },
            Genesis {
                parameters: Default::default(),
            },
        )
    }
}

#[test]
fn test_hello_contract_subcalls() {
    let mut mock = mock::Mock::default();
    let mut ctx = mock.create_ctx_for_runtime::<ContractRuntime>(context::Mode::ExecuteTx);

    ContractRuntime::migrate(&mut ctx);

    // First upload the contract code.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Upload".to_owned(),
            body: cbor::to_value(types::Upload {
                abi: types::ABI::OasisV1,
                instantiate_policy: types::Policy::Everyone,
                code: HELLO_CONTRACT.to_vec(),
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 1000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        Contracts::tx_upload(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect("upload should succeed");

        tx_ctx.commit();
    });

    // Then instantiate the code.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Instantiate".to_owned(),
            body: cbor::to_value(types::Instantiate {
                code_id: 0.into(),
                upgrades_policy: types::Policy::Nobody,
                data: cbor::to_vec(cbor::cbor_text!("instantiate")), // Needs to conform to contract API.
                tokens: vec![],
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 1_000_000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        Contracts::tx_instantiate(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect("instantiate should succeed");

        tx_ctx.commit();
    });

    // And finally call a method.
    let tx = transaction::Transaction {
        version: 1,
        call: transaction::Call {
            method: "contracts.Call".to_owned(),
            body: cbor::to_value(types::Call {
                id: 0.into(),
                data: cbor::to_vec(cbor::cbor_text!("call_self")), // Needs to conform to contract API.
                tokens: vec![],
            }),
        },
        auth_info: transaction::AuthInfo {
            signer_info: vec![transaction::SignerInfo::new(keys::alice::pk(), 0)],
            fee: transaction::Fee {
                amount: Default::default(),
                gas: 2_000_000,
            },
        },
    };
    ctx.with_tx(tx, |mut tx_ctx, call| {
        let result = Contracts::tx_call(&mut tx_ctx, cbor::from_value(call.body).unwrap())
            .expect_err("call should fail");

        assert_eq!(result.module_name(), "contracts.0.handle_reply");
        assert_eq!(result.code(), 1);
        assert_eq!(&result.to_string(), "contract error: subcall failed");
    });
}
