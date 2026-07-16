//! `WatchedDirTransport`: a shared directory as a convergence register.
//!
//! The directory IS the session state: every participant folds the `*.psbt`
//! files it finds there and publishes the join back as a NEW content-addressed
//! file (`<sha256-of-bytes>.psbt`). Nothing is ever overwritten — the only
//! mutations are new files and atomic link/unlink:
//!
//! 1. write the payload to a hidden create-new temp file, fsync it
//! 2. `link()` the temp file to its content-hash name — atomic, and it FAILS
//!    (`EEXIST`) rather than replacing; content-addressing turns that failure
//!    into a success, because equal names imply equal bytes
//! 3. unlink the temp name
//! 4. unlink each file this step folded into the published join — the LUB
//!    subsumes them, so removing them loses no information (this is the
//!    lattice's replace-by-LUB, spelled in filesystem operations)
//!
//! Concurrent writers need no lock: a name never changes content (write-once,
//! content-addressed), a fold only prunes the files it actually read, and a
//! file that vanishes between scan and read was pruned by a peer whose
//! published join already contains it. Any interleaving of collect/publish
//! steps keeps the directory's join constant or growing.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use psbt_v2::bitcoin::hashes::{Hash as _, sha256};
use transport_core::{Message, Transport};

use super::local::psbt_paths_from_sources;
use crate::{Error, Result, io};

/// Shared-directory transport: the first positional source is the register
/// directory; any further sources are seed PSBTs folded alongside it (and
/// published into it as part of the join, never pruned from where they live).
pub(crate) struct WatchedDirTransport {
    /// The register: a directory of write-once `<sha256>.psbt` files.
    dir: PathBuf,
    /// Seed PSBT files/directories outside the register, re-read each
    /// `collect` like the local transport's sources. Never pruned.
    seeds: Vec<PathBuf>,
    /// One-shot `-` stdin seed, with the same drain-once semantics as
    /// `LocalTransport` (see its field comment).
    stdin: Option<Option<Vec<u8>>>,
    /// Register files read by the latest `collect`, awaiting replacement by
    /// the LUB the driver publishes back.
    folded: Vec<PathBuf>,
}

impl WatchedDirTransport {
    /// Open (creating if absent) the register directory. A fresh register
    /// starts empty — creation is not an error, it is how a session begins.
    pub(crate) fn new(dir: PathBuf, seeds: Vec<PathBuf>, stdin: Option<&[u8]>) -> Result<Self> {
        fs::create_dir_all(&dir).map_err(|error| {
            Error::new(format!(
                "creating watched directory {}: {error}",
                dir.display()
            ))
        })?;
        let stdin = if seeds.iter().any(|seed| io::is_stdin_path(seed)) {
            Some(stdin.map(<[u8]>::to_vec))
        } else {
            None
        };
        Ok(Self {
            dir,
            seeds,
            stdin,
            folded: Vec::new(),
        })
    }

    fn collect_dir(&mut self) -> Result<Vec<Vec<u8>>> {
        let mut messages = Vec::new();

        // The register scan. Tolerant of files vanishing between the scan and
        // the read: a vanished file was pruned by a concurrent writer, and the
        // protocol guarantees its content lives on inside that writer's
        // published join (which this scan, or the next step, picks up).
        let mut folded = Vec::new();
        for path in super::local::psbt_files_in_directory(&self.dir)? {
            let raw = match fs::read(&path) {
                Ok(raw) => raw,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(Error::new(format!("reading {}: {error}", path.display())));
                }
            };
            // Quarantine by exclusion: a truncated or foreign `.psbt` (easy
            // on sneakernet media) is skipped — never folded, so never
            // pruned — instead of wedging every reader of the register.
            let Ok(psbt) = io::parse_psbt_bytes(&path.display().to_string(), &raw) else {
                continue;
            };
            messages.push(io::encode_psbt(&psbt).into_bytes());
            folded.push(path);
        }
        self.folded = folded;

