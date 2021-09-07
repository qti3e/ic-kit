use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::time::{SystemTime, UNIX_EPOCH};

use ic_cdk::export::candid::utils::{ArgumentDecoder, ArgumentEncoder};
use ic_cdk::export::candid::{decode_args, encode_args};
use ic_cdk::export::{candid, Principal};
use serde::Serialize;

use crate::candid::CandidType;
use crate::inject::{get_context, inject};
use crate::interface::{CallResponse, Context};
use crate::{CallHandler, Method};

/// A context that could be used to fake/control the behaviour of the IC when testing the canister.
pub struct MockContext {
    /// The watcher on the context.
    watcher: Watcher,
    /// ID of the current canister.
    id: Principal,
    /// The balance of the canister. By default set to 100TC.
    balance: u64,
    /// The caller principal passed to the calls, by default `anonymous` is used.
    caller: Principal,
    /// Determines if a call was made or not.
    is_reply_callback_mode: bool,
    /// Whatever the canister called trap or not.
    trapped: bool,
    /// Available cycles sent by the caller.
    cycles: u64,
    /// Cycles refunded by the previous call.
    cycles_refunded: u64,
    /// The storage tree for the current context.
    storage: BTreeMap<TypeId, Box<dyn Any>>,
    /// The stable storage data.
    stable: Vec<u8>,
    /// The certified data.
    certified_data: Option<Vec<u8>>,
    /// The certificate certifying the certified_data.
    certificate: Option<Vec<u8>>,
    /// The handlers used to handle inter-canister calls.
    handlers: Vec<Box<dyn CallHandler>>,
}

/// A watcher can be used to inspect the calls made in a call.
pub struct Watcher {
    /// True if the `context.id()` was called during execution.
    pub called_id: bool,
    /// True if the `context.time()` was called during execution.
    pub called_time: bool,
    /// True if the `context.balance()` was called during execution.
    pub called_balance: bool,
    /// True if the `context.caller()` was called during execution.
    pub called_caller: bool,
    /// True if the `context.msg_cycles_available()` was called during execution.
    pub called_msg_cycles_available: bool,
    /// True if the `context.msg_cycles_accept()` was called during execution.
    pub called_msg_cycles_accept: bool,
    /// True if the `context.msg_cycles_refunded()` was called during execution.
    pub called_msg_cycles_refunded: bool,
    /// True if the `context.stable_store()` was called during execution.
    pub called_stable_store: bool,
    /// True if the `context.stable_restore()` was called during execution.
    pub called_stable_restore: bool,
    /// True if the `context.set_certified_data()` was called during execution.
    pub called_set_certified_data: bool,
    /// True if the `context.data_certificate()` was called during execution.
    pub called_data_certificate: bool,
    /// Storage items that were mutated.
    storage_modified: BTreeSet<TypeId>,
    /// List of all the inter canister calls that took place.
    calls: Vec<WatcherCall>,
}

pub struct WatcherCall {
    canister_id: Principal,
    method_name: String,
    args_raw: Vec<u8>,
    cycles_sent: u64,
    cycles_refunded: u64,
}

impl MockContext {
    /// Create a new mock context which could be injected for testing.
    #[inline]
    pub fn new() -> Self {
        Self {
            watcher: Watcher::default(),
            id: Principal::from_text("sgymv-uiaaa-aaaaa-aaaia-cai").unwrap(),
            balance: 100_000_000_000_000,
            caller: Principal::anonymous(),
            is_reply_callback_mode: false,
            trapped: false,
            cycles: 0,
            cycles_refunded: 0,
            storage: BTreeMap::new(),
            stable: Vec::new(),
            certified_data: None,
            certificate: None,
            handlers: vec![],
        }
    }

    /// Reset the current watcher on the MockContext and return a reference to it.
    #[inline]
    pub fn watch(&self) -> &Watcher {
        self.as_mut().watcher = Watcher::default();
        &self.watcher
    }

