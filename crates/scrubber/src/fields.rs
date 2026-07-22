/// PSBT_GLOBAL_VERSION value identifying a v2 PSBT.
pub(crate) const PSBT_V2: u8 = 2;

/// Global map key types retained when scrubbing (non-sensitive).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GlobalInsensitive {
    UnsignedTx = 0x00,
    TxVersion = 0x02,
    FallbackLocktime = 0x03,
    InputCount = 0x04,
    OutputCount = 0x05,
    TxModifiable = 0x06,
    Version = 0xFB,
}

impl TryFrom<u8> for GlobalInsensitive {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == Self::UnsignedTx as u8 => Ok(Self::UnsignedTx),
            x if x == Self::TxVersion as u8 => Ok(Self::TxVersion),
            x if x == Self::FallbackLocktime as u8 => Ok(Self::FallbackLocktime),
            x if x == Self::InputCount as u8 => Ok(Self::InputCount),
            x if x == Self::OutputCount as u8 => Ok(Self::OutputCount),
            x if x == Self::TxModifiable as u8 => Ok(Self::TxModifiable),
            x if x == Self::Version as u8 => Ok(Self::Version),
            _ => Err(()),
        }
    }
}

impl GlobalInsensitive {
    pub(crate) fn contains(v: u8) -> bool {
        Self::try_from(v).is_ok()
    }
}

/// Input map key types retained when scrubbing (non-sensitive).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputInsensitive {
    NonWitnessUtxo = 0x00,
    WitnessUtxo = 0x01,
    SighashType = 0x03,
    RedeemScript = 0x04,
    WitnessScript = 0x05,
    FinalScriptsig = 0x07,
    FinalScriptwitness = 0x08,
    PreviousTxid = 0x0e,
    OutputIndex = 0x0f,
    Sequence = 0x10,
    RequiredTimeLocktime = 0x11,
    RequiredHeightLocktime = 0x12,
    TapKeySig = 0x13,
    TapScriptSig = 0x14,
    TapLeafScript = 0x15,
}

impl TryFrom<u8> for InputInsensitive {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == Self::NonWitnessUtxo as u8 => Ok(Self::NonWitnessUtxo),
            x if x == Self::WitnessUtxo as u8 => Ok(Self::WitnessUtxo),
            x if x == Self::SighashType as u8 => Ok(Self::SighashType),
            x if x == Self::RedeemScript as u8 => Ok(Self::RedeemScript),
            x if x == Self::WitnessScript as u8 => Ok(Self::WitnessScript),
            x if x == Self::FinalScriptsig as u8 => Ok(Self::FinalScriptsig),
            x if x == Self::FinalScriptwitness as u8 => Ok(Self::FinalScriptwitness),
            x if x == Self::PreviousTxid as u8 => Ok(Self::PreviousTxid),
            x if x == Self::OutputIndex as u8 => Ok(Self::OutputIndex),
            x if x == Self::Sequence as u8 => Ok(Self::Sequence),
            x if x == Self::RequiredTimeLocktime as u8 => Ok(Self::RequiredTimeLocktime),
            x if x == Self::RequiredHeightLocktime as u8 => Ok(Self::RequiredHeightLocktime),
            x if x == Self::TapKeySig as u8 => Ok(Self::TapKeySig),
            x if x == Self::TapScriptSig as u8 => Ok(Self::TapScriptSig),
            x if x == Self::TapLeafScript as u8 => Ok(Self::TapLeafScript),
            _ => Err(()),
        }
    }
}

impl InputInsensitive {
    pub(crate) fn contains(v: u8) -> bool {
        Self::try_from(v).is_ok()
    }
}

/// Output map key types retained when scrubbing (non-sensitive).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputInsensitive {
    Amount = 0x03,
    Script = 0x04,
}

impl TryFrom<u8> for OutputInsensitive {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            x if x == Self::Amount as u8 => Ok(Self::Amount),
            x if x == Self::Script as u8 => Ok(Self::Script),
            _ => Err(()),
        }
    }
}

impl OutputInsensitive {
    pub(crate) fn contains(v: u8) -> bool {
        Self::try_from(v).is_ok()
    }
}
