//! Contract execution context.
use crate::types::{address::Address, message::Message, token, ExecutionContext, InstanceId};

/// Execution context.
pub trait Context {
    /// Contract instance identifier.
    fn instance_id(&self) -> InstanceId;

    /// Contract instance address.
    fn instance_address(&self) -> &Address;

    /// Signer addresses.
    fn signer_addresses(&self) -> &[Address];

    /// Tokens deposited by the caller.
    fn deposited_tokens(&self) -> &[token::BaseUnits];

    /// Emits a message.
    fn emit_message(&mut self, msg: Message);
}

pub(crate) struct Internal {
    ec: ExecutionContext,

    /// Emitted messages.
    pub(crate) messages: Vec<Message>,
}

impl From<ExecutionContext> for Internal {
    fn from(ec: ExecutionContext) -> Self {
        Self {
            ec,
            messages: Vec::new(),
        }
    }
}

impl Context for Internal {
    fn instance_id(&self) -> InstanceId {
        self.ec.instance_id
    }

    fn instance_address(&self) -> &Address {
        &self.ec.instance_address
    }

    fn signer_addresses(&self) -> &[Address] {
        &self.ec.signer_addresses
    }

    fn deposited_tokens(&self) -> &[token::BaseUnits] {
        &self.ec.deposited_tokens
    }

    fn emit_message(&mut self, msg: Message) {
        self.messages.push(msg);
    }
}
