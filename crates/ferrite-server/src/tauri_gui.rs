//! Tauri desktop shell for the Windows build.
//!
//! The server runtime stays in a dedicated thread. The webview only sends start/stop/command
//! actions and receives log and lifecycle events, so the game loop never depends on the UI.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use tracing_subscriber::fmt::MakeWriter;

use super::run_server;

enum Action {
    Start,
    Stop,
    Command(String),
    Exit,
}

enum UiMessage {
    Log(String),
    ServerStarted,
    ServerStopped,
}

struct GuiState {
    actions: Mutex<Option<Sender<Action>>>,
    active: AtomicBool,
    stopping: AtomicBool,
    closing: AtomicBool,
}

impl GuiState {
    fn sender(&self) -> Result<Sender<Action>, String> {
        self.actions
            .lock()
            .map_err(|_| "서버 관리자 상태를 읽을 수 없습니다.".to_owned())?
            .clone()
            .ok_or_else(|| "서버 관리자가 아직 준비되지 않았습니다.".to_owned())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfo {
    version: &'static str,
    protocol: i32,
    address: String,
}

#[tauri::command]
fn server_info() -> ServerInfo {
    let address = super::configured_bind_address();
    ServerInfo {
        version: env!("CARGO_PKG_VERSION"),
        protocol: ferrite_protocol::PROTOCOL_VERSION,
        address,
    }
}

#[tauri::command]
fn start_server(state: State<'_, GuiState>) -> Result<(), String> {
    if state.active.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    state.stopping.store(false, Ordering::SeqCst);

    let result = state.sender().and_then(|sender| {
        sender
            .send(Action::Start)
            .map_err(|_| "서버 시작 요청을 전달하지 못했습니다.".to_owned())
    });
    if result.is_err() {
        state.active.store(false, Ordering::SeqCst);
    }
    result
}

#[tauri::command]
fn stop_server(state: State<'_, GuiState>) -> Result<(), String> {
    if !state.active.load(Ordering::SeqCst) {
        return Ok(());
    }
    state.stopping.store(true, Ordering::SeqCst);
    state
        .sender()?
        .send(Action::Stop)
        .map_err(|_| "서버 종료 요청을 전달하지 못했습니다.".to_owned())
}

#[tauri::command]
fn send_command(command: String, state: State<'_, GuiState>) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Ok(());
    }
    if !state.active.load(Ordering::SeqCst) {
        return Err("서버가 실행 중일 때만 명령을 보낼 수 있습니다.".to_owned());
    }
    state
        .sender()?
        .send(Action::Command(command.to_owned()))
        .map_err(|_| "서버에 명령을 전달하지 못했습니다.".to_owned())
}

#[tauri::command]
fn request_close(app: AppHandle) {
    begin_shutdown(&app);
}

fn begin_shutdown(app: &AppHandle) {
    let state = app.state::<GuiState>();
    if state.closing.swap(true, Ordering::SeqCst) {
        return;
    }
    if !state.active.load(Ordering::SeqCst) {
        app.exit(0);
        return;
    }
    if !state.stopping.load(Ordering::SeqCst) && !confirm_shutdown() {
        state.closing.store(false, Ordering::SeqCst);
        return;
    }

    let sent = state
        .sender()
        .ok()
        .is_some_and(|sender| sender.send(Action::Exit).is_ok());
    if !sent {
        app.exit(0);
        return;
    }

    // A wedged server must not leave an invisible window behind forever.
    let timeout_app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(30));
        timeout_app.exit(0);
    });
}

#[derive(Clone)]
struct GuiLogWriter(Sender<UiMessage>);

struct GuiLogBuffer {
    sender: Sender<UiMessage>,
    bytes: Vec<u8>,
}

impl<'a> MakeWriter<'a> for GuiLogWriter {
    type Writer = GuiLogBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        GuiLogBuffer {
            sender: self.0.clone(),
            bytes: Vec::with_capacity(256),
        }
    }
}

impl Write for GuiLogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.bytes.is_empty() {
            let line = String::from_utf8_lossy(&self.bytes).trim_end().to_owned();
            self.bytes.clear();
            if !line.is_empty() {
                let _ = self.sender.send(UiMessage::Log(line));
            }
        }
        Ok(())
    }
}

