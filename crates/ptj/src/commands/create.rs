use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::roles::Creator;
use psbt_v2::v2::{Input, Output, Psbt};

use crate::cli::{CreateConfig, OrderingArg};
use crate::{Error, Result};

pub(super) fn run(config: CreateConfig) -> Result<Psbt> {
    create_psbt(config)
}

pub(crate) fn create_psbt(config: CreateConfig) -> Result<Psbt> {
    create_psbt_with_prevouts(config, &[])
}

/// `prevouts[index]`, when present, is `inputs[index]`'s creating
/// transaction: it becomes the input's PSBT_IN_NON_WITNESS_UTXO, so a coin
/// injected into PSBT space carries its utxo data, not just the outpoint.
/// Positional; a missing or `None` entry leaves a bare outpoint input.
pub(crate) fn create_psbt_with_prevouts(
    config: CreateConfig,
    prevouts: &[Option<bitcoin::Transaction>],
) -> Result<Psbt> {
    let has_items = !config.inputs.is_empty() || !config.outputs.is_empty();
    let mut constructor = Creator::new().build();

    for (index, input) in config.inputs.into_iter().enumerate() {
        let outpoint = input.into_outpoint();
        let mut psbt_input = Input::new(&outpoint);
        if let Some(Some(prevout)) = prevouts.get(index) {
            let txid = prevout.compute_txid();
            if txid != outpoint.txid {
                return Err(Error::new(format!(
                    "inputs[{index}] raw_tx has txid {txid}, not the outpoint's {}",
                    outpoint.txid
                )));
            }
            if prevout.output.len() <= outpoint.vout as usize {
                return Err(Error::new(format!(
                    "inputs[{index}] raw_tx has {} output(s); vout {} does not exist",
                    prevout.output.len(),
                    outpoint.vout
                )));
            }
            psbt_input.non_witness_utxo = Some(prevout.clone());
        }
        constructor = constructor.input(psbt_input);
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
    match (config.ordering, config.seed) {
        (OrderingArg::Unset, Some(seed)) => {
            super::require_spec_minimum_seed(seed.as_bytes(), config.allow_short_seed)?;
            psbt.global.set_sort_seed(seed.into_bytes());
        }
        (OrderingArg::Unset, None) => {}
        (OrderingArg::Deterministic, Some(seed)) => {
            super::require_spec_minimum_seed(seed.as_bytes(), config.allow_short_seed)?;
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
