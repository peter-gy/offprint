use std::time::Duration;

use pageknot::{Result, Viewport};
use url::Url;

use super::config_value_error;
use super::document::ConfigScalar;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CdpEndpointError {
    Parse(url::ParseError),
    Scheme,
}

pub(crate) fn parse_duration_text(value: &str) -> Option<Duration> {
    let split = value
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number = number.parse::<f64>().ok()?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    let seconds = match unit {
        "ms" => number / 1000.0,
        "" | "s" => number,
        "m" => number * 60.0,
        "h" => number * 3600.0,
        _ => return None,
    };
    Duration::try_from_secs_f64(seconds).ok()
}

pub(super) fn parse_config_duration(value: &ConfigScalar) -> Result<Duration> {
    match value {
        ConfigScalar::Integer(milliseconds) => Ok(Duration::from_millis(*milliseconds)),
        ConfigScalar::String(value) => {
            parse_duration_text(value).ok_or_else(|| config_value_error("duration", value))
        }
    }
}

pub(super) fn parse_bytes(value: &ConfigScalar) -> Result<u64> {
    let value = match value {
        ConfigScalar::Integer(value) => return Ok(*value),
        ConfigScalar::String(value) => value,
    };
    let split = value
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number = number
        .parse::<u64>()
        .map_err(|_| config_value_error("byte size", value))?;
    let multiplier = match unit.to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kib" => 1024,
        "mib" => 1024 * 1024,
        "gib" => 1024 * 1024 * 1024,
        _ => return Err(config_value_error("byte size", value)),
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| config_value_error("byte size", value))
}

pub(crate) fn parse_cdp_endpoint(value: &str) -> std::result::Result<Url, CdpEndpointError> {
    let endpoint = Url::parse(value).map_err(CdpEndpointError::Parse)?;
    if matches!(endpoint.scheme(), "http" | "https" | "ws" | "wss") {
        Ok(endpoint)
    } else {
        Err(CdpEndpointError::Scheme)
    }
}

pub(crate) fn parse_viewport_text(value: &str) -> Option<Viewport> {
    let (width, height) = value.split_once(['x', 'X'])?;
    let width = width.parse::<u32>().ok()?;
    let height = height.parse::<u32>().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(Viewport {
        width,
        height,
        scale: 1,
    })
}
