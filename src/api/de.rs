//! Deserialization helpers for fields the server sends inconsistently.

use serde::{Deserialize, Deserializer};

/// Treats `null` as the type's default. The server leaves many text columns NULL
/// (artwork, author, description), and one such row shouldn't fail a whole list.
pub fn null_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

/// Accepts `true`/`false`, `0`/`1` or `null`. Postgres and MySQL backends disagree on
/// how booleans come back from some queries.
pub fn flexible_bool<'de, D>(de: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bool(bool),
        Int(i64),
    }
    Ok(match Option::<Raw>::deserialize(de)? {
        Some(Raw::Bool(b)) => b,
        Some(Raw::Int(i)) => i != 0,
        None => false,
    })
}

/// Like [`flexible_bool`], but `null` means true: for flags that default on.
pub fn flexible_bool_or_yes<'de, D>(de: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bool(bool),
        Int(i64),
    }
    Ok(match Option::<Raw>::deserialize(de)? {
        Some(Raw::Bool(b)) => b,
        Some(Raw::Int(i)) => i != 0,
        None => true,
    })
}

/// Seconds as a number or a numeric string; chapter files written by hand use both.
pub fn flexible_seconds<'de, D>(de: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(f64),
        Text(String),
    }
    match Raw::deserialize(de)? {
        Raw::Number(n) => Ok(n),
        Raw::Text(s) => s.trim().parse().map_err(serde::de::Error::custom),
    }
}
