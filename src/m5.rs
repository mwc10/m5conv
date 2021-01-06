use std::str::FromStr;

use anyhow::{Result, Error, Context, anyhow};

#[derive(Debug)]
pub(crate) struct M5DataBlocks(u16);

impl FromStr for M5DataBlocks {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut it = s.split_whitespace().take(2);

        match (it.next().as_deref(), it.next()) {
            (Some("##BLOCKS="), Some(b)) => Ok(b),
            _ => Err(anyhow!("Missing BLOCKS magic string")),
        }.and_then(|b| b.parse().map_err(Into::into))
        .map(Self)
    }
}
