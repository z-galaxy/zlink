// We manually implement `Serialize` and `Deserialize` for `Call` because we need to flatten the
// `method` field and `serde` requires `alloc` for both serialization and deserialization when
// using the `flatten` attribute.
mod de;
#[doc(hidden)]
pub mod parameters;
mod ser;

#[cfg(test)]
mod tests;

/// A method call.
#[derive(Debug, Clone)]
pub struct Call<M> {
    pub(super) method: M,
    pub(super) oneway: bool,
    pub(super) more: bool,
    pub(super) upgrade: bool,
}

impl<M> Call<M> {
    /// Create a new method call.
    pub fn new(method: M) -> Self {
        Self {
            method,
            oneway: false,
            more: false,
            upgrade: false,
        }
    }

    /// Set the oneway flag.
    pub fn set_oneway(mut self, oneway: bool) -> Self {
        self.oneway = oneway;
        self
    }

    /// Set the more flag.
    pub fn set_more(mut self, more: bool) -> Self {
        self.more = more;
        self
    }

    /// Set the upgrade flag.
    pub fn set_upgrade(mut self, upgrade: bool) -> Self {
        self.upgrade = upgrade;
        self
    }

    /// The method call name and parameters.
    pub fn method(&self) -> &M {
        &self.method
    }

    /// If the method call doesn't want a reply.
    pub fn oneway(&self) -> bool {
        self.oneway
    }

    /// If the method call is requesting more replies.
    pub fn more(&self) -> bool {
        self.more
    }

    /// If the method call is requesting an upgrade to a different protocol.
    pub fn upgrade(&self) -> bool {
        self.upgrade
    }
}

impl<M> From<M> for Call<M> {
    fn from(method: M) -> Self {
        Self::new(method)
    }
}
