//! Borrowed conflicts in keyed sets and their paths within a PSBT.

use std::collections::HashSet;

use bitcoin::OutPoint;

use super::global::{GlobalConflict, GlobalField};
use super::input::{InputConflict, InputField};
use super::output::{OutputConflict, OutputField, UniqueId};

pub use super::keyed_set::SetConflict;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPath<'a> {
    Global(GlobalField<'a>),
    Input {
        outpoint: &'a OutPoint,
        field: InputField<'a>,
    },
    Output {
        id: &'a UniqueId,
        field: OutputField<'a>,
    },
    InputMembership,
    OutputMembership,
}

#[derive(Debug)]
pub enum PsbtConflict<'a> {
    Global(GlobalConflict<'a>),
    Input {
        outpoint: &'a OutPoint,
        conflict: InputConflict<'a>,
    },
    Output {
        id: &'a UniqueId,
        conflict: OutputConflict<'a>,
    },
    /// Distinct frozen input key sets and the joined entry keys, when they disagree.
    /// Reported once per transaction; set order is unspecified.
    InputMembership(Vec<HashSet<&'a OutPoint>>),
    /// Distinct frozen output key sets and the joined entry keys, when they disagree.
    /// Reported once per transaction; set order is unspecified.
    OutputMembership(Vec<HashSet<&'a UniqueId>>),
}

impl<'a> From<GlobalConflict<'a>> for PsbtConflict<'a> {
    fn from(conflict: GlobalConflict<'a>) -> Self {
        Self::Global(conflict)
    }
}

impl<'a> From<SetConflict<'a, OutPoint, InputConflict<'a>>> for PsbtConflict<'a> {
    fn from(conflict: SetConflict<'a, OutPoint, InputConflict<'a>>) -> Self {
        match conflict {
            SetConflict::Membership(sets) => Self::InputMembership(sets),
            SetConflict::Entry { key, conflict } => Self::Input {
                outpoint: key,
                conflict,
            },
        }
    }
}

impl<'a> From<SetConflict<'a, UniqueId, OutputConflict<'a>>> for PsbtConflict<'a> {
    fn from(conflict: SetConflict<'a, UniqueId, OutputConflict<'a>>) -> Self {
        match conflict {
            SetConflict::Membership(sets) => Self::OutputMembership(sets),
            SetConflict::Entry { key, conflict } => Self::Output { id: key, conflict },
        }
    }
}

impl<'a> PsbtConflict<'a> {
    pub fn path(&self) -> ConflictPath<'a> {
        match self {
            Self::Global(conflict) => ConflictPath::Global(conflict.path()),
            Self::Input { outpoint, conflict } => ConflictPath::Input {
                outpoint,
                field: conflict.path(),
            },
            Self::Output { id, conflict } => ConflictPath::Output {
                id,
                field: conflict.path(),
            },
            Self::InputMembership(_) => ConflictPath::InputMembership,
            Self::OutputMembership(_) => ConflictPath::OutputMembership,
        }
    }
}