    /// Set the ID of the canister.
    ///
    /// # Example
    ///
    /// ```
    /// use ic_kit::*;
    ///
    /// let id = Principal::from_text("ai7t5-aibaq-aaaaa-aaaaa-c").unwrap();
    ///
    /// MockContext::new()
    ///     .with_id(id.clone())
    ///     .inject();
    ///
    /// let ic = get_context();
    /// assert_eq!(ic.id(), id);
    /// ```
    #[inline]
    pub fn with_id(mut self, id: Principal) -> Self {
        self.id = id;
        self
    }

    /// Set the balance of the canister.
    ///
    /// # Example
    ///
    /// ```
    /// use ic_kit::*;
    ///
    /// MockContext::new()
    ///     .with_balance(1000)
    ///     .inject();
    ///
    /// let ic = get_context();
    /// assert_eq!(ic.balance(), 1000);
    /// ```
    #[inline]
    pub fn with_balance(mut self, cycles: u64) -> Self {
        self.balance = cycles;
        self
    }

    /// Set the caller for the current call.
    ///
    /// # Example
    ///
    /// ```
    /// use ic_kit::*;
    ///
    /// let alice = Principal::from_text("ai7t5-aibaq-aaaaa-aaaaa-c").unwrap();
    ///
    /// MockContext::new()
    ///     .with_caller(alice.clone())
    ///     .inject();
    ///
    /// let ic = get_context();
    /// assert_eq!(ic.caller(), alice);
    /// ```
    #[inline]
    pub fn with_caller(mut self, caller: Principal) -> Self {
        self.caller = caller;
        self
    }

    /// Make the given amount of cycles available for the call. This amount of cycles will
    /// be deduced if the call accepts them or will be refunded. If the canister accepts any
    /// cycles the balance of the canister will be increased.
    ///
    /// # Example
    ///
    /// ```
    /// use ic_kit::*;
    ///
    /// MockContext::new()
    ///     .with_msg_cycles(1000)
    ///     .inject();
    ///
    /// let ic = get_context();
    /// assert_eq!(ic.msg_cycles_available(), 1000);
    /// ic.msg_cycles_accept(300);
    /// assert_eq!(ic.msg_cycles_available(), 700);
    /// ```
    #[inline]
    pub fn with_msg_cycles(mut self, cycles: u64) -> Self {
        self.cycles = cycles;
        self
    }

    /// Initialize the context with the given value inserted in the storage.
    ///
    /// # Example
    ///
    /// ```
    /// use ic_kit::*;
    ///
    /// MockContext::new()
    ///     .with_data(String::from("Hello"))
    ///     .inject();
    ///
    /// let ic = get_context();
    /// assert_eq!(ic.get::<String>(), &"Hello".to_string());
    /// ```
    #[inline]
    pub fn with_data<T: 'static>(mut self, data: T) -> Self {
        let type_id = std::any::TypeId::of::<T>();
        self.storage.insert(type_id, Box::new(data));
        self
    }

    /// Initialize the context with the given value inserted into the stable storage.
    ///
    /// # Example
    ///
    /// ```
    /// use ic_kit::*;
    ///
    /// MockContext::new()
    ///     .with_stable(("Bella".to_string(), ))
    ///     .inject();
    ///
    /// let ic = get_context();
    /// assert_eq!(ic.stable_restore::<(String, )>(), Ok(("Bella".to_string(), )));
    /// ```
    #[inline]
    pub fn with_stable<T: Serialize>(self, data: T) -> Self
    where
        T: ArgumentEncoder,
    {
        self.stable_store(data)
            .expect("Encoding stable data failed.");
        self
    }

    /// Set the certified data of the canister.
    #[inline]
    pub fn with_certified_data(mut self, data: Vec<u8>) -> Self {
        assert!(data.len() < 32);
        self.certificate = Some(MockContext::sign(data.as_slice()));
        self.certified_data = Some(data);
        self
    }

    /// Creates a mock context with a default handler that accepts the given amount of cycles
    /// on every request.
    #[inline]
    pub fn with_accept_cycles_handler(self, cycles: u64) -> Self {
        self.with_handler(Method::new().cycles_consume(cycles))
    }

    /// Creates a mock context with a default handler that refunds the given amount of cycles
    /// on every request.
    #[inline]
    pub fn with_refund_cycles_handler(self, cycles: u64) -> Self {
        self.with_handler(Method::new().cycles_refund(cycles))
    }

    /// Create a mock context with a default handler that returns the given value.
    #[inline]
    pub fn with_constant_return_handler<T: CandidType>(self, value: T) -> Self {
        self.with_handler(Method::new().response(value))
    }

    /// Add the given handler to the handlers pipeline.
    #[inline]
    pub fn with_handler<T: 'static + CallHandler>(mut self, handler: T) -> Self {
        self.handlers.push(Box::new(handler));
        self
    }

    /// Use this context as the default context for this thread.
    #[inline]
    pub fn inject(self) -> &'static mut Self {
        inject(self);
        get_context()
    }

    /// Sign a data and return the certificate, this is the method used in set_certified_data
    /// to set the data certificate for the given certified data.
    pub fn sign(data: &[u8]) -> Vec<u8> {
        let data = {
            let mut tmp: Vec<u8> = vec![0; 32];
            for (i, b) in data.iter().enumerate() {
                tmp[i] = *b;
            }
            tmp
        };

        let mut certificate = Vec::with_capacity(32 * 8);

        for i in 0..32 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for b in &certificate {
                hasher.write_u8(*b);
            }
            hasher.write_u8(data[i]);
            let hash = hasher.finish().to_be_bytes();
            certificate.extend_from_slice(&hash);
        }

        certificate
    }

    /// This is how we do interior mutability for MockContext. Since the context is only accessible
    /// by only one thread, it is safe to do it here.
    #[inline]
    fn as_mut(&self) -> &mut Self {
        unsafe {
            let const_ptr = self as *const Self;
            let mut_ptr = const_ptr as *mut Self;
            &mut *mut_ptr
        }
    }
}

