//! WIP: interactive terminal UI over the ptj core (`ptj tui`).
//!
//! A local, no-backend frontend surface — the terminal sibling of the webgui.
//! It will drive the SAME core operations the CLI exposes
//! (create/join/sort/sync/pay/confirm/payments), never a parallel
//! implementation.
//!
//! Current state: a placeholder screen only. It draws a static "work in
//! progress" notice and exits on any key press. Nothing is implemented beyond
//! terminal setup/teardown; do not read this module as a working TUI.
//!
//! TODO checklist (roughly in order):
//! - [ ] App state model: which PSBT document is open (state file + sources,
//!       mirroring `sync`'s file/dir transport), dirty/converged status.
//! - [ ] Screen: inspect view (decoded inputs/outputs, ordering status) —
//!       read-only port of `inspect`.
//! - [ ] Screen: payments/negotiation view (`payments` report, incl.
//!       encrypted entries given a secret).
//! - [ ] Actions: pay / confirm forms writing through the same code paths as
//!       `commands::negotiation` (validation errors surfaced inline).
//! - [ ] Actions: create / join / sort / make-unordered over file pickers.
//! - [ ] Ongoing sync: reuse the notify-based watcher from `run_ongoing_sync`
//!       to live-refresh the view when sources/state change on disk.
//! - [ ] Event loop hygiene: tick/redraw cadence, terminal resize, panic hook
//!       that restores the terminal (ratatui::restore on unwind).
//! - [ ] Keybindings + help footer; quit confirmation when unsaved.
//! - [ ] Tests: state-model unit tests; rendering snapshot tests via
//!       ratatui's TestBackend.

use crate::cli::TuiConfig;
use crate::{Error, Result};

/// Open the placeholder screen. WIP: no real functionality yet — draws a
/// static notice and returns once the user presses any key.
pub fn run(config: TuiConfig) -> Result<()> {
    // No options exist yet; consume the config so the signature is stable.
    let TuiConfig {} = config;
    let mut terminal = ratatui::init();
    let result = placeholder_screen(&mut terminal);
    ratatui::restore();
    result
}

/// Draw the static WIP notice until a key is pressed.
fn placeholder_screen(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    loop {
        terminal
            .draw(draw_placeholder)
            .map_err(|error| Error::new(format!("tui: drawing placeholder screen: {error}")))?;
        match crossterm::event::read()
            .map_err(|error| Error::new(format!("tui: reading terminal event: {error}")))?
        {
            crossterm::event::Event::Key(key)
                if key.kind == crossterm::event::KeyEventKind::Press =>
            {
                return Ok(());
            }
            _ => {}
        }
    }
}

fn draw_placeholder(frame: &mut ratatui::Frame) {
    let notice = ratatui::widgets::Paragraph::new(
        "ptj tui: not yet implemented\n\n\
         This is a WIP placeholder screen. The interactive terminal UI over\n\
         the ptj core (create/join/sort/sync/pay/confirm/payments) is not\n\
         built yet — see the TODO checklist in crates/ptj/src/tui.rs.\n\n\
         Press any key to exit.",
    )
    .block(
        ratatui::widgets::Block::bordered().title("ptj tui — work in progress"),
    );
    frame.render_widget(notice, frame.area());
}
