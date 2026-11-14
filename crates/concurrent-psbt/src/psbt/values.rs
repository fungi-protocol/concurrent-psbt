//! [`IdempotentValue`](crate::values::IdempotentValue) implementations for
//! bitcoin and psbt-v2 types used as PSBT fields.

use crate::values::IdempotentValue;

impl IdempotentValue for bitcoin::absolute::Height {}
impl IdempotentValue for bitcoin::absolute::Time {}
impl IdempotentValue for bitcoin::Amount {}
impl IdempotentValue for bitcoin::bip32::KeySource {}
impl IdempotentValue for bitcoin::ecdsa::Signature {}
impl IdempotentValue for bitcoin::locktime::absolute::LockTime {}
impl IdempotentValue for bitcoin::OutPoint {}
impl IdempotentValue for bitcoin::ScriptBuf {}
impl IdempotentValue for bitcoin::secp256k1::XOnlyPublicKey {}
impl IdempotentValue for bitcoin::Sequence {}
impl IdempotentValue for bitcoin::TapLeafHash {}
impl IdempotentValue for bitcoin::TapNodeHash {}
impl IdempotentValue for bitcoin::taproot::LeafVersion {}
impl IdempotentValue for bitcoin::taproot::Signature {}
impl IdempotentValue for bitcoin::taproot::TapTree {}
impl IdempotentValue for bitcoin::Transaction {}
impl IdempotentValue for bitcoin::transaction::Version {}
impl IdempotentValue for bitcoin::Txid {}
impl IdempotentValue for bitcoin::TxOut {}
impl IdempotentValue for bitcoin::Witness {}

impl IdempotentValue for psbt_v2::PsbtSighashType {}
impl IdempotentValue for psbt_v2::Version {}

impl IdempotentValue for (bitcoin::ScriptBuf, bitcoin::taproot::LeafVersion) {}
