use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::output::PSBT_OUT_UNIQUE_ID_SUBTYPE;
use concurrent_psbt::roles::Creator;
use psbt_v2::v2::{Input, Output, Psbt};

use crate::cli::{CreateConfig, OrderingArg};
use crate::{Error, Result};

pub(super) fn run(config: CreateConfig) -> Result<Psbt> {
    create_psbt(config)
}

pub(crate) fn create_psbt(config: CreateConfig) -> Result<Psbt> {
    let has_items = !config.inputs.is_empty() || !config.outputs.is_empty();
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

        let mut psbt_output = Output {
            amount: output.amount,
            script_pubkey: address.script_pubkey(),
            ..Output::default()
        };
        psbt_output
            .proprietaries
            .insert(unique_id_key(), rand::random::<[u8; 16]>().to_vec());
        constructor = constructor.output(psbt_output);
    }

    let mut psbt = constructor.into_inner();
    psbt.global.set_unordered();
    match (config.ordering, config.seed) {
        (OrderingArg::Unset, Some(seed)) => {
            psbt.global.set_sort_seed(seed.into_bytes());
        }
        (OrderingArg::Unset, None) => {}
        (OrderingArg::Deterministic, Some(seed)) => {
            psbt.global.set_sort_seed(seed.into_bytes());
            psbt.global.set_sort_deterministic(0x01);
        }
        (OrderingArg::Deterministic, None) => {
            return Err(Error::new("deterministic ordering requires --seed"));
        }
        (OrderingArg::Explicit, Some(_)) => {
            return Err(Error::new("explicit ordering does not use --seed"));
        }
        (OrderingArg::Explicit, None) if has_items => {
            return Err(Error::new(
                "explicit ordering requires sort keys for every input and output; non-empty explicit create is not implemented yet",
            ));
        }
        (OrderingArg::Explicit, None) => {
            psbt.global.set_sort_deterministic(0x00);
        }
    }
    psbt.global.tx_modifiable_flags = 0x03;

    Ok(psbt.into_psbt())
}

fn unique_id_key() -> psbt_v2::raw::ProprietaryKey {
    psbt_v2::raw::ProprietaryKey {
        prefix: concurrent_psbt::PROPRIETARY_PREFIX.to_vec(),
        subtype: PSBT_OUT_UNIQUE_ID_SUBTYPE,
        key: vec![],
    }
}
