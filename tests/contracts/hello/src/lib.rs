#![feature(wasm_abi)]

extern crate alloc;

use oasis_contract_sdk as sdk;
use oasis_contract_sdk::types::{
    message::{Message, NotifyReply, Reply},
    storage::StoreKind,
};

pub struct HelloWorld;

#[derive(Clone, Debug, cbor::Encode, cbor::Decode)]
pub enum Request {
    #[cbor(rename = "instantiate")]
    Instantiate,

    #[cbor(rename = "say_hello")]
    SayHello { who: String },

    #[cbor(rename = "call_self")]
    CallSelf,

    #[cbor(rename = "increment_counter")]
    IncrementCounter,
}

#[derive(Clone, Debug, cbor::Encode, cbor::Decode)]
pub enum Response {
    #[cbor(rename = "hello")]
    Hello { greeting: String },

    #[cbor(rename = "empty")]
    Empty,
}

const STATE_COUNTER: &[u8] = b"counter";

impl HelloWorld {
    /// Increment the counter and return the previous value.
    fn increment_counter() -> u64 {
        let counter: u64 = sdk::storage::get(StoreKind::Public, STATE_COUNTER)
            .map(|raw| cbor::from_slice(&raw).unwrap())
            .unwrap_or_default();
        sdk::storage::insert(StoreKind::Public, STATE_COUNTER, &cbor::to_vec(counter + 1));

        counter
    }
}

impl sdk::Contract for HelloWorld {
    type Request = Request;
    type Response = Response;

    fn instantiate<C: sdk::Context>(_ctx: &mut C, _request: Request) -> Result<(), sdk::Error> {
        // Initialize counter to 1.
        sdk::storage::insert(StoreKind::Public, STATE_COUNTER, &cbor::to_vec(1u64));

        Ok(())
    }

    fn call<C: sdk::Context>(ctx: &mut C, request: Request) -> Result<Response, sdk::Error> {
        match request {
            Request::SayHello { who } => {
                let counter = Self::increment_counter();

                Ok(Response::Hello {
                    greeting: format!("hello {} ({})", who, counter),
                })
            }
            Request::CallSelf => {
                use cbor::cbor_map;

                ctx.emit_message(Message::Call {
                    id: 0,
                    reply: NotifyReply::Always,
                    method: "contracts.Call".to_string(),
                    body: cbor::cbor_map! {
                        "id" => cbor::cbor_int!(ctx.instance_id().as_u64() as i64),
                        "data" => cbor::cbor_bytes!(cbor::to_vec(cbor::cbor_text!("call_self"))),
                        "tokens" => cbor::cbor_array![],
                    },
                    max_gas: None,
                });
                Ok(Response::Empty)
            }
            Request::IncrementCounter => {
                Self::increment_counter();
                Ok(Response::Empty)
            }
            _ => Err(sdk::Error {
                module: "".to_string(),
                code: 1,
                message: "bad request".to_string(),
            }),
        }
    }

    fn handle_reply<C: sdk::Context>(
        _ctx: &mut C,
        reply: Reply,
    ) -> Result<Option<Self::Response>, sdk::Error> {
        match reply {
            Reply::Call { result, .. } => {
                // Propagate all failures.
                if !result.is_success() {
                    return Err(sdk::Error {
                        module: "handle_reply".to_string(),
                        code: 1,
                        message: "subcall failed".to_string(),
                    });
                }

                // Do not modify the result.
                Ok(None)
            }
        }
    }
}

sdk::create_contract!(HelloWorld);