        // Seed sources outside the register: strict, like the local transport
        // (the user named them on the command line, so absence is an error).
        for path in psbt_paths_from_sources(&self.seeds)? {
            let psbt = io::read_psbt(&path)?;
            messages.push(io::encode_psbt(&psbt).into_bytes());
        }
        if let Some(runner_stdin) = self.stdin.take() {
            let psbt = io::read_psbt_source(Path::new("-"), runner_stdin.as_deref())?;
            messages.push(io::encode_psbt(&psbt).into_bytes());
        }
        Ok(messages)
    }

    fn publish_dir(&mut self, message: Vec<u8>) -> Result<()> {
        // Only PSBTs persist, matching the local transport: in the sneakernet
        // setting negotiation rides inside the PSBT as proprietary fields.
        // A non-PSBT publish subsumes nothing — drop the collected frontier
        // so a later publish can never prune files its join does not contain.
        let Message::Psbt(payload) = Message::decode(&message)? else {
            self.folded.clear();
            return Ok(());
        };
        let text = String::from_utf8(payload)
            .map_err(|_| Error::new("converged PSBT payload is not valid UTF-8"))?;
        // Same on-disk form as a base64 state file (text + trailing newline);
        // the name is the sha256 of exactly the bytes written, so anyone can
        // check a register file with sha256sum.
        let mut bytes = Vec::with_capacity(text.len() + 1);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(b'\n');
        let hash = sha256::Hash::hash(&bytes);
        let target = self.dir.join(format!("{hash}.psbt"));
        // Existence equals content under content-addressed names, so a
        // present target makes the whole write a no-op (a concurrent prune
        // of that name means a peer published a superseding join that holds
        // this value). This is also what lets the ongoing watcher quiesce:
        // a step that changes nothing performs zero directory mutations, so
        // it cannot wake itself through its own temp-file churn.
        if !target.exists() {
            link_new(&self.dir, &target, &bytes)?;
            // The join must be durably in the directory BEFORE the folded
            // fragments are pruned: on a power cut the register may hold
            // both, never neither. This ordering fsync is the one that
            // guards data, so it is error-checked.
            sync_dir(&self.dir)
                .map_err(|error| Error::new(format!("syncing register directory: {error}")))?;
        }

        // Replace-by-LUB: the published join subsumes everything this step
        // folded, so those register files can go. Only files actually read
        // are pruned — anything a peer added since the scan is untouched —
        // and a NotFound just means a peer beat us to the same conclusion.
        for path in std::mem::take(&mut self.folded) {
            if path == target {
                continue;
            }
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Error::new(format!("pruning {}: {error}", path.display())));
                }
            }
        }
        // Best-effort: prunes are safe to lose (a replay just re-prunes).
        let _ = sync_dir(&self.dir);
        Ok(())
    }
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

/// The write-once half of the protocol: create-new temp file, atomic
/// `link()` to the content-hash name, unlink the temp name. `EEXIST` on the
/// link is a success — the target name is derived from the bytes, so an
/// existing target already holds exactly this content. No path ever has its
/// contents replaced.
fn link_new(dir: &Path, target: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = target
        .file_name()
        .ok_or_else(|| Error::new(format!("{} is not a file path", target.display())))?;
    let temp_path = dir.join(format!(
        ".{}.tmp-{}-{:016x}",
        file_name.to_string_lossy(),
        std::process::id(),
        rand::random::<u64>()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temp_path, target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error),
        }
    })();
    let _ = fs::remove_file(&temp_path);
    result.map_err(|error| {
        // FAT/exFAT — the default format of most USB sticks — has no hard
        // links; the OS reports that as a bare EPERM/ENOTSUP, so name the
        // actual limitation.
        let hint = match error.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported => {
                " (does this filesystem support hard links? FAT/exFAT USB sticks do not — \
                 use an ext4/APFS/NTFS location for the register)"
            }
            _ => "",
        };
        Error::new(format!("publishing {}: {error}{hint}", target.display()))
    })
}

#[async_trait]
impl Transport for WatchedDirTransport {
    // Directory I/O is synchronous and fast; the async methods just call the
    // sync gather/publish, mirroring `LocalTransport`.
    async fn collect(&mut self) -> transport_core::Result<Vec<Vec<u8>>> {
        self.collect_dir()
            .map_err(|error| transport_core::Error::new(error.to_string()))
    }