impl MockContext {
    /// Reset the state after a call.
    #[inline]
    pub fn call_state_reset(&self) {
        let mut_ref = self.as_mut();
        mut_ref.is_reply_callback_mode = false;
        mut_ref.trapped = false;
    }

    /// Clear the storage.
    #[inline]
    pub fn clear_storage(&self) {
        self.as_mut().storage.clear()
    }

    /// Update the balance of the canister.
    #[inline]
    pub fn update_balance(&self, cycles: u64) {
        self.as_mut().balance = cycles;
    }

    /// Update the cycles of the next message.
    #[inline]
    pub fn update_msg_cycles(&self, cycles: u64) {
        self.as_mut().cycles = cycles;
    }

    /// Update the caller for the next message.
    #[inline]
    pub fn update_caller(&self, caller: Principal) {
        self.as_mut().caller = caller;
    }

    /// Return the certified data set on the canister.
    #[inline]
    pub fn get_certified_data(&self) -> Option<Vec<u8>> {
        match &self.certified_data {
            Some(v) => Some(v.clone()),
            None => None,
        }
    }
}

impl Context for MockContext {
    #[inline]
    fn trap(&self, message: &str) -> ! {
        self.as_mut().trapped = true;
        panic!("Canister {} trapped with message: {}", self.id, message);
    }

    #[inline]
    fn print<S: AsRef<str>>(&self, s: S) {
        println!("{} : {}", self.id, s.as_ref())
    }

    #[inline]
    fn id(&self) -> Principal {
        self.as_mut().watcher.called_id = true;
        self.id.clone()
    }

    #[inline]
    fn time(&self) -> u64 {
        self.as_mut().watcher.called_time = true;
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos() as u64
    }

    #[inline]
    fn balance(&self) -> u64 {
        self.as_mut().watcher.called_balance = true;
        self.balance
    }

    #[inline]
    fn caller(&self) -> Principal {
        self.as_mut().watcher.called_caller = true;

        if self.is_reply_callback_mode {
            panic!(
                "Canister {} violated contract: \"{}\" cannot be executed in reply callback mode",
                self.id(),
                "ic0_msg_caller_size"
            )
        }

        self.caller.clone()
    }

    #[inline]
    fn msg_cycles_available(&self) -> u64 {
        self.as_mut().watcher.called_msg_cycles_available = true;
        self.cycles
    }

