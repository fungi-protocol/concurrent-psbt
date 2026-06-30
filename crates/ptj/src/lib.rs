use cli::OutputFileFormat;

pub mod cli;
pub mod webgui;

mod commands;
mod error;
mod io;

pub use error::{Error, Result};

pub fn run(cli: cli::Cli) -> Result<String> {
    commands::run(cli.command)
}

pub fn run_with_stdin(cli: cli::Cli, stdin: &[u8]) -> Result<String> {
    commands::run_with_stdin(cli.command, Some(stdin))
}

pub fn run_or_write(cli: cli::Cli) -> Result<Option<String>> {
    run_or_write_with_stdin(cli, None)
}

pub fn run_or_write_with_stdin(cli: cli::Cli, stdin: Option<&[u8]>) -> Result<Option<String>> {
    let output_path = output_path(&cli)?;
    let output_file_format = output_file_format(&cli, output_path.as_deref())?;
    if let Some(path) = output_path.as_ref() {
        reject_destructive_output_alias(path, &cli.command)?;
    }
    if let cli::Command::Sync(config) = cli.command.clone()
        && config.ongoing
    {
        let path = output_path.ok_or_else(|| {
            Error::new("ongoing sync requires --state or --output-file to update")
        })?;
        return run_ongoing_sync(config, stdin, &path, output_file_format);
    }
    if let Some(path) = output_path {
        if matches!(&cli.command, cli::Command::Sync(_)) {
            io::with_file_lock(&path, || {
                let output = commands::run_with_stdin(cli.command, stdin)?;
                write_output_file(&path, &output, output_file_format)?;
                Ok(())
            })?;
            return Ok(None);
        }
        let output = commands::run_with_stdin(cli.command, stdin)?;
        write_output_file(&path, &output, output_file_format)?;
        Ok(None)
    } else {
        let output = commands::run_with_stdin(cli.command, stdin)?;
        Ok(Some(output))
    }
}

fn run_ongoing_sync(
    mut config: cli::SyncConfig,
    stdin: Option<&[u8]>,
    path: &std::path::Path,
    output_file_format: OutputFileFormat,
) -> Result<Option<String>> {
    commands::validate_ongoing_sync(&config, stdin)?;
    config.state = Some(path.to_path_buf());
    let mut iterations = 0usize;
    loop {
        io::with_file_lock(path, || {
            let output = commands::run_sync_once(&config, None)?;
            write_output_file(path, &output, output_file_format)?;
            Ok(())
        })?;
        iterations += 1;
        if config.max_iterations.is_some_and(|max| iterations >= max) {
            return Ok(None);
        }
        std::thread::sleep(commands::sync_poll_interval(&config));
    }
}

fn output_path(cli: &cli::Cli) -> Result<Option<std::path::PathBuf>> {
    let command_state = match &cli.command {
        cli::Command::Sync(config) => config.state.clone(),
        _ => None,
    };
    match (cli.output.clone(), command_state) {
        (Some(_), Some(_)) => Err(Error::new(
            "use either --output-file or sync --state, not both",
        )),
        (Some(path), None) | (None, Some(path)) => Ok(Some(path)),
        (None, None) => Ok(None),
    }
}

fn output_file_format(
    cli: &cli::Cli,
    output_path: Option<&std::path::Path>,
) -> Result<OutputFileFormat> {
    if cli.binary {
        if output_path.is_none() {
            return Err(Error::new(
                "--binary requires --output-file or sync --state; stdout is always base64 text",
            ));
        }
        return Ok(OutputFileFormat::Binary);
    }
    Ok(cli.output_file_format)
}

fn write_output_file(path: &std::path::Path, output: &str, format: OutputFileFormat) -> Result<()> {
    match format {
        OutputFileFormat::Base64 => io::write_text_atomic(path, output),
        OutputFileFormat::Binary => {
            io::write_binary_atomic(path, &single_psbt_output_bytes(output)?)
        }
    }
}

fn single_psbt_output_bytes(output: &str) -> Result<Vec<u8>> {
    let trimmed = output.trim();
    if trimmed.lines().count() != 1 {
        return Err(Error::new(
            "binary --output-file-format requires a command that emits exactly one PSBT",
        ));
    }
    use psbt_v2::bitcoin::base64::prelude::{BASE64_STANDARD, Engine as _};
    BASE64_STANDARD
        .decode(trimmed)
        .map_err(|error| Error::new(format!("decoding command output as PSBT base64: {error}")))
}

fn reject_destructive_output_alias(output: &std::path::Path, command: &cli::Command) -> Result<()> {
    match command {
        cli::Command::Atomize(config) if same_existing_path(output, &config.file) => {
            Err(Error::new(
                "refusing to overwrite atomize input: atomize writes multiple PSBTs, so choose a distinct -o/--output-file",
            ))
        }
        cli::Command::ExportBip174(config) if same_existing_path(output, &config.file) => {
            Err(Error::new(
                "refusing to overwrite export-bip174 input: export changes the PSBT file format, so choose a distinct -o/--output-file",
            ))
        }
        cli::Command::ImportBip174(config) if same_existing_path(output, &config.file) => {
            Err(Error::new(
                "refusing to overwrite import-bip174 input: import changes the PSBT file format, so choose a distinct -o/--output-file",
            ))
        }
        cli::Command::MakeUnordered(config) if same_existing_path(output, &config.file) => {
            Err(Error::new(
                "refusing to overwrite make-unordered input: make-unordered changes ordering semantics, so choose a distinct -o/--output-file",
            ))
        }
        cli::Command::Sort(config) if same_existing_path(output, &config.file) => Err(Error::new(
            "refusing to overwrite sort input: sort fixes PSBT ordering, so choose a distinct -o/--output-file",
        )),
        _ => Ok(()),
    }
}

fn same_existing_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