    async fn publish(&mut self, message: Vec<u8>) -> transport_core::Result<()> {
        self.publish_dir(message)
            .map_err(|error| transport_core::Error::new(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use psbt_v2::v2::Psbt;

    use super::*;
    use crate::commands::sync::{drive_async, sync_step};

    const TXID: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    fn psbt_spending(vout: u32) -> Psbt {
        crate::commands::create::create_psbt(crate::cli::CreateConfig {
            inputs: vec![format!("{TXID}:{vout}").parse().unwrap()],
            outputs: vec![],
            ordering: crate::cli::OrderingArg::Unset,
            seed: None,
            allow_short_seed: false,
            network: crate::cli::NetworkArg(bitcoin::Network::Regtest),
        })
        .unwrap()
    }

    fn envelope(psbt: &Psbt) -> Vec<u8> {
        Message::Psbt(io::encode_psbt(psbt).into_bytes()).encode()
    }

    fn register(dir: &Path) -> WatchedDirTransport {
        WatchedDirTransport::new(dir.to_path_buf(), Vec::new(), None).unwrap()
    }

    fn register_files(dir: &Path) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        files
    }

    #[test]
    fn publish_is_write_once_under_the_content_hash_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut transport = register(dir.path());
        let psbt = psbt_spending(0);
        drive_async(async { Ok(transport.publish(envelope(&psbt)).await?) }).unwrap();
        drive_async(async { Ok(transport.publish(envelope(&psbt)).await?) }).unwrap();

        // Exactly one file — the second publish found its name taken and was
        // content — and no temp debris. The name IS the sha256 of the bytes.
        let files = register_files(dir.path());
        assert_eq!(files.len(), 1, "{files:?}");
        let bytes = fs::read(&files[0]).unwrap();
        let expected = format!("{}.psbt", sha256::Hash::hash(&bytes));
        assert_eq!(files[0].file_name().unwrap().to_str().unwrap(), expected);
        assert_eq!(bytes, format!("{}\n", io::encode_psbt(&psbt)).into_bytes());
    }

    #[test]
    fn a_step_replaces_the_folded_frontier_with_its_join() {
        let dir = tempfile::tempdir().unwrap();
        let mut transport = register(dir.path());
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(0))).await?) }).unwrap();
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(1))).await?) }).unwrap();
        assert_eq!(register_files(dir.path()).len(), 2);

        let mut stepper = register(dir.path());
        let (joined, _messages) = drive_async(async { sync_step(&mut stepper).await }).unwrap();

        // The two fragments were replaced by the single LUB file.
        let files = register_files(dir.path());
        assert_eq!(files.len(), 1, "{files:?}");
        assert_eq!(
            fs::read(&files[0]).unwrap(),
            format!("{}\n", io::encode_psbt(&joined)).into_bytes()
        );
        // And the join really carries both inputs.
        let text = String::from_utf8(fs::read(&files[0]).unwrap()).unwrap();
        let stored = io::parse_psbt_bytes("register", text.trim().as_bytes()).unwrap();
        assert_eq!(stored.inputs.len(), 2);
    }

    #[test]
    fn concurrent_additions_survive_the_prune() {
        let dir = tempfile::tempdir().unwrap();
        let mut transport = register(dir.path());
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(0))).await?) }).unwrap();

        let mut stepper = register(dir.path());
        let collected = drive_async(async { Ok(stepper.collect().await?) }).unwrap();
        assert_eq!(collected.len(), 1);
        // A peer publishes between our collect and our publish.
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(1))).await?) }).unwrap();
        // Our publish prunes only what we read; the peer's file stays.
        drive_async(async { Ok(stepper.publish(envelope(&psbt_spending(2))).await?) }).unwrap();

        let names: Vec<String> = register_files(dir.path())
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
        let survivors: Vec<Psbt> = register_files(dir.path())
            .iter()
            .map(|path| {
                let text = fs::read_to_string(path).unwrap();
                io::parse_psbt_bytes("register", text.trim().as_bytes()).unwrap()
            })
            .collect();
        // vout 0 was folded and pruned; 1 (concurrent) and 2 (ours) remain.
        assert!(
            survivors
                .iter()
                .all(|psbt| psbt.inputs[0].previous_txid.to_string() == TXID)
        );
        let mut vouts: Vec<u32> = survivors
            .iter()
            .map(|psbt| psbt.inputs[0].spent_output_index)
            .collect();
        vouts.sort_unstable();
        assert_eq!(vouts, vec![1, 2]);
    }

    #[test]
    fn publish_without_a_collect_prunes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut transport = register(dir.path());
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(0))).await?) }).unwrap();

        // The webgui pre-publish shape: a fresh transport publishes its local
        // fold before ever collecting. The existing register file stays.
        let mut fresh = register(dir.path());
        drive_async(async { Ok(fresh.publish(envelope(&psbt_spending(1))).await?) }).unwrap();
        assert_eq!(register_files(dir.path()).len(), 2);
    }

    #[test]
    fn a_converged_step_mutates_nothing() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let mut transport = register(dir.path());
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(0))).await?) }).unwrap();
        let before = register_files(dir.path());
        assert_eq!(before.len(), 1);

        // The ongoing watcher's steady state: the register holds exactly the
        // join, so a step collects it and publishes it right back. With the
        // directory read-only the OS vetoes ANY create, link, or unlink — the
        // step succeeds only if it truly performs zero mutations, which is
        // what lets the watch loop quiesce instead of waking itself.
        let writable = fs::metadata(dir.path()).unwrap().permissions();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let mut stepper = register(dir.path());
        let result = drive_async(async { sync_step(&mut stepper).await });
        fs::set_permissions(dir.path(), writable).unwrap();
        result.unwrap();
        assert_eq!(register_files(dir.path()), before);
    }

    #[test]
    fn unparsable_register_files_are_quarantined_not_folded() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.psbt");
        fs::write(&junk, b"not a psbt").unwrap();
        let mut transport = register(dir.path());
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(0))).await?) }).unwrap();
        drive_async(async { Ok(transport.publish(envelope(&psbt_spending(1))).await?) }).unwrap();

        let mut stepper = register(dir.path());
        let (joined, _messages) = drive_async(async { sync_step(&mut stepper).await }).unwrap();
        assert_eq!(joined.inputs.len(), 2);

        // The garbage neither wedged the fold nor entered it: the two real
        // fragments were replaced by their join while junk.psbt sits in the
        // register untouched — excluded from the fold, so excluded from the
        // prune.
        let files = register_files(dir.path());
        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files.contains(&junk));
        assert_eq!(fs::read(&junk).unwrap(), b"not a psbt");
    }

    #[test]
    fn non_psbt_messages_do_not_persist() {
        let dir = tempfile::tempdir().unwrap();
        let mut transport = register(dir.path());
        drive_async(async {
            Ok(transport
                .publish(Message::Payment(vec![1, 2, 3]).encode())
                .await?)
        })
        .unwrap();
        assert_eq!(register_files(dir.path()), Vec::<PathBuf>::new());
    }

    #[test]
    fn new_creates_the_register_directory() {
        let dir = tempfile::tempdir().unwrap();
        let register_dir = dir.path().join("session").join("register");
        let mut transport =
            WatchedDirTransport::new(register_dir.clone(), Vec::new(), None).unwrap();
        assert!(register_dir.is_dir());
        // And an empty register collects nothing rather than erroring.
        let collected = drive_async(async { Ok(transport.collect().await?) }).unwrap();
        assert_eq!(collected, Vec::<Vec<u8>>::new());
    }

    #[test]
    fn seed_sources_fold_in_but_are_never_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let register_dir = dir.path().join("register");
        let seed_path = dir.path().join("seed.psbt");
        fs::write(&seed_path, io::encode_psbt(&psbt_spending(3))).unwrap();

        let mut transport =
            WatchedDirTransport::new(register_dir.clone(), vec![seed_path.clone()], None).unwrap();
        let (joined, _messages) = drive_async(async { sync_step(&mut transport).await }).unwrap();
        assert_eq!(joined.inputs.len(), 1);

        // The seed file is untouched where it lives; its content entered the
        // register as part of the published join.
        assert!(seed_path.is_file());
        assert_eq!(register_files(&register_dir).len(), 1);
    }
}
