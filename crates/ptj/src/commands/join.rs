use concurrent_psbt::Join;
use psbt_v2::v2::Psbt;

use crate::cli::JoinConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: JoinConfig) -> Result<Psbt> {
    let result = config
        .files
        .iter()
        .map(|path| io::read_modifiable(path).map(io::wrap_constructor))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .reduce(|left, right| left.join(right))
        .ok_or_else(|| Error::new("join expects at least two PSBT files"))?;

    if !result.is_ok() {
        let mut details = vec![
            "join produced conflicting fields".to_string(),
            String::new(),
        ];
        result.for_each_conflict(|section, field, conflict| {
            details.push(format!("  {section}.{field}: {conflict:?}"));
        });
        return Err(Error::new(details.join("\n")));
    }

    let constructor = match result.try_unwrap() {
        Ok(constructor) => constructor,
        Err(_) => unreachable!("is_ok() guard verified all entries"),
    };
    Ok(constructor.into_psbt())
}