impl Drop for GuiLogBuffer {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

struct RunningServer {
    commands: tokio::sync::mpsc::UnboundedSender<String>,
    thread: thread::JoinHandle<()>,
    started: Option<oneshot::Receiver<()>>,
    cancelled: Arc<AtomicBool>,
}

fn server_manager(actions: Receiver<Action>, messages: Sender<UiMessage>) {
    let mut running: Option<RunningServer> = None;
    let mut exiting = false;

    while !exiting || running.is_some() {
        match actions.recv_timeout(Duration::from_millis(100)) {
            Ok(Action::Start) if running.is_none() => {
                let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
                let (started_tx, started_rx) = oneshot::channel();
                let cancelled = Arc::new(AtomicBool::new(false));
                let startup_cancelled = Arc::clone(&cancelled);
                let server_thread = thread::Builder::new()
                    .name("ferrite-server-runtime".into())
                    .spawn(move || {
                        let result = tokio::runtime::Builder::new_multi_thread()
                            .enable_all()
                            .build()
                            .map(|runtime| {
                                runtime.block_on(run_server(
                                    command_rx,
                                    Some(started_tx),
                                    Some(startup_cancelled),
                                ))
                            });
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::error!(
                                    error = %format!("{error:#}"),
                                    "server stopped with an error"
                                );
                            }
                            Err(error) => {
                                tracing::error!(%error, "could not create the server runtime");
                            }
                        }
                    });

                match server_thread {
                    Ok(server_thread) => {
                        running = Some(RunningServer {
                            commands: command_tx,
                            thread: server_thread,
                            started: Some(started_rx),
                            cancelled,
                        });
                    }
                    Err(error) => {
                        tracing::error!(%error, "could not start the server thread");
                        let _ = messages.send(UiMessage::ServerStopped);
                    }
                }
            }
            Ok(Action::Stop) => {
                if let Some(server) = &running {
                    server.cancelled.store(true, Ordering::SeqCst);
                    let _ = server.commands.send("stop".into());
                }
            }
            Ok(Action::Command(command)) => {
                if let Some(server) = &running {
                    let _ = server.commands.send(command);
                }
            }
            Ok(Action::Exit) => {
                exiting = true;
                if let Some(server) = &running {
                    server.cancelled.store(true, Ordering::SeqCst);
                    let _ = server.commands.send("stop".into());
                }
            }
            Ok(Action::Start) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                exiting = true;
                if let Some(server) = &running {
                    let _ = server.commands.send("stop".into());
                }
            }
        }

        if let Some(server) = &mut running {
            let started = server.started.as_mut().map(oneshot::Receiver::try_recv);
            match started {
                Some(Ok(())) => {
                    server.started = None;
                    let _ = messages.send(UiMessage::ServerStarted);
                }
                Some(Err(oneshot::error::TryRecvError::Closed)) => {
                    server.started = None;
                }
                _ => {}
            }
        }

        if running
            .as_ref()
            .is_some_and(|server| server.thread.is_finished())
            && let Some(server) = running.take()
        {
            let _ = server.thread.join();
            let _ = messages.send(UiMessage::ServerStopped);
        }
    }
}

fn forward_messages(app: AppHandle, messages: Receiver<UiMessage>) {
    while let Ok(message) = messages.recv() {
        match message {
            UiMessage::Log(line) => {
                let _ = app.emit("server-log", line);
            }
            UiMessage::ServerStarted => {
                let _ = app.emit("server-started", ());
            }
            UiMessage::ServerStopped => {
                let state = app.state::<GuiState>();
                state.active.store(false, Ordering::SeqCst);
                state.stopping.store(false, Ordering::SeqCst);
                let closing = state.closing.load(Ordering::SeqCst);
                let _ = app.emit("server-stopped", ());
                if closing {
                    app.exit(0);
                }
            }
        }
    }
}

pub(super) fn run() -> anyhow::Result<()> {
    let executable = std::env::current_exe().context("locating ferrite-server.exe")?;
    if let Some(directory) = executable.parent() {
        std::env::set_current_dir(directory)
            .with_context(|| format!("using server directory {}", directory.display()))?;
    }

    let (actions_tx, actions_rx) = mpsc::channel();
    let (messages_tx, messages_rx) = mpsc::channel();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(GuiLogWriter(messages_tx.clone()))
        .try_init()
        .map_err(|error| anyhow::anyhow!("initializing GUI log capture: {error}"))?;

    tauri::Builder::default()
        .manage(GuiState {
            actions: Mutex::new(Some(actions_tx)),
            active: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            closing: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            server_info,
            start_server,
            stop_server,
            send_command,
            request_close
        ])
        .setup(move |app| {
            let manager_handle = app.handle().clone();
            thread::Builder::new()
                .name("ferrite-server-manager".into())
                .spawn(move || server_manager(actions_rx, messages_tx))?;
            thread::Builder::new()
                .name("ferrite-ui-events".into())
                .spawn(move || forward_messages(manager_handle, messages_rx))?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                begin_shutdown(window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .map_err(|error| anyhow::anyhow!("running the Ferrite window: {error}"))
}

fn confirm_shutdown() -> bool {
    let title: Vec<u16> = "Ferrite 서버 종료\0".encode_utf16().collect();
    let body: Vec<u16> = "서버를 저장하고 종료할까요?\n\n저장이 끝날 때까지 창이 열려 있습니다.\0"
        .encode_utf16()
        .collect();
    // MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2: closing must not save by accident.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            0x0000_0004 | 0x0000_0020 | 0x0000_0100,
        ) == 6
    }
}

pub(super) fn fatal_error(message: &str) {
    let title: Vec<u16> = "Ferrite Server\0".encode_utf16().collect();
    let body: Vec<u16> = format!("{message}\0").encode_utf16().collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            0x0000_0010,
        );
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(
        hwnd: *mut std::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
}
