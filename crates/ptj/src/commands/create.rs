use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::roles::Creator;
use psbt_v2::v2::{Input, Output, Psbt};

use crate::cli::CreateConfig;
use crate::{Error, Result};

pub(super) fn run(config: CreateConfig) -> Result<Psbt> {
    let mut constructor = Creator::new().build();

    for input in config.inputs {
        constructor = constructor.input(Input::new(&input.into_outpoint()));
    }

    for output in config.outputs {
        let address_text = output.address_text;
        let address = output
            .address
            .require_network(config.network.0)
            .map_err(|error| {
                Error::new(format!(
                    "address {address_text} not valid for {}: {error}",
                    config.network
                ))
            })?;

        let psbt_output = Output {
            amount: output.amount,
            script_pubkey: address.script_pubkey(),
            ..Output::default()
        };
        constructor = constructor.output_with_new_uid(psbt_output);
    }

    let mut psbt = constructor.into_inner();
    psbt.global.set_unordered();
    if let Some(seed) = config.seed {
        psbt.global.set_sort_seed(seed.into_bytes());
        psbt.global.set_sort_deterministic(0x01);
    }
    psbt.global.tx_modifiable_flags = 0x03;

    Ok(psbt.into_psbt())
}
