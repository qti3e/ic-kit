//! Create mock handlers for simulating inter-canister calls.

use std::cell::{Ref, RefCell};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use ic_cdk::api::call::CallResult;
use ic_cdk::export::candid::utils::{ArgumentDecoder, ArgumentEncoder};
use ic_cdk::export::candid::{decode_args, encode_args};

use crate::candid::CandidType;
use crate::{Context, MockContext, Principal};

/// Anything that could be used to simulate a inter-canister call.
pub trait CallHandler {
    /// Whatever the handler can handle the given call or not, if this method returns false, we
    /// skip this handler and try to find the next handler that can handle the call.
    fn accept(&self, canister_id: &Principal, method: &str) -> bool;

    /// Perform the call using this handler. Only called if `accept()` first returned true.
    fn perform(
        &self,
        caller: &Principal,
        cycles: u64,
        canister_id: &Principal,
        method: &str,
        args_raw: &Vec<u8>,
        ctx: Option<&mut MockContext>,
    ) -> (CallResult<Vec<u8>>, u64);
}

/// A method that is constructed using nested calls.
pub struct Method {
    /// An optional name for the method.
    name: Option<String>,
    /// The sub-commands that should be executed by the method.
    atoms: Vec<MethodAtom>,
    /// If set we assert that the arguments passed to the method are this value.
    expected_args: Option<Vec<u8>>,
    /// The response that we send back from the caller. By default `()` is returned.
    response: Option<Vec<u8>>,
}

enum MethodAtom {
    ConsumeAllCycles,
    ConsumeCycles(u64),
    RefundCycles(u64),
}

/// A method which uses Rust closures to handle the calls, it accepts every call.
pub struct RawHandler {
    handler: Box<dyn Fn(&mut MockContext, &Vec<u8>, &Principal, &str) -> CallResult<Vec<u8>>>,
}

/// Can be used to represent a canister and different method on the canister.
pub struct Canister {
    /// ID of the canister, makes the CallHandler skip the call to this canister if it's trying
    /// to make a call to a canister with different id.
    id: Principal,
    /// Implementation of the methods on this canister.
    methods: HashMap<String, Box<dyn CallHandler>>,
    /// The default callback which can be called if the method was not found on this canister.
    default: Option<Box<dyn CallHandler>>,
    /// The context used in this canister.
    context: RefCell<MockContext>,
}

impl Method {
    /// Create a new method.
    #[inline]
    pub const fn new() -> Self {
        Method {
            name: None,
            atoms: Vec::new(),
            expected_args: None,
            response: None,
        }
    }

    /// Put a name for the method. Setting a name on the method makes the CallHandler for this
    /// method skip this method if it's trying to make a call to a method with a different name.
    ///
    /// # Panics
    /// If the method already has a name.
    #[inline]
    pub fn name<S: Into<String>>(mut self, name: S) -> Self {
        if self.name.is_some() {
            panic!("Method already has a name.");
        }

        self.name = Some(name.into());
        self
    }

    /// Make the method consume all of the cycles provided to it.
    #[inline]
    pub fn cycles_consume_all(mut self) -> Self {
        self.atoms.push(MethodAtom::ConsumeAllCycles);
        self
    }

    /// Make the method consume at most the given amount of cycles.
    #[inline]
    pub fn cycles_consume(mut self, cycles: u64) -> Self {
        self.atoms.push(MethodAtom::ConsumeCycles(cycles));
        self
    }

    /// Make the method refund the given amount of cycles.
    #[inline]
    pub fn cycles_refund(mut self, cycles: u64) -> Self {
        self.atoms.push(MethodAtom::RefundCycles(cycles));
        self
    }

    /// Make the method expect the given value as the argument, this method makes the method
    /// panic if it's called with an argument other than what is provided.
    ///
    /// # Panics
    /// If called more than once.
    #[inline]
    pub fn expect_arguments<T: ArgumentEncoder>(mut self, arguments: T) -> Self {
        if self.expected_args.is_some() {
            panic!("expect_arguments can only be called once on a method.");
        }
        self.expected_args = Some(encode_args(arguments).expect("Cannot encode arguments."));
        self
    }

    /// Make the method return the given constant value every time.
    ///
    /// # Panics
    /// If called more than once.
    #[inline]
    pub fn response<T: CandidType>(mut self, value: T) -> Self {
        if self.response.is_some() {
            panic!("response can only be called once on a method.");
        }
        self.response = Some(encode_args((value,)).expect("Failed to encode response."));
        self
    }
}

impl Canister {
    /// Create a new canister with the given principal id, this handler rejects any call to a
    /// different canister id.
    #[inline]
    pub fn new(id: Principal) -> Self {
        let context = MockContext::new().with_id(id.clone());

        Canister {
            id,
            methods: HashMap::new(),
            default: None,
            context: RefCell::new(context),
        }
    }

