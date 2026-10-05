use core::{cell::Cell, fmt, marker::PhantomData};

use alloc::string::String;

use serde::{
    Deserialize, Deserializer,
    de::{
        self, DeserializeSeed, MapAccess, Visitor,
        value::{BorrowedStrDeserializer, MapAccessDeserializer},
    },
};

use super::Call;

mod empty_params;
use empty_params::{EmptyParamsSeed, SyntheticMethodMap};

impl<'de, M> Deserialize<'de> for Call<M>
where
    M: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CallVisitor<M>(PhantomData<M>);

        impl<'de, M> Visitor<'de> for CallVisitor<M>
        where
            M: Deserialize<'de>,
        {
            type Value = Call<M>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(
                    f,
                    "a map with optional booleans and flattened method fields"
                )
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                // The deserializer streams the outer map once, capturing the `oneway`/`more`/
                // `upgrade` flags into cells and forwarding the method/parameters to `M`. The cells
                // below also track whether the empty-params fallback (re-deserializing `M` from a
                // synthetic `{method, parameters: {}}` map) is needed; see `empty_params`.
                let oneway_cell = Cell::new(None::<bool>);
                let more_cell = Cell::new(None::<bool>);
                let upgrade_cell = Cell::new(None::<bool>);
                // Set when an empty/null `parameters` value was forwarded as `visit_unit()`.
                let needs_retry = Cell::new(false);
                // Set once the `parameters` key is seen at all; its *absence* for a struct variant
                // is the other case the fallback handles.
                let saw_parameters = Cell::new(false);
                // Set if a flag (`oneway`/`more`/`upgrade`) value failed to parse. Such an error is
                // a genuine envelope error that must never be masked by the fallback.
                let flag_error = Cell::new(false);
                // Captures the method name string so the fallback can rebuild the synthetic map.
                let method_capture: Cell<Option<String>> = Cell::new(None);

                // Streaming adapter capturing the flags by Cell refs while forwarding the
                // method/parameters entries to `M`.
                struct FilterMap<'a, MAcc> {
                    inner: MAcc,
                    oneway: &'a Cell<Option<bool>>,
                    more: &'a Cell<Option<bool>>,
                    upgrade: &'a Cell<Option<bool>>,
                    needs_retry: &'a Cell<bool>,
                    saw_parameters: &'a Cell<bool>,
                    flag_error: &'a Cell<bool>,
                    method_capture: &'a Cell<Option<String>>,
                    // True when the key just surfaced to the inner method enum was
                    // `parameters`, so the matching `next_value_seed` knows to apply the
                    // empty-object workaround (see `empty_params`).
                    at_parameters: bool,
                    // True when the key just surfaced was `method`, so we can capture its value.
                    capture_method: bool,
                }
                impl<'de, 'a, MAcc> FilterMap<'a, MAcc>
                where
                    MAcc: MapAccess<'de>,
                {
                    /// Read a boolean flag value, recording a `flag_error` if it is malformed so
                    /// the fallback never masks a genuine envelope error.
                    fn flag_value(&mut self) -> Result<bool, MAcc::Error> {
                        self.inner.next_value().inspect_err(|_| {
                            self.flag_error.set(true);
                        })
                    }
                }
                impl<'de, 'a, MAcc> MapAccess<'de> for FilterMap<'a, MAcc>
                where
                    MAcc: MapAccess<'de>,
                {
                    type Error = MAcc::Error;

                    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, MAcc::Error>
                    where
                        K: DeserializeSeed<'de>,
                    {
                        self.at_parameters = false;
                        self.capture_method = false;
                        while let Some(key) = self.inner.next_key::<&'de str>()? {
                            match key {
                                "oneway" => {
                                    let v = self.flag_value()?;
                                    self.oneway.set(Some(v));
                                    continue;
                                }
                                "more" => {
                                    let v = self.flag_value()?;
                                    self.more.set(Some(v));
                                    continue;
                                }
                                "upgrade" => {
                                    let v = self.flag_value()?;
                                    self.upgrade.set(Some(v));
                                    continue;
                                }
                                "method" => {
                                    self.capture_method = true;
                                    let de = BorrowedStrDeserializer::new(key);
                                    return seed.deserialize(de).map(Some);
                                }
                                other => {
                                    // Remember whether this is the `parameters` key so the
                                    // following `next_value_seed` can normalise an empty or
                                    // null value into an absent one (see `empty_params`).
                                    self.at_parameters = other == "parameters";
                                    if self.at_parameters {
                                        self.saw_parameters.set(true);
                                    }
                                    let de = BorrowedStrDeserializer::new(other);
                                    return seed.deserialize(de).map(Some);
                                }
                            }
                        }
                        Ok(None)
                    }

                    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, MAcc::Error>
                    where
                        V: DeserializeSeed<'de>,
                    {
                        if core::mem::take(&mut self.capture_method) {
                            // Capture the method value as an owned String in case the fallback
                            // needs it, then re-feed it to the seed via a StrDeserializer.
                            let method_str: String = self.inner.next_value()?;
                            let result = seed.deserialize(serde::de::value::StrDeserializer::<
                                MAcc::Error,
                            >::new(
                                method_str.as_str()
                            ));
                            self.method_capture.set(Some(method_str));
                            result
                        } else if core::mem::take(&mut self.at_parameters) {
                            // Wrap the inner method enum's content seed so that an empty object
                            // (`{}`) or `null` is forwarded as `visit_unit()`. `EmptyParamsSeed`
                            // sets `needs_retry` when it does so, marking this call as eligible for
                            // the empty-params fallback if it fails.
                            let result = self
                                .inner
                                .next_value_seed(EmptyParamsSeed::new(seed, self.needs_retry));

                            // When empty params were forwarded as a unit but the method is actually
                            // a struct variant, the inner deserialize errors mid-stream (and
                            // `needs_retry` is set). Drain the rest of the outer map so the
                            // underlying deserializer is left in a clean state and any trailing
                            // `oneway`/`more`/`upgrade` flag is still captured for the fallback.
                            // The drain is strict: a malformed flag
                            // value records `flag_error` so the
                            // fallback cannot mask it.
                            if result.is_err() && self.needs_retry.get() {
                                loop {
                                    match self.inner.next_key::<&'de str>() {
                                        Ok(Some(key)) => {
                                            let drained = match key {
                                                "oneway" => self.flag_value().map(|v| {
                                                    self.oneway.set(Some(v));
                                                }),
                                                "more" => self.flag_value().map(|v| {
                                                    self.more.set(Some(v));
                                                }),
                                                "upgrade" => self.flag_value().map(|v| {
                                                    self.upgrade.set(Some(v));
                                                }),
                                                _ => self
                                                    .inner
                                                    .next_value::<de::IgnoredAny>()
                                                    .map(|_| ()),
                                            };
                                            if drained.is_err() {
                                                self.flag_error.set(true);
                                                break;
                                            }
                                        }
                                        Ok(None) => break,
                                        Err(_) => {
                                            self.flag_error.set(true);
                                            break;
                                        }
                                    }
                                }
                            }

                            result
                        } else {
                            self.inner.next_value_seed(seed)
                        }
                    }
                }

                // Deserialize M using the streaming FilterMap.
                let filter = FilterMap {
                    inner: map,
                    oneway: &oneway_cell,
                    more: &more_cell,
                    upgrade: &upgrade_cell,
                    needs_retry: &needs_retry,
                    saw_parameters: &saw_parameters,
                    flag_error: &flag_error,
                    method_capture: &method_capture,
                    at_parameters: false,
                    capture_method: false,
                };
                let streamed =
                    M::deserialize(MapAccessDeserializer::new(filter)).map_err(de::Error::custom);

                // The empty-params fallback runs when the streaming deserialize failed and the
                // parameters were either empty/null (`needs_retry`) or entirely absent
                // (`!saw_parameters`). Both reduce to "no arguments", which an all-optional struct
                // variant accepts as an empty map. The captured method name is the safety
                // interlock: a required-field struct variant still errors against the synthetic
                // empty map, so the fallback never invents data.
                //
                // A malformed flag (`flag_error`) is a genuine envelope error, so it suppresses the
                // fallback and lets the streaming error surface instead of being masked.
                let use_fallback = streamed.is_err()
                    && !flag_error.get()
                    && (needs_retry.get() || !saw_parameters.get());
                let method = if use_fallback {
                    // Re-deserialize M from a synthetic `{"method": <captured>, "parameters": {}}`
                    // map, which presents `parameters` as an empty map so all-optional struct
                    // variants populate their fields with `None`.
                    if let Some(method_str) = method_capture.take() {
                        M::deserialize(MapAccessDeserializer::new(
                            SyntheticMethodMap::<A::Error>::new(&method_str),
                        ))
                        .map_err(de::Error::custom)?
                    } else {
                        // No method captured; propagate the original error.
                        streamed?
                    }
                } else {
                    streamed?
                };

                // Extract the captured flags, defaulting absent ones to `false`.
                let oneway = oneway_cell.get().unwrap_or_default();
                let more = more_cell.get().unwrap_or_default();
                let upgrade = upgrade_cell.get().unwrap_or_default();

                Ok(Call {
                    method,
                    oneway,
                    more,
                    upgrade,
                })
            }
        }

        deserializer.deserialize_map(CallVisitor(PhantomData))
    }
}
