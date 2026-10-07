//! In-memory dashboard and scoped terminal ownership.
pub mod render;
pub mod state;
pub use render::draw;
pub use state::{Action, Focus, Overlay, Sort, State};

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste},
    execute,
    style::ResetColor,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use signal_hook::{
    SigId,
    consts::{SIGHUP, SIGINT, SIGTERM},
};
use std::{
    io::{self, Stdout},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use std::{
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::Duration,
};

use crate::{
    app,
    cli::Options,
    domain::Report,
    helius::{Helius, RequestContext},
    storage::{Store, default_path},
};

/// Created before setup so partial setup failures also restore terminal state.
struct Restore {
    signals: Vec<SigId>,
}
impl Drop for Restore {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            ResetColor,
            Show,
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        for signal in self.signals.drain(..) {
            signal_hook::low_level::unregister(signal);
        }
    }
}

pub struct Session {
    pub terminal: Terminal<CrosstermBackend<Stdout>>,
    signal: Arc<AtomicUsize>,
    _restore: Restore,
}
impl Session {
    pub fn enter() -> io::Result<Self> {
        let mut restore = Restore { signals: vec![] };
        let signal = Arc::new(AtomicUsize::new(0));
        for code in [SIGINT, SIGTERM, SIGHUP] {
            restore.signals.push(signal_hook::flag::register_usize(
                code,
                Arc::clone(&signal),
                code as usize,
            )?);
        }
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste
        )?;
        let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        Ok(Self {
            terminal,
            signal,
            _restore: restore,
        })
    }
    pub fn interrupted(&self) -> Option<u8> {
        match self.signal.load(Ordering::Relaxed) {
            0 => None,
            code => Some((128 + code) as u8),
        }
    }
}

enum WorkerEvent {
    Report(u64, Box<Report>),
    Done(u64),
}

struct Worker {
    generation: u64,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    events: Receiver<WorkerEvent>,
}

fn send_report(send: &SyncSender<WorkerEvent>, generation: u64, report: Report) {
    let _ = send.try_send(WorkerEvent::Report(generation, Box::new(report)));
}

fn start_worker(options: Options, generation: u64, key: Option<String>) -> Worker {
    let (send, events) = mpsc::sync_channel(8);
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    std::thread::spawn(move || {
        let report = match default_path().and_then(Store::open) {
            Ok(store) => {
                let client = if options.offline {
                    None
                } else {
                    key.as_deref().and_then(|value| Helius::new(value).ok())
                };
                if !options.offline && client.is_none() {
                    app::failure(
                        &options,
                        "INTERNAL",
                        "Unable to initialize the provider transport.",
                    )
                } else {
                    app::inspect(
                        &options,
                        &store,
                        client.as_ref(),
                        &RequestContext::new(
                            if options.epochs == 1 {
                                Duration::from_secs(30)
                            } else {
                                Duration::from_secs(120)
                            },
                            Arc::clone(&worker_cancel),
                        ),
                        options.refresh,
                        |report| send_report(&send, generation, report),
                    )
                }
            }
            Err(error) => app::failure(
                &options,
                error.code(),
                match error.code() {
                    "UNSUPPORTED_SCHEMA" => {
                        "This database needs a newer ssteak version; it was not modified."
                    }
                    _ => {
                        "Local storage failed. Check the database location, free space and permissions; preserve the file before recovery."
                    }
                },
            ),
        };
        let _ = send.send(WorkerEvent::Report(generation, Box::new(report)));
        let _ = send.send(WorkerEvent::Done(generation));
    });
    Worker {
        generation,
        cancel,
        events,
    }
}

/// Runs the full-screen UI. The event loop holds no database connection and
/// performs no RPC work; all blocking work stays in one cancellable worker.
pub fn run(options: Options, key: Option<String>) -> u8 {
    let path = match default_path() {
        Ok(path) => path,
        Err(_) => return 5,
    };
    let mut session = match Session::enter() {
        Ok(session) => session,
        Err(_) => return 5,
    };
    let mut state = State::new(&options, path);
    let mut worker = Some(start_worker(options.clone(), state.generation, key.clone()));
    let mut pending: Option<Options> = None;
    let mut dirty = true;
    loop {
        if dirty {
            state.now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |time| time.as_secs());
            if session
                .terminal
                .draw(|frame| draw(frame, &mut state))
                .is_err()
            {
                return 5;
            }
            dirty = false;
        }
        if let Some(code) = session.interrupted() {
            if let Some(worker) = &worker {
                worker.cancel.store(true, Ordering::Relaxed);
            }
            return code;
        }
        let mut done = None;
        if let Some(active) = &worker {
            loop {
                match active.events.try_recv() {
                    Ok(WorkerEvent::Report(generation, report)) => {
                        if state.apply_generation(generation, *report) {
                            state.loading = true;
                            dirty = true;
                        }
                    }
                    Ok(WorkerEvent::Done(generation)) => done = Some(generation),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
        }
        if done.is_some_and(|generation| {
            worker
                .as_ref()
                .is_some_and(|active| active.generation == generation)
        }) {
            worker = None;
            state.loading = false;
            dirty = true;
            if let Some(next) = pending.take() {
                worker = Some(start_worker(next, state.generation, key.clone()));
            }
        }
        if crossterm::event::poll(Duration::from_millis(50)).unwrap_or(false)
            && let Ok(event) = crossterm::event::read()
        {
            match state.handle_event(event) {
                Action::Quit(code) => {
                    if let Some(worker) = &worker {
                        worker.cancel.store(true, Ordering::Relaxed);
                    }
                    return code;
                }
                Action::Refresh => {
                    let mut next = options.clone();
                    next.address = state.address.clone();
                    next.refresh = true;
                    if worker.is_none() {
                        worker = Some(start_worker(next, state.generation, key.clone()));
                    } else {
                        pending = Some(next);
                    }
                }
                Action::Address(address) => {
                    let mut next = options.clone();
                    next.address = address;
                    next.refresh = false;
                    if let Some(active) = &worker {
                        active.cancel.store(true, Ordering::Relaxed);
                        pending = Some(next);
                    } else {
                        worker = Some(start_worker(next, state.generation, key.clone()));
                    }
                }
                Action::None => {}
            }
            dirty = true;
        }
    }
}
