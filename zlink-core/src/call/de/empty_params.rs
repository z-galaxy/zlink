//! Workaround for serde#2045 on the `parameters` field of a method [`Call`].
//!
//! Varlink no-argument methods are unit variants of an adjacently-tagged enum
//! (`#[serde(tag = "method", content = "parameters")]`). serde refuses to deserialize an empty
//! object (`{}`) into a unit variant, even though an absent `parameters` key works fine. Real
//! clients such as `varlinkctl` send `{"method":"...","parameters":{}}`, so we must accept it.
//!
//! For all-optional-field struct variants (e.g. `List { name: Option<String>, … }`) the opposite
//! is true: `{}` must deserialize as a map so all fields receive `None`. `null`/absent works fine.
//!
//! These two requirements are mutually exclusive under a single serde pass. `serde_derive` routes
//! the `content` payload of *both* unit and struct variants through the type-erased
//! `Deserializer::deserialize_any`, so this adapter has no static signal for which shape the
//! variant's visitor expects, and the visitor is consumed by value (so it cannot be tried twice).
//! Yielding `visit_unit()` fails struct variants; yielding `visit_map(empty)` fails unit variants.
//!
//! The resolution (driven from [`de`]) has two paths:
//!
//! * **Streaming path**: empty `{}`, `null`, or absent `parameters` is forwarded as `visit_unit()`,
//!   which satisfies unit variants. Non-empty objects are forwarded verbatim, preserving zero-copy
//!   `&'de` borrows in field values.
//! * **Empty-params fallback**: only when the streaming path fails *and* the parameters were
//!   empty/null/absent, `M` is re-deserialized from a synthetic map `{"method": <captured>,
//!   "parameters": {}}` whose parameters value is an empty map access. This satisfies all-optional
//!   struct variants, which need `visit_map(empty)` to fill every field with `None`. The captured
//!   method name acts as a safety interlock: a struct variant with a required field still errors
//!   against the empty map, so the fallback never invents data.
//!
//! The items in this module implement the streaming path (`EmptyParamsSeed` →
//! `EmptyParamsDeserializer` → `PeekVisitor`) and the fallback's empty map
//! (`SyntheticMethodMap` → `EmptyMapAccess`).
//!
//! [`de`]: super
//! [`Call`]: super::Call

use core::{cell::Cell, fmt, marker::PhantomData};

use alloc::borrow::Cow;

use serde::de::{
    DeserializeSeed, Deserializer, IntoDeserializer, MapAccess, Visitor,
    value::{BorrowedStrDeserializer, MapAccessDeserializer, StrDeserializer},
};

// ── EmptyParamsSeed ──────────────────────────────────────────────────────────

/// A [`DeserializeSeed`] that wraps the inner content seed with [`EmptyParamsDeserializer`].
///
/// The `needs_retry` cell is set to `true` when an empty/null parameters value is encountered so
/// the caller knows to run the empty-params fallback if this deserialization fails.
pub(super) struct EmptyParamsSeed<'r, S> {
    inner: S,
    needs_retry: &'r Cell<bool>,
}

impl<'r, S> EmptyParamsSeed<'r, S> {
    pub(super) fn new(inner: S, needs_retry: &'r Cell<bool>) -> Self {
        Self { inner, needs_retry }
    }
}

impl<'de, 'r, S> DeserializeSeed<'de> for EmptyParamsSeed<'r, S>
where
    S: DeserializeSeed<'de>,
{
    type Value = S::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.inner.deserialize(EmptyParamsDeserializer {
            inner: deserializer,
            needs_retry: self.needs_retry,
        })
    }
}

// ── EmptyParamsDeserializer ──────────────────────────────────────────────────

/// Forwards to the wrapped deserializer but wraps the visitor so empty/null is forwarded as
/// `visit_unit()` (satisfying unit variants). Sets `needs_retry` when it does so, signalling that
/// the empty-params fallback should run if this deserialization fails.
struct EmptyParamsDeserializer<'r, D> {
    inner: D,
    needs_retry: &'r Cell<bool>,
}

impl<'de, 'r, D> Deserializer<'de> for EmptyParamsDeserializer<'r, D>
where
    D: Deserializer<'de>,
{
    type Error = D::Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.inner.deserialize_any(PeekVisitor {
            inner: visitor,
            needs_retry: self.needs_retry,
        })
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.inner.deserialize_option(PeekVisitor {
            inner: visitor,
            needs_retry: self.needs_retry,
        })
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.inner.deserialize_map(PeekVisitor {
            inner: visitor,
            needs_retry: self.needs_retry,
        })
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.inner.deserialize_struct(
            name,
            fields,
            PeekVisitor {
                inner: visitor,
                needs_retry: self.needs_retry,
            },
        )
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct newtype_struct seq tuple tuple_struct
        enum identifier ignored_any
    }
}

// ── PeekVisitor ──────────────────────────────────────────────────────────────

/// Wraps the real content visitor.
///
/// * Empty map / `null` / `none` → `visit_unit()` (the unit-variant path). Also sets `needs_retry`
///   so the empty-params fallback can run if this fails.
/// * Non-empty map → peeked key restored, forwarded verbatim (preserves zero-copy borrows).
/// * `visit_some` → unwrap and re-apply peek logic.
struct PeekVisitor<'r, V> {
    inner: V,
    needs_retry: &'r Cell<bool>,
}

