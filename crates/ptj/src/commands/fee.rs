//! Explicit fee contribution subcommand.
//!
//! `fee` appends one `PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION` entry (subtype
//! `0x22`, the third negotiation-band field) via the library's
//! [`GlobalFeeExt`] — a grow-only add keyed by a fresh random uuid, exactly
//! like `pay`/`confirm` grow their bands. The record is the library codec's
//! bare u64 satoshi amount ([`FeeContribution`]); with `--encrypt --secret`
//! it is sealed with the same deterministic group-key AEAD as the negotiation
//! records (`commands::negotiation`), parameterized by the fee subtype.
//!
//! This works on LOCAL PSBTs: no session or transport is required, so a
//! sneakernet coinjoin can declare its termination fee on a file that never
//! touches a sync backend.

use concurrent_psbt::fee::{
    FeeContribution, GlobalFeeExt as _, PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE,
};
use psbt_v2::v2::Psbt;

use crate::cli::FeeConfig;
use crate::{Result, io};

pub(super) fn run(config: FeeConfig, stdin: Option<&[u8]>) -> Result<Psbt> {
    let mut psbt = io::read_psbt_source(&config.file, stdin)?;
    let secret = if config.encrypt {
        Some(super::negotiation::require_secret(
            config.secret.as_ref().map(|secret| secret.as_bytes()),
        )?)
    } else {
        None
    };
    add_fee_contribution(&mut psbt, config.amount_sats, secret)?;
    Ok(psbt)
}

/// Append one explicit fee contribution of `amount_sats` under a fresh random
/// id. Shared by `ptj fee` and the webgui `/api/fee` route. When `secret` is
/// present the record is encrypted (fee-subtype AAD); otherwise it is stored
/// as the plaintext codec bytes that `fee::total_declared_fee` counts.
pub(crate) fn add_fee_contribution(
    psbt: &mut Psbt,
    amount_sats: u64,
    secret: Option<&[u8]>,
) -> Result<()> {
    let id = super::negotiation::random_id();
    let record = FeeContribution { amount_sats }.encode();
    let blob = match secret {
        Some(secret) => super::negotiation::encrypt(
            secret,
            PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE,
            &id,
            &record,
        )?,
        None => record,
    };
    psbt.global.add_fee_contribution(id, blob);
    Ok(())
}

#[cfg(test)]
mod tests {
    use concurrent_psbt::fee::total_declared_fee;
    use concurrent_psbt::payments::negotiation::FORMAT_ENCRYPTED;

    use super::*;

    fn empty_psbt() -> Psbt {
        Psbt {
            global: psbt_v2::v2::Global::default(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    }

    #[test]
    fn plaintext_contributions_grow_the_band_and_sum() {
        let mut psbt = empty_psbt();
        add_fee_contribution(&mut psbt, 700, None).unwrap();
        add_fee_contribution(&mut psbt, 42, None).unwrap();
        assert_eq!(psbt.global.fee_contributions().len(), 2);
        assert_eq!(total_declared_fee(&psbt.global), 742);
    }

    #[test]
    fn encrypted_contribution_is_opaque_until_decrypted_with_the_secret() {
        let mut psbt = empty_psbt();
        add_fee_contribution(&mut psbt, 9_999, Some(b"shared")).unwrap();

        // Opaque without the secret: stored as FORMAT_ENCRYPTED, invisible
        // to the plaintext termination sum.
        let contributions = psbt.global.fee_contributions();
        assert_eq!(contributions.len(), 1);
        let (id, blob) = &contributions[0];
        assert_eq!(blob.first(), Some(&FORMAT_ENCRYPTED));
        assert_eq!(total_declared_fee(&psbt.global), 0);

        // The group secret round-trips the record through the shared
        // negotiation AEAD under the fee subtype.
        let plaintext = crate::commands::negotiation::decrypt(
            b"shared",
            PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE,
            id,
            blob,
        )
        .unwrap()
        .expect("blob is encrypted");
        assert_eq!(
            FeeContribution::decode(&plaintext).unwrap(),
            FeeContribution { amount_sats: 9_999 }
        );

        // The wrong secret fails loudly rather than yielding garbage.
        assert!(
            crate::commands::negotiation::decrypt(
                b"wrong",
                PSBT_GLOBAL_EXPLICIT_FEE_CONTRIBUTION_SUBTYPE,
                id,
                blob,
            )
            .is_err()
        );
    }
}