    #[inline]
    fn msg_cycles_accept(&self, cycles: u64) -> u64 {
        self.as_mut().watcher.called_msg_cycles_accept = true;
        let mut_ref = self.as_mut();
        if cycles > mut_ref.cycles {
            let r = mut_ref.cycles;
            mut_ref.cycles = 0;
            mut_ref.balance += r;
            r
        } else {
            mut_ref.cycles -= cycles;
            mut_ref.balance += cycles;
            cycles
        }
    }

    #[inline]
    fn msg_cycles_refunded(&self) -> u64 {
        self.as_mut().watcher.called_msg_cycles_refunded = true;
        self.cycles_refunded
    }

    #[inline]
    fn store<T: 'static + Default>(&self, data: T) {
        let type_id = TypeId::of::<T>();
        let mut_ref = self.as_mut();
        mut_ref.watcher.storage_modified.insert(type_id);
        mut_ref.storage.insert(type_id, Box::new(data));
    }

    #[inline]
    fn get<T: 'static + Default>(&self) -> &T {
        let type_id = std::any::TypeId::of::<T>();
        self.as_mut()
            .storage
            .entry(type_id)
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut()
            .expect("Unexpected value of invalid type.")
    }

    #[inline]
    fn get_mut<T: 'static + Default>(&self) -> &mut T {
        let type_id = std::any::TypeId::of::<T>();
        let mut_ref = self.as_mut();
        mut_ref.watcher.storage_modified.insert(type_id);
        mut_ref
            .storage
            .entry(type_id)
            .or_insert_with(|| Box::new(T::default()))
            .downcast_mut()
            .expect("Unexpected value of invalid type.")
    }

    #[inline]
    fn delete<T: 'static + Default>(&self) -> bool {
        let type_id = std::any::TypeId::of::<T>();
        let mut_ref = self.as_mut();
        mut_ref.watcher.storage_modified.insert(type_id);
        mut_ref.storage.remove(&type_id).is_some()
    }

    #[inline]
    fn stable_store<T>(&self, data: T) -> Result<(), candid::Error>
    where
        T: ArgumentEncoder,
    {
        let mut_ref = self.as_mut();
        mut_ref.watcher.called_stable_store = true;
        mut_ref.stable = encode_args(data)?;
        Ok(())
    }

    #[inline]
    fn stable_restore<T>(&self) -> Result<T, String>
    where
        T: for<'de> ArgumentDecoder<'de>,
    {
        self.as_mut().watcher.called_stable_restore = true;
        use candid::de::IDLDeserialize;
        let bytes = &self.stable;
        let mut de = IDLDeserialize::new(bytes.as_slice()).map_err(|e| format!("{:?}", e))?;
        let res = ArgumentDecoder::decode(&mut de).map_err(|e| format!("{:?}", e))?;
        // The idea here is to ignore an error that comes from Candid, because we have trailing
        // bytes.
        let _ = de.done();
        Ok(res)
    }

    fn call_raw(
        &'static self,
        id: Principal,
        method: &'static str,
        args_raw: Vec<u8>,
        cycles: u64,
    ) -> CallResponse<Vec<u8>> {
        if cycles > self.balance {
            panic!(
                "Calling canister {} with {} cycles when there is only {} cycles available.",
                id, cycles, self.balance
            );
        }

        let mut_ref = self.as_mut();
        mut_ref.balance -= cycles;
        mut_ref.is_reply_callback_mode = true;

        let mut i = 0;
        let (res, refunded) = loop {
            if i == self.handlers.len() {
                panic!("No handler found to handle the data.")
            }

            let handler = &self.handlers[i];
            i += 1;

            if handler.accept(&id, method) {
                break handler.perform(&self.id, cycles, &id, method, &args_raw, None);
            }
        };

        mut_ref.cycles_refunded = refunded;
        mut_ref.balance += refunded;

        mut_ref.watcher.record_call(WatcherCall {
            canister_id: id,
            method_name: method.to_string(),
            args_raw,
            cycles_sent: cycles,
            cycles_refunded: refunded,
        });

        Box::pin(async move { res })
    }

    #[inline]
    fn set_certified_data(&self, data: &[u8]) {
        if data.len() > 32 {
            panic!("Data certificate has more than 32 bytes.");
        }

        let mut_ref = self.as_mut();
        mut_ref.watcher.called_set_certified_data = true;
        mut_ref.certificate = Some(MockContext::sign(data));
        mut_ref.certified_data = Some(data.to_vec());
    }

    #[inline]
    fn data_certificate(&self) -> Option<Vec<u8>> {
        self.as_mut().watcher.called_data_certificate = true;
        match &self.certificate {
            Some(c) => Some(c.clone()),
            None => None,
        }
    }
}