    /// Return a reference to the context associated with this canister.
    #[inline]
    pub fn context(&self) -> Ref<'_, MockContext> {
        self.context.borrow()
    }

    /// Update the balance of this canister.
    #[inline]
    pub fn with_balance(self, cycles: u64) -> Self {
        self.context.borrow_mut().update_balance(cycles);
        self
    }

    /// Add the given method to the canister.
    ///
    /// # Panics
    /// If a method with the same name is already defined on the canister.
    #[inline]
    pub fn method<S: Into<String> + Copy>(
        mut self,
        name: S,
        handler: Box<dyn CallHandler>,
    ) -> Self {
        if let Entry::Vacant(o) = self.methods.entry(name.into()) {
            o.insert(handler);
            self
        } else {
            panic!(
                "Method {} already exists on canister {}",
                name.into(),
                &self.id
            );
        }
    }

    /// Add a default handler to the canister.
    ///
    /// # Panics
    /// If a default handler is already set.
    #[inline]
    pub fn or(mut self, handler: Box<dyn CallHandler>) -> Self {
        if self.default.is_some() {
            panic!("Default handler is already set for canister {}", self.id);
        }
        self.default = Some(handler);
        self
    }
}

impl RawHandler {
    /// Create a raw handler.
    #[inline]
    pub fn raw(
        handler: Box<dyn Fn(&mut MockContext, &Vec<u8>, &Principal, &str) -> CallResult<Vec<u8>>>,
    ) -> Self {
        Self { handler }
    }

    /// Create a new handler.
    #[inline]
    pub fn new<
        T: for<'de> ArgumentDecoder<'de>,
        R: ArgumentEncoder,
        F: 'static + Fn(&mut MockContext, T, &Principal, &str) -> CallResult<R>,
    >(
        handler: F,
    ) -> Self {
        Self {
            handler: Box::new(move |ctx, bytes, canister_id, method_name| {
                let args = decode_args(bytes).expect("Failed to decode arguments.");
                handler(ctx, args, canister_id, method_name)
                    .map(|r| encode_args(r).expect("Failed to encode response."))
            }),
        }
    }
}

impl CallHandler for Method {
    #[inline]
    fn accept(&self, _: &Principal, method: &str) -> bool {
        if let Some(name) = &self.name {
            name == method
        } else {
            true
        }
    }

    #[inline]
    fn perform(
        &self,
        _caller: &Principal,
        cycles: u64,
        _canister_id: &Principal,
        _method: &str,
        args_raw: &Vec<u8>,
        ctx: Option<&mut MockContext>,
    ) -> (CallResult<Vec<u8>>, u64) {
        if let Some(expected_args) = &self.expected_args {
            assert_eq!(expected_args, args_raw);
        }

        let mut default_ctx = MockContext::new().with_msg_cycles(cycles);
        let ctx = ctx.unwrap_or(&mut default_ctx);

        for atom in &self.atoms {
            match *atom {
                MethodAtom::ConsumeAllCycles => {
                    ctx.msg_cycles_accept(u64::MAX);
                }
                MethodAtom::ConsumeCycles(cycles) => {
                    ctx.msg_cycles_accept(cycles);
                }
                MethodAtom::RefundCycles(amount) => {
                    let cycles = ctx.msg_cycles_available();
                    if amount > cycles {
                        panic!(
                            "Can not refund {} cycles when only {} cycles is available.",
                            amount, cycles
                        );
                    } else {
                        ctx.msg_cycles_accept(cycles - amount);
                    }
                }
            }
        }

        let refund = ctx.msg_cycles_available();

        if let Some(v) = &self.response {
            (Ok(v.clone()), refund)
        } else {
            (Ok(encode_args(()).unwrap()), refund)
        }
    }
}

impl CallHandler for RawHandler {
    #[inline]
    fn accept(&self, _: &Principal, _: &str) -> bool {
        true
    }

    #[inline]
    fn perform(
        &self,
        caller: &Principal,
        cycles: u64,
        canister_id: &Principal,
        method: &str,
        args_raw: &Vec<u8>,
        ctx: Option<&mut MockContext>,
    ) -> (CallResult<Vec<u8>>, u64) {
        let mut default_ctx = MockContext::new()
            .with_caller(caller.clone())
            .with_msg_cycles(cycles)
            .with_id(canister_id.clone());
        let ctx = ctx.unwrap_or(&mut default_ctx);

        let handler = &self.handler;
        let res = handler(ctx, args_raw, canister_id, method);

        (res, ctx.msg_cycles_available())
    }
}

impl CallHandler for Canister {
    #[inline]
    fn accept(&self, canister_id: &Principal, method: &str) -> bool {
        &self.id == canister_id
            && (self.default.is_some() || {
                let maybe_handler = self.methods.get(method);
                if let Some(handler) = maybe_handler {
                    handler.accept(canister_id, method)
                } else {
                    false
                }
            })
    }

    #[inline]
    fn perform(
        &self,
        caller: &Principal,
        cycles: u64,
        canister_id: &Principal,
        method: &str,
        args_raw: &Vec<u8>,
        ctx: Option<&mut MockContext>,
    ) -> (CallResult<Vec<u8>>, u64) {
        assert!(ctx.is_none());

        let mut ctx = self.context.borrow_mut();
        ctx.update_caller(caller.clone());
        ctx.update_msg_cycles(cycles);

        let res = if let Some(handler) = self.methods.get(method) {
            handler.perform(
                caller,
                cycles,
                canister_id,
                method,
                args_raw,
                Some(&mut ctx),
            )
        } else {
            let handler = self.default.as_ref().unwrap();
            handler.perform(
                caller,
                cycles,
                canister_id,
                method,
                args_raw,
                Some(&mut ctx),
            )
        };

        assert_eq!(res.1, ctx.msg_cycles_available());
        ctx.update_msg_cycles(0);
        res
    }
}
