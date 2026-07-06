use concurrent_psbt::global::GlobalSortExt;
use concurrent_psbt::roles::constructor::dynamic;
use psbt_v2::v2::{Global, Psbt};

use crate::cli::AtomizeConfig;
use crate::{Error, Result, io};

pub(super) fn run(config: AtomizeConfig) -> Result<String> {
    let mut psbt = io::read_psbt(&config.file)?;
    psbt.global.set_unordered();
    let psbt = dynamic::Constructor::try_from_psbt(psbt)
        .map(dynamic::Constructor::into_psbt)
        .map_err(|error| Error::new(format!("{}: {error}", config.file.display())))?;
    let atoms = atomize(psbt)?;
    Ok(atoms
        .iter()
        .map(io::encode_psbt)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn atomize(psbt: Psbt) -> Result<Vec<Psbt>> {
    if psbt.inputs.len() + psbt.outputs.len() <= 1 {
        return Err(Error::new("PSBT is already atomic"));
    }

    let global = psbt.global;
    let mut atoms = Vec::with_capacity(psbt.inputs.len() + psbt.outputs.len());
    atoms.extend(psbt.inputs.into_iter().map(|input| Psbt {
        global: atom_global(&global, 1, 0),
        inputs: vec![input],
        outputs: vec![],
    }));
    atoms.extend(psbt.outputs.into_iter().map(|output| Psbt {
        global: atom_global(&global, 0, 1),
        inputs: vec![],
        outputs: vec![output],
    }));
    Ok(atoms)
}

fn atom_global(global: &Global, input_count: usize, output_count: usize) -> Global {
    let mut global = global.clone();
    global.input_count = input_count;
    global.output_count = output_count;
    global
}
