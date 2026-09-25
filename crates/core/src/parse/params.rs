//! Typed accessors for legacy XML parameters, and the serde bridge that fills a
//! plugin's parameter struct from them.

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::de::value::{SeqDeserializer, StrDeserializer};
use serde::de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, MapAccess, Visitor};
use std::collections::{BTreeMap, btree_map};
use std::fmt;

pub type Params = BTreeMap<String, String>;

/// Deserializes a plugin's `#[derive(Deserialize)]` parameter struct.
///
/// Every mission value is text. Numbers must be finite, booleans are `true`,
/// `false`, `1`, or `0`, and lists (`Vec` or fixed arrays) are separated by
/// commas or whitespace, so `"1, 0.01, 2, 9"` fills a `[f64; 4]`. A plugin that
/// takes no parameters can deserialize `()`, which rejects any key.
pub(crate) fn deserialize<T: DeserializeOwned>(params: &Params) -> Result<T> {
    T::deserialize(Entries {
        entries: params.iter(),
        value: None,
    })
    .map_err(|error| anyhow!(error.0))
}

#[derive(Debug)]
struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// The whole parameter map, read as a struct.
struct Entries<'a> {
    entries: btree_map::Iter<'a, String, String>,
    value: Option<(&'a str, &'a str)>,
}

impl<'de> de::Deserializer<'de> for Entries<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_map(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(mut self, visitor: V) -> Result<V::Value, Error> {
        match self.next_key_seed(std::marker::PhantomData::<String>)? {
            Some(key) => Err(Error(format!(
                "unknown parameter `{key}`; this plugin takes no parameters"
            ))),
            None => visitor.visit_unit(),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit_struct newtype_struct seq tuple tuple_struct map struct enum
        identifier ignored_any
    }
}

impl<'de> MapAccess<'de> for Entries<'_> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, text)) = self.entries.next() else {
            return Ok(None);
        };
        self.value = Some((key, text));
        let key: StrDeserializer<'_, Error> = key.as_str().into_deserializer();
        seed.deserialize(key).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let (key, text) = self.value.take().expect("value follows its key");
        seed.deserialize(Text(text))
            .map_err(|error| Error(format!("{key}: {error}")))
    }
}

/// One mission value, parsed as whatever type its field asks for.
struct Text<'a>(&'a str);

impl Text<'_> {
    fn number<T: std::str::FromStr>(&self) -> Result<T, Error> {
        self.0
            .parse()
            .map_err(|_| Error(format!("expected number, got {:?}", self.0)))
    }

    /// Parses as the field's own float type, so `1e100` is rejected for an `f32`.
    fn finite<T: std::str::FromStr + Copy + Into<f64>>(&self) -> Result<T, Error> {
        let value: T = self.number()?;
        if value.into().is_finite() {
            Ok(value)
        } else {
            Err(Error(format!("expected finite number, got {:?}", self.0)))
        }
    }
}

impl<'a> IntoDeserializer<'_, Error> for Text<'a> {
    type Deserializer = Self;
    fn into_deserializer(self) -> Self {
        self
    }
}