impl Default for Watcher {
    #[inline]
    fn default() -> Self {
        Watcher {
            called_id: false,
            called_time: false,
            called_balance: false,
            called_caller: false,
            called_msg_cycles_available: false,
            called_msg_cycles_accept: false,
            called_msg_cycles_refunded: false,
            called_stable_store: false,
            called_stable_restore: false,
            called_set_certified_data: false,
            called_data_certificate: false,
            storage_modified: Default::default(),
            calls: Vec::with_capacity(3),
        }
    }
}

impl Watcher {
    /// Push a call to the call history of the watcher.
    #[inline]
    pub fn record_call(&mut self, call: WatcherCall) {
        self.calls.push(call);
    }

    /// Return the number of calls made during the last execution.
    #[inline]
    pub fn call_count(&self) -> usize {
        self.calls.len()
    }

    /// Returns the total amount of cycles consumed in inter-canister calls.
    #[inline]
    pub fn cycles_consumed(&self) -> u64 {
        let mut result = 0;
        for call in &self.calls {
            result += call.cycles_consumed();
        }
        result
    }

    /// Returns the total amount of cycles refunded in inter-canister calls.
    #[inline]
    pub fn cycles_refunded(&self) -> u64 {
        let mut result = 0;
        for call in &self.calls {
            result += call.cycles_refunded();
        }
        result
    }

    /// Returns the total amount of cycles sent in inter-canister calls, not deducing the refunded
    /// amounts.
    #[inline]
    pub fn cycles_sent(&self) -> u64 {
        let mut result = 0;
        for call in &self.calls {
            result += call.cycles_sent();
        }
        result
    }

    /// Return the n-th call that took place during the execution.
    #[inline]
    pub fn get_call(&self, n: usize) -> &WatcherCall {
        &self.calls[n]
    }

    /// Returns true if the given method was called during the execution.
    #[inline]
    pub fn is_method_called(&self, method_name: &str) -> bool {
        for call in &self.calls {
            if call.method_name() == method_name {
                return true;
            }
        }
        false
    }

    /// Returns true if the given canister was called during the execution.
    #[inline]
    pub fn is_canister_called(&self, canister_id: &Principal) -> bool {
        for call in &self.calls {
            if &call.canister_id() == canister_id {
                return true;
            }
        }
        false
    }

    /// Returns true if the given method was called.
    #[inline]
    pub fn is_called(&self, canister_id: &Principal, method_name: &str) -> bool {
        for call in &self.calls {
            if &call.canister_id() == canister_id && call.method_name() == method_name {
                return true;
            }
        }
        false
    }

    /// Returns true if the given storage item was accessed in a mutable way during the execution.
    /// This method tracks calls to:
    /// - context.store()
    /// - context.get_mut()
    /// - context.delete()
    #[inline]
    pub fn is_modified<T: 'static>(&self) -> bool {
        let type_id = std::any::TypeId::of::<T>();
        self.storage_modified.contains(&type_id)
    }
}

impl WatcherCall {
    /// The amount of cycles consumed by this call.
    #[inline]
    pub fn cycles_consumed(&self) -> u64 {
        self.cycles_sent - self.cycles_refunded
    }

    /// The amount of cycles sent to the call.
    #[inline]
    pub fn cycles_sent(&self) -> u64 {
        self.cycles_sent
    }

    /// The amount of cycles refunded from the call.
    #[inline]
    pub fn cycles_refunded(&self) -> u64 {
        self.cycles_refunded
    }

    /// Return the arguments passed to the call.
    #[inline]
    pub fn args<T: for<'de> ArgumentDecoder<'de>>(&self) -> T {
        decode_args(&self.args_raw).expect("Failed to decode arguments.")
    }

