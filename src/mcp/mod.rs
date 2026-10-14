pub mod bridge;
pub mod tools;
pub mod transport;

pub fn port(value: Option<&str>) -> Result<u16, crate::core::contracts::Error> {
    match value {
        None => Ok(1583),
        Some(value) => value.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| {
            crate::core::contracts::Error::new(
                "invalid_config",
                "MCP port must be an integer from1to65535.",
            )
        }),
    }
}