impl<'de> de::Deserializer<'de> for Text<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_str(self.0)
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.0 {
            "true" | "1" => visitor.visit_bool(true),
            "false" | "0" => visitor.visit_bool(false),
            text => Err(Error(format!("expected boolean, got {text:?}"))),
        }
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_f64(self.finite()?)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_f32(self.finite()?)
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_i8(self.number()?)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_i16(self.number()?)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_i32(self.number()?)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_i64(self.number()?)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_u8(self.number()?)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_u16(self.number()?)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_u32(self.number()?)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_u64(self.number()?)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let items = self
            .0
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|item| !item.is_empty())
            .map(Text);
        let mut items = SeqDeserializer::new(items);
        let value = visitor.visit_seq(&mut items)?;
        items.end()?;
        Ok(value)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let text: StrDeserializer<'_, Error> = self.0.into_deserializer();
        visitor.visit_enum(text)
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct
        tuple_struct map struct identifier ignored_any
    }
}
pub(crate) fn number(params: &Params, key: &str, default: f64) -> Result<f64> {
    let value = match params.get(key) {
        Some(value) => value
            .parse()
            .with_context(|| format!("{key}: expected number, got {value:?}"))?,
        None => default,
    };
    ensure!(value.is_finite(), "{key}: expected finite number");
    Ok(value)
}
pub(crate) fn integer(params: &Params, key: &str, default: usize) -> Result<usize> {
    params.get(key).map_or(Ok(default), |value| {
        value
            .parse()
            .with_context(|| format!("{key}: expected nonnegative integer, got {value:?}"))
    })
}
pub(crate) fn boolean(params: &Params, key: &str, default: bool) -> Result<bool> {
    match params.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        Some(value) => bail!("{key}: expected boolean, got {value:?}"),
    }
}
pub(crate) fn vector<const N: usize>(
    params: &Params,
    key: &str,
    default: [f64; N],
) -> Result<[f64; N]> {
    match params.get(key) {
        None => Ok(default),
        Some(value) => {
            let values: Vec<f64> = value
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()
                .with_context(|| format!("invalid vector {key}"))?;
            ensure!(
                values.len() == N && values.iter().all(|x| x.is_finite()),
                "{key}: expected {N} finite values"
            );
            Ok(std::array::from_fn(|i| values[i]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Params, deserialize};
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(default, deny_unknown_fields)]
    struct Example {
        #[serde(rename = "speed")]
        speed_mps: f64,
        enabled: bool,
        gains: [f64; 4],
        speeds: Vec<f64>,
        count: u32,
        small: u8,
        offset: i16,
        scale: f32,
        label: String,
        team: Option<i32>,
    }

    impl Default for Example {
        fn default() -> Self {
            Self {
                speed_mps: 21.0,
                enabled: false,
                gains: [0.0; 4],
                speeds: Vec::new(),
                count: 1,
                small: 0,
                offset: 0,
                scale: 1.0,
                label: "none".into(),
                team: None,
            }
        }
    }

    fn params(entries: &[(&str, &str)]) -> Params {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn text_values_fill_typed_fields_and_missing_keys_use_defaults() -> anyhow::Result<()> {
        let example: Example = deserialize(&params(&[
            ("speed", "30"),
            ("enabled", "1"),
            ("gains", "1, 0.01, 2, 9"),
            ("speeds", "400 400 400"),
            ("team", "2"),
            ("small", "42"),
            ("offset", "-7"),
            ("scale", "0.5"),
        ]))?;
        assert_eq!(
            example,
            Example {
                speed_mps: 30.0,
                enabled: true,
                gains: [1.0, 0.01, 2.0, 9.0],
                speeds: vec![400.0; 3],
                team: Some(2),
                small: 42,
                offset: -7,
                scale: 0.5,
                ..Example::default()
            }
        );
        Ok(())
    }

    #[test]
    fn invalid_values_and_unknown_keys_name_the_key() {
        for (key, value, message) in [
            ("speed", "fast", "speed: expected number"),
            ("speed", "NaN", "speed: expected finite number"),
            ("scale", "1e100", "scale: expected finite number"),
            ("small", "300", "small: expected number"),
            ("enabled", "yes", "enabled: expected boolean"),
            ("gains", "1 2 3", "gains: invalid length 3"),
            ("gains", "1 2 3 4 5", "gains: invalid length 5"),
            ("sped", "30", "unknown field `sped`"),
        ] {
            let error = deserialize::<Example>(&params(&[(key, value)])).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
        }
    }

    #[test]
    fn a_plugin_without_parameters_rejects_every_key() -> anyhow::Result<()> {
        deserialize::<()>(&params(&[]))?;
        let error = deserialize::<()>(&params(&[("speed", "30")])).unwrap_err();
        assert!(error.to_string().contains("unknown parameter `speed`"));
        Ok(())
    }
}