    /// Name of the method that was called.
    #[inline]
    pub fn method_name(&self) -> &str {
        &self.method_name
    }

    /// Canister ID that was target of the call.
    #[inline]
    pub fn canister_id(&self) -> Principal {
        self.canister_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use crate::Principal;
    use crate::{Context, MockContext};

    /// A simple canister implementation which helps the testing.
    mod canister {
        use std::collections::BTreeMap;

        use crate::interfaces::management::WithCanisterId;
        use crate::interfaces::*;
        use crate::Context;
        use crate::{get_context, Principal};

        /// An update method that returns the principal id of the caller.
        pub fn whoami() -> Principal {
            let ic = get_context();
            ic.caller()
        }

        /// An update method that returns the principal id of the canister.
        pub fn canister_id() -> Principal {
            let ic = get_context();
            ic.id()
        }

        /// An update method that returns the balance of the canister.
        pub fn balance() -> u64 {
            let ic = get_context();
            ic.balance()
        }

        /// An update method that returns the number of cycles provided by the user in the call.
        pub fn msg_cycles_available() -> u64 {
            let ic = get_context();
            ic.msg_cycles_available()
        }

        /// An update method that accepts the given number of cycles from the caller, the number of
        /// accepted cycles is returned.
        pub fn msg_cycles_accept(cycles: u64) -> u64 {
            let ic = get_context();
            ic.msg_cycles_accept(cycles)
        }

        pub type Counter = BTreeMap<u64, i64>;

        /// An update method that increments one to the given key, the new value is returned.
        pub fn increment(key: u64) -> i64 {
            let ic = get_context();
            let count = ic.get_mut::<Counter>().entry(key).or_insert(0);
            *count += 1;
            *count
        }

        /// An update method that decrement one from the given key. The new value is returned.
        pub fn decrement(key: u64) -> i64 {
            let ic = get_context();
            let count = ic.get_mut::<Counter>().entry(key).or_insert(0);
            *count -= 1;
            *count
        }

        pub async fn withdraw(canister_id: Principal, amount: u64) -> Result<(), String> {
            let ic = get_context();
            let user_balance = ic.get_mut::<u64>();

            if amount > *user_balance {
                return Err(format!("Insufficient balance."));
            }

            *user_balance -= amount;

            match management::DepositCycles::perform_with_payment(
                ic,
                Principal::management_canister(),
                (WithCanisterId { canister_id },),
                amount,
            )
            .await
            {
                Ok(()) => {
                    *user_balance += ic.msg_cycles_refunded();
                    Ok(())
                }
                Err((code, msg)) => {
                    assert_eq!(amount, ic.msg_cycles_refunded());
                    *user_balance += amount;
                    Err(format!(
                        "An error happened during the call: {}: {}",
                        code as u8, msg
                    ))
                }
            }
        }

        pub fn user_balance() -> u64 {
            let ic = get_context();
            *ic.get::<u64>()
        }

        pub fn pre_upgrade() {
            let ic = get_context();
            let map = ic.get::<Counter>();
            ic.stable_store((map,))
                .expect("Failed to write to stable storage");
        }

        pub fn post_upgrade() {
            let ic = get_context();
            if let Ok((map,)) = ic.stable_restore() {
                ic.store::<Counter>(map);
            }
        }

        pub fn set_certified_data(data: &[u8]) {
            let ic = get_context();
            ic.set_certified_data(data);
        }

        pub fn data_certificate() -> Option<Vec<u8>> {
            let ic = get_context();
            ic.data_certificate()
        }
    }

    /// Some mock principal ids.
    mod users {
        use crate::Principal;

        pub fn bob() -> Principal {
            Principal::from_text("ai7t5-aibaq-aaaaa-aaaaa-c").unwrap()
        }

        pub fn john() -> Principal {
            Principal::from_text("hozae-racaq-aaaaa-aaaaa-c").unwrap()
        }
    }

    #[test]
    fn test_with_id() {
        let ctx = MockContext::new()
            .with_id(Principal::management_canister())
            .inject();
        let watcher = ctx.watch();

        assert_eq!(canister::canister_id(), Principal::management_canister());
        assert!(watcher.called_id);
    }

    #[test]
    fn test_balance() {
        let ctx = MockContext::new().with_balance(1000).inject();
        let watcher = ctx.watch();

        assert_eq!(canister::balance(), 1000);
        assert!(watcher.called_balance);

        ctx.update_balance(2000);
        assert_eq!(canister::balance(), 2000);
    }

    #[test]
    fn test_caller() {
        let ctx = MockContext::new().with_caller(users::john()).inject();
        let watcher = ctx.watch();

        assert_eq!(canister::whoami(), users::john());
        assert!(watcher.called_caller);

        ctx.update_caller(users::bob());
        assert_eq!(canister::whoami(), users::bob());
    }

    #[test]
    fn test_msg_cycles() {
        let ctx = MockContext::new().with_msg_cycles(1000).inject();
        let watcher = ctx.watch();

        assert_eq!(canister::msg_cycles_available(), 1000);
        assert!(watcher.called_msg_cycles_available);

        ctx.update_msg_cycles(50);
        assert_eq!(canister::msg_cycles_available(), 50);
    }

    #[test]
    fn test_msg_cycles_accept() {
        let ctx = MockContext::new()
            .with_msg_cycles(1000)
            .with_balance(240)
            .inject();
        let watcher = ctx.watch();

        assert_eq!(canister::msg_cycles_accept(100), 100);
        assert!(watcher.called_msg_cycles_accept);
        assert_eq!(ctx.msg_cycles_available(), 900);
        assert_eq!(ctx.balance(), 340);

        ctx.update_msg_cycles(50);
        assert_eq!(canister::msg_cycles_accept(100), 50);
        assert_eq!(ctx.msg_cycles_available(), 0);
        assert_eq!(ctx.balance(), 390);
    }

    #[test]
    fn test_storage_simple() {
        let ctx = MockContext::new().inject();
        let watcher = ctx.watch();
        assert_eq!(watcher.is_modified::<canister::Counter>(), false);
        assert_eq!(canister::increment(0), 1);
        assert_eq!(watcher.is_modified::<canister::Counter>(), true);
        assert_eq!(canister::increment(0), 2);
        assert_eq!(canister::increment(0), 3);
        assert_eq!(canister::increment(1), 1);
        assert_eq!(canister::decrement(0), 2);
        assert_eq!(canister::decrement(2), -1);
    }

    #[test]
    fn test_storage() {
        let ctx = MockContext::new()
            .with_data({
                let mut map = canister::Counter::default();
                map.insert(0, 12);
                map.insert(1, 17);
                map
            })
            .inject();
        assert_eq!(canister::increment(0), 13);
        assert_eq!(canister::decrement(1), 16);

        let watcher = ctx.watch();
        assert_eq!(watcher.is_modified::<canister::Counter>(), false);
        ctx.store({
            let mut map = canister::Counter::default();
            map.insert(0, 12);
            map.insert(1, 17);
            map
        });
        assert_eq!(watcher.is_modified::<canister::Counter>(), true);

        assert_eq!(canister::increment(0), 13);
        assert_eq!(canister::decrement(1), 16);

        ctx.clear_storage();

        assert_eq!(canister::increment(0), 1);
        assert_eq!(canister::decrement(1), -1);
    }

    #[test]
    fn stable_storage() {
        let ctx = MockContext::new()
            .with_data({
                let mut map = canister::Counter::default();
                map.insert(0, 2);
                map.insert(1, 27);
                map.insert(2, 5);
                map.insert(3, 17);
                map
            })
            .inject();

        let watcher = ctx.watch();

        canister::pre_upgrade();
        assert!(watcher.called_stable_store);
        ctx.clear_storage();
        canister::post_upgrade();
        assert!(watcher.called_stable_restore);

        let counter = ctx.get::<canister::Counter>();
        let data: Vec<(u64, i64)> = counter
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        assert_eq!(data, vec![(0, 2), (1, 27), (2, 5), (3, 17)]);

        assert_eq!(canister::increment(0), 3);
        assert_eq!(canister::decrement(1), 26);
    }

    #[test]
    fn certified_data() {
        let ctx = MockContext::new()
            .with_certified_data(vec![0, 1, 2, 3, 4, 5])
            .inject();
        let watcher = ctx.watch();

        assert_eq!(ctx.get_certified_data(), Some(vec![0, 1, 2, 3, 4, 5]));
        assert_eq!(
            ctx.data_certificate(),
            Some(MockContext::sign(&[0, 1, 2, 3, 4, 5]))
        );

        canister::set_certified_data(&[1, 2, 3]);
        assert_eq!(watcher.called_set_certified_data, true);
        assert_eq!(ctx.get_certified_data(), Some(vec![1, 2, 3]));
        assert_eq!(ctx.data_certificate(), Some(MockContext::sign(&[1, 2, 3])));

        canister::data_certificate();
        assert_eq!(watcher.called_data_certificate, true);
    }

    #[async_std::test]
    async fn withdraw_accept() {
        let ctx = MockContext::new()
            .with_accept_cycles_handler(200)
            .with_data(1000u64)
            .with_balance(2000)
            .inject();
        let watcher = ctx.watch();

        assert_eq!(canister::user_balance(), 1000);

        assert_eq!(
            watcher.is_canister_called(&Principal::management_canister()),
            false
        );
        assert_eq!(watcher.is_method_called("deposit_cycles"), false);
        assert_eq!(
            watcher.is_called(&Principal::management_canister(), "deposit_cycles"),
            false
        );
        assert_eq!(watcher.cycles_consumed(), 0);

        canister::withdraw(users::bob(), 100).await.unwrap();

        assert_eq!(watcher.call_count(), 1);
        assert_eq!(
            watcher.is_canister_called(&Principal::management_canister()),
            true
        );
        assert_eq!(watcher.is_method_called("deposit_cycles"), true);
        assert_eq!(
            watcher.is_called(&Principal::management_canister(), "deposit_cycles"),
            true
        );
        assert_eq!(watcher.cycles_consumed(), 100);

        // The user balance needs to be decremented.
        assert_eq!(canister::user_balance(), 900);
        // The canister balance needs to be decremented.
        assert_eq!(canister::balance(), 1900);
    }

    #[async_std::test]
    async fn withdraw_accept_portion() {
        let ctx = MockContext::new()
            .with_accept_cycles_handler(50)
            .with_data(1000u64)
            .with_balance(2000)
            .inject();
        let watcher = ctx.watch();

        assert_eq!(canister::user_balance(), 1000);

        canister::withdraw(users::bob(), 100).await.unwrap();
        assert_eq!(watcher.cycles_sent(), 100);
        assert_eq!(watcher.cycles_consumed(), 50);
        assert_eq!(watcher.cycles_refunded(), 50);

        // The user balance needs to be decremented.
        assert_eq!(canister::user_balance(), 950);
        // The canister balance needs to be decremented.
        assert_eq!(canister::balance(), 1950);
    }

    #[async_std::test]
    async fn withdraw_accept_zero() {
        let ctx = MockContext::new()
            .with_accept_cycles_handler(0)
            .with_data(1000u64)
            .with_balance(2000)
            .inject();
        let watcher = ctx.watch();

        assert_eq!(canister::user_balance(), 1000);

        canister::withdraw(users::bob(), 100).await.unwrap();
        assert_eq!(watcher.cycles_sent(), 100);
        assert_eq!(watcher.cycles_consumed(), 0);
        assert_eq!(watcher.cycles_refunded(), 100);

        // The balance should not be decremented.
        assert_eq!(canister::user_balance(), 1000);
        assert_eq!(canister::balance(), 2000);
    }

    #[async_std::test]
    async fn with_refund() {
        let ctx = MockContext::new()
            .with_refund_cycles_handler(30)
            .with_data(1000u64)
            .with_balance(2000)
            .inject();
        let watcher = ctx.watch();

        canister::withdraw(users::bob(), 100).await.unwrap();
        assert_eq!(watcher.cycles_sent(), 100);
        assert_eq!(watcher.cycles_consumed(), 70);
        assert_eq!(watcher.cycles_refunded(), 30);
        assert_eq!(canister::user_balance(), 930);
        assert_eq!(canister::balance(), 1930);
    }
}