impl<'de, 'r, V> Visitor<'de> for PeekVisitor<'r, V>
where
    V: Visitor<'de>,
{
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.expecting(f)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        // Peek only the first key to decide what to do.
        //
        // * Empty map → set `needs_retry`, forward as `visit_unit()` (unit variants accept this;
        //   struct variants error here and recover via the empty-params fallback).
        // * Non-empty map → restore the peeked key and forward the full map intact so that `&'de`
        //   borrows in field *values* are not consumed.
        match map.next_key::<Cow<'de, str>>()? {
            None => {
                self.needs_retry.set(true);
                self.inner.visit_unit()
            }
            Some(first_key) => self.inner.visit_map(PrefixedMap {
                first_key: Some(first_key),
                rest: map,
            }),
        }
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // A bare `null` content arrives here via `deserialize_any` (serde_json calls `visit_unit`,
        // not `visit_none`, outside of `deserialize_option`). Treat it like empty parameters: set
        // `needs_retry` so an all-optional struct variant can recover via the synthetic-map retry.
        self.needs_retry.set(true);
        self.inner.visit_unit()
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        // `null` → treat as absent → visit_unit (same as no parameters key).
        self.needs_retry.set(true);
        self.inner.visit_unit()
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Unwrap the option and re-apply the peek logic to the inner value.
        deserializer.deserialize_any(self)
    }
}

// ── EmptyMapAccess ───────────────────────────────────────────────────────────

/// A [`MapAccess`] that is always empty (no entries).
///
/// Used by the empty-params fallback to present `parameters: {}` as an empty map visit so that
/// all-optional-field struct variants can populate every field with `None`.
pub(super) struct EmptyMapAccess<E> {
    _error: PhantomData<E>,
}

impl<E> EmptyMapAccess<E> {
    pub(super) fn new() -> Self {
        Self {
            _error: PhantomData,
        }
    }
}

impl<'de, E> MapAccess<'de> for EmptyMapAccess<E>
where
    E: serde::de::Error,
{
    type Error = E;

    fn next_key_seed<K>(&mut self, _seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        Ok(None)
    }

    fn next_value_seed<Vv>(&mut self, _seed: Vv) -> Result<Vv::Value, Self::Error>
    where
        Vv: DeserializeSeed<'de>,
    {
        // Should never be called since `next_key_seed` always returns `None`.
        Err(E::custom("EmptyMapAccess: next_value called unexpectedly"))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(0)
    }
}

// ── PrefixedMap ──────────────────────────────────────────────────────────────

/// A [`MapAccess`] that yields one cached key before delegating to the underlying map.
///
/// Only the key is cached; the matching value is still pending in `rest`, so a single unconditional
/// delegation to `rest.next_value_seed` is correct for both the cached first entry and the rest.
struct PrefixedMap<'de, A> {
    first_key: Option<Cow<'de, str>>,
    rest: A,
}

impl<'de, A> MapAccess<'de> for PrefixedMap<'de, A>
where
    A: MapAccess<'de>,
{
    type Error = A::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.first_key.take() {
            // Preserve the `&'de` borrow when the key came from self-describing input.
            Some(Cow::Borrowed(key)) => seed
                .deserialize(BorrowedStrDeserializer::new(key))
                .map(Some),
            // Buffered-content replay path: the key is owned.
            Some(Cow::Owned(key)) => seed
                .deserialize(StrDeserializer::new(key.as_str()))
                .map(Some),
            None => self.rest.next_key_seed(seed),
        }
    }

    fn next_value_seed<Vv>(&mut self, seed: Vv) -> Result<Vv::Value, Self::Error>
    where
        Vv: DeserializeSeed<'de>,
    {
        self.rest.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.rest
            .size_hint()
            .map(|n| n + usize::from(self.first_key.is_some()))
    }
}

// ── SyntheticMethodMap ────────────────────────────────────────────────────────

/// A [`MapAccess`] used by the empty-params fallback: yields exactly two entries,
/// `"method" → <captured_method_str>` and `"parameters" → <EmptyMapAccess>`,
/// so that all-optional struct variants receive a `visit_map(empty)` for parameters.
pub(super) struct SyntheticMethodMap<'a, E> {
    state: SyntheticState,
    method: &'a str,
    _error: PhantomData<E>,
}

enum SyntheticState {
    Method,
    Parameters,
    Done,
}

impl<'a, E> SyntheticMethodMap<'a, E> {
    pub(super) fn new(method: &'a str) -> Self {
        Self {
            state: SyntheticState::Method,
            method,
            _error: PhantomData,
        }
    }
}

impl<'de, 'a, E> MapAccess<'de> for SyntheticMethodMap<'a, E>
where
    E: serde::de::Error,
{
    type Error = E;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.state {
            SyntheticState::Method => seed
                .deserialize(StrDeserializer::<E>::new("method"))
                .map(Some),
            SyntheticState::Parameters => seed
                .deserialize(StrDeserializer::<E>::new("parameters"))
                .map(Some),
            SyntheticState::Done => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        match self.state {
            SyntheticState::Method => {
                self.state = SyntheticState::Parameters;
                seed.deserialize(self.method.into_deserializer())
            }
            SyntheticState::Parameters => {
                self.state = SyntheticState::Done;
                seed.deserialize(MapAccessDeserializer::new(EmptyMapAccess::<E>::new()))
            }
            SyntheticState::Done => Err(E::custom("SyntheticMethodMap: exhausted")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        match self.state {
            SyntheticState::Method => Some(2),
            SyntheticState::Parameters => Some(1),
            SyntheticState::Done => Some(0),
        }
    }
}
