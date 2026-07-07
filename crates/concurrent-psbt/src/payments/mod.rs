//! The payment-negotiation stack, consolidated as one gate/extraction
//! boundary: the `PSBT_GLOBAL_PAYMENT` (0x20) / `PSBT_GLOBAL_CONFIRMATION`
//! (0x21) wire fields plus the pure-logic layer over them.

pub mod negotiation;

// Pure-logic layer over the existing PSBT_GLOBAL_PAYMENT (0x20) and
// PSBT_GLOBAL_CONFIRMATION (0x21) fields. These define NO new PSBT field; they
// read the grow-only payment/confirmation sets out of a Global and sequence
// join → confirmation → export.
pub mod graph;
pub mod readiness;
pub mod session;
