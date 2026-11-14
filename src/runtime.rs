//! What the GPUI window and the headless mode both sit on: the background
//! server (the service, its HTTP API and HID, on their own thread), the commands
//! and update events that cross to the window, the update flow, and the helpers
//! for restarting, opening logs and the control panel, and relaunching after a
//! crash.

use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Result, anyhow};
use openmouse_bridge::{
    BRIDGE_PORT, api, config, platform,
    service::{BridgeService, BridgeSnapshot, Notice},
    updater::{self, UpdateInfo},
};
use std::io::Cursor;
use tokio::{
    net::TcpListener,
    sync::{mpsc as tokio_mpsc, oneshot},
    task::JoinHandle,
};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tray_icon::Icon;

/// Present when the user explicitly wants no tray panel (or when no display
/// exists): Bridge serves its HTTP API, HID, and game profiles without a
/// window. Checked first so servers and SSH sessions never touch winit.
fn headless_requested() -> bool {
    env::var_os("OPENMOUSE_BRIDGE_HEADLESS").is_some() || env::args().any(|arg| arg == "--headless")
}

/// Whether a display server is reachable for the window.
fn gui_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        env::var_os("WAYLAND_DISPLAY").is_some() || env::var_os("DISPLAY").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

/// Serves the Bridge runtime without a window: drains notices as native
/// notifications, applies update restarts, and blocks on Ctrl-C.
fn run_headless(
    server: BackgroundServer,
    snapshots: Receiver<BridgeSnapshot>,
    commands: tokio_mpsc::UnboundedSender<BridgeCommand>,
    updates: Receiver<UpdateEvent>,
    notices: Receiver<Notice>,
) -> Result<()> {
    // No panel here: dropping the snapshot receiver stops the publisher, and
    // dropping the command sender leaves the tray command loop with nothing
    // to wait for (update checks still run on their own). The update channel
    // is drained below so the updater's try_send calls keep working.
    drop((snapshots, commands));
    // Ctrl-C watcher on its own thread: the multi-thread Bridge runtime
    // already lives on the server thread, so this gets a current-thread one.
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("openmouse-bridge-signal".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(runtime) = runtime {
                let _ = runtime.block_on(tokio::signal::ctrl_c());
                let _ = shutdown_tx.send(());
            }
        })
        .ok();
    tracing::info!("OpenMouse Bridge running headless (no tray panel)");
    loop {
        while let Ok(notice) = notices.try_recv() {
            notify_natively(&notice);
        }
        while let Ok(event) = updates.try_recv() {
            // Match the GUI path: the updater stages the new binary and
            // spawns its own post-exit relaunch helper, so exiting here
            // hands off to the updated Bridge.
            if matches!(event, UpdateEvent::Restarting(_)) {
                server.stop()?;
                return Ok(());
            }
        }
        if shutdown_rx.try_recv().is_ok() {
            server.stop()?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Runs Bridge: the GPUI window, or, when asked to (or when no display exists),
/// the same runtime with no window at all.
pub fn run() -> Result<()> {
    if headless_requested() || !gui_available() {
        if !headless_requested() {
            tracing::warn!(
                "no usable display for the tray panel (set WAYLAND_DISPLAY or DISPLAY); running headless: HTTP API, HID, and game profiles still work"
            );
        }
        let (notice_tx, notices) = mpsc::channel();
        // No panel to open: dropping the panel-request receiver at once means
        // a `PUT /v1/panel` has nowhere to queue up.
        let Some((server, snapshots, commands, updates, _, _)) =
            BackgroundServer::start(notice_tx)?
        else {
            return Ok(());
        };
        return run_headless(server, snapshots, commands, updates, notices);
    }
    crate::gpui_shell::run()
}

pub(crate) const WINDOW_WIDTH: f32 = 360.0;

pub(crate) const WINDOW_HEIGHT: f32 = 280.0;

/// Glow/winit often tear down after Windows hibernate. If the tray loop ends
/// without an Exit or in-app update, spawn a replacement (issue #16).
const RELAUNCH_ENV: &str = "OPENMOUSE_BRIDGE_RELAUNCH";

const RESUME_GAP: Duration = Duration::from_secs(10);

const GUI_STABLE_AFTER: Duration = Duration::from_secs(30);

const MAX_QUICK_RELAUNCHES: u32 = 5;

pub(crate) const RELAUNCH_DELAY: Duration = Duration::from_secs(2);

pub(crate) fn service_label(snapshot: Option<&BridgeSnapshot>) -> &'static str {
    if snapshot.is_some() {
        "Bridge service ready"
    } else {
        "Starting Bridge…"
    }
}

pub(crate) enum BridgeCommand {
    SetBatteryThreshold(u8),
    SetNotificationSound(bool),
    SetNotificationVolume(u8),
    SetAutomaticUpdates(bool),
    SetHybridMode(bool),
    /// One of Bridge's own button assignments, by button bit; `None` clears it.
    SetButtonRemap(u8, Option<openmouse_bridge::buttons::Action>),
    SetTrayBatteryStyle(platform::TrayBatteryStyle),
    CheckForUpdates,
    /// Download and stage an update; it applies on "Restart to update".
    InstallUpdate(UpdateInfo),
    /// Restart into the staged update.
    ApplyUpdate,
    AcknowledgeTrayHint,
    AcknowledgeWhatsNew,
    ForgetBattery(String),
}

enum Startup {
    Serving { show_panel: bool },
    AlreadyRunning,
}

pub(crate) enum UpdateEvent {
    /// The release notes of an update about to be staged, as `(version,
    /// notes)`: an automatic install goes straight to `Downloading`, so the
    /// panel would otherwise never see them. Changes no update state.
    Notes(String, String),
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading(String),
    /// Downloaded and verified; waiting for "Restart to update".
    Ready(String),
    Restarting(String),
    Failed(String),
}

/// How often Bridge looks for an update while automatic updates are on, on
/// top of the check at launch.
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

pub(crate) struct BackgroundServer {
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<Result<()>>>,
}

/// Where the runtime sends notices, and the tray app's context to
/// wake so it shows them even while its panel is hidden.
pub(crate) type NoticeSink = Sender<Notice>;

pub(crate) type BackgroundRuntime = (
    BackgroundServer,
    Receiver<BridgeSnapshot>,
    tokio_mpsc::UnboundedSender<BridgeCommand>,
    Receiver<UpdateEvent>,
    Receiver<()>,
    bool,
);

impl BackgroundServer {
    pub(crate) fn start(notices: NoticeSink) -> Result<Option<BackgroundRuntime>> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (snapshot_tx, snapshots) = mpsc::sync_channel(1);
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (update_tx, updates) = mpsc::sync_channel(8);
        let (panel_tx, panel_requests) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("openmouse-bridge-runtime".into())
            .spawn(move || {
                let outcome = run_server(
                    shutdown_rx,
                    ready_tx.clone(),
                    snapshot_tx,
                    command_rx,
                    update_tx,
                    notices,
                    panel_tx,
                );
                if let Err(error) = &outcome {
                    let _ = ready_tx.send(Err(format!("{error:#}")));
                }
                outcome
            })
            .context("could not start the Bridge runtime")?;

        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(Startup::AlreadyRunning)) => {
                let _ = shutdown_tx.send(());
                let _ = thread.join();
                Ok(None)
            }
            Ok(Ok(Startup::Serving { show_panel })) => Ok(Some((
                Self {
                    shutdown: Some(shutdown_tx),
                    thread: Some(thread),
                },
                snapshots,
                command_tx,
                updates,
                panel_requests,
                show_panel,
            ))),
            Ok(Err(error)) => {
                let _ = shutdown_tx.send(());
                let _ = thread.join();
                Err(anyhow!(error))
            }
            Err(error) => {
                let _ = shutdown_tx.send(());
                let _ = thread.join();
                Err(anyhow!("Bridge did not become ready: {error}"))
            }
        }
    }

    pub(crate) fn stop(mut self) -> Result<()> {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        match self.thread.take().map(|thread| thread.join()) {
            Some(Ok(result)) => result,
            Some(Err(_)) => Err(anyhow!("the Bridge runtime thread stopped unexpectedly")),
            None => Ok(()),
        }
    }
}

/// The time since `previous` when it is long enough that the PC must have
/// slept; a backwards clock change is not a resume.
pub(crate) fn resume_gap(previous: SystemTime, now: SystemTime) -> Option<Duration> {
    now.duration_since(previous)
        .ok()
        .filter(|gap| *gap >= RESUME_GAP)
}

pub(crate) fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
        })
        .unwrap_or_else(|| "unknown panic".into())
}

pub(crate) fn current_relaunch_generation() -> u32 {
    env::var(RELAUNCH_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

/// After Glow/winit die (common on Windows hibernate), replace this process
/// unless the user quit or an in-app update is already relaunching. Quick
/// crash loops stop after `MAX_QUICK_RELAUNCHES`; a panel that stayed up
/// for `GUI_STABLE_AFTER` resets the counter.
pub(crate) fn next_relaunch_generation(
    current: u32,
    gui_lived: Duration,
    keep_running: bool,
) -> Option<u32> {
    if !keep_running {
        return None;
    }
    let generation = if gui_lived >= GUI_STABLE_AFTER {
        0
    } else {
        current.saturating_add(1)
    };
    (generation <= MAX_QUICK_RELAUNCHES).then_some(generation)
}

pub(crate) fn restart_bridge(generation: u32) -> Result<()> {
    let executable = env::current_exe().context("could not locate the Bridge executable")?;
    let working_directory = executable
        .parent()
        .context("Bridge executable has no parent directory")?;
    std::process::Command::new(&executable)
        .current_dir(working_directory)
        .env(RELAUNCH_ENV, generation.to_string())
        .spawn()
        .context("could not restart OpenMouse Bridge")?;
    Ok(())
}

async fn activate_running_bridge() -> bool {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::debug!(%error, "could not ask the running Bridge to open its panel");
            return false;
        }
    };
    match client
        .put(format!("http://127.0.0.1:{BRIDGE_PORT}/v1/panel"))
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(error) => {
            tracing::debug!(%error, "the running Bridge did not open its panel");
            false
        }
    }
}

/// How long quitting waits for blocking work still running (a native-hid
/// helper read can take 10 s) before the runtime lets it go.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

fn run_server(
    shutdown: oneshot::Receiver<()>,
    ready: SyncSender<Result<Startup, String>>,
    snapshots: SyncSender<BridgeSnapshot>,
    commands: tokio_mpsc::UnboundedReceiver<BridgeCommand>,
    update_events: SyncSender<UpdateEvent>,
    notices: NoticeSink,
    panel_requests: Sender<()>,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("could not create the Bridge async runtime")?;
    let outcome = runtime.block_on(async move {
        // Bound before any slow startup work, so the API is up at once and a
        // second launch finds this one without fetching or saving anything.
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), BRIDGE_PORT);
        let listener = match TcpListener::bind(address).await {
            Ok(listener) => listener,
            Err(error) => {
                // The first launch still holds 17846. Ask it to open the panel
                // instead of exiting with nothing on screen.
                if activate_running_bridge().await {
                    tracing::info!("OpenMouse Bridge is already running; opening its tray panel");
                    let _ = ready.send(Ok(Startup::AlreadyRunning));
                    return Ok(());
                }
                return Err(error).context(format!(
                    "could not bind http://{address}; is Bridge already running?"
                ));
            }
        };
        let (bridge_config, path, set_aside) = config::load_or_create()?;
        if let Some(kept) = set_aside {
            // Queued for the tray, which shows it once it is up.
            let _ = notices.send(Notice {
                title: "Bridge settings were reset".into(),
                detail: format!(
                    "config.json could not be read, so Bridge started on defaults. The old file is kept as {}",
                    kept.display()
                ),
            });
        }
        let show_panel = !bridge_config.tray_hint_shown;
        let automatic_updates = bridge_config.automatic_updates;
        let origins = bridge_config.allowed_origins.clone();
        let service = BridgeService::new(bridge_config, path.clone());
        let notices = std::sync::Mutex::new(notices);
        service.on_notice(move |notice| {
            let _ = notices.lock().map(|sender| sender.send(notice));
        });
        service.on_show_panel(move || {
            let _ = panel_requests.send(());
        });
        let tasks = start_background(
            &service,
            SystemHost,
            snapshots,
            commands,
            update_events,
            automatic_updates,
        )
        .await;

        tracing::info!(%address, config = %path.display(), "OpenMouse Bridge is ready");
        // Only now is this the one Bridge running, so nothing else can be
        // about to apply a download this removes.
        tokio::task::spawn_blocking(updater::remove_stale_downloads);
        let _ = ready.send(Ok(Startup::Serving { show_panel }));
        // Now that this is the one Bridge running: on Plasma Wayland, have
        // KWin report the focused window (issue #59).
        #[cfg(target_os = "linux")]
        let focus = tokio::task::spawn_blocking(openmouse_bridge::kwin_focus::start)
            .await
            .ok()
            .flatten();
        let outcome = axum::serve(listener, api::make_service(api::router(service, &origins)))
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await;
        // Unloads the KWin script.
        #[cfg(target_os = "linux")]
        if let Some(focus) = focus {
            let _ = tokio::task::spawn_blocking(move || drop(focus)).await;
        }
        tasks.abort();
        outcome?;
        Ok(())
    });
    // Dropping the runtime waits for every blocking task still running,
    // which held up quitting, restarting, the "already running" exit and the
    // update hand-off. The update helper is already started by then.
    runtime.shutdown_timeout(SHUTDOWN_GRACE);
    outcome
}

/// What starting the server reaches outside the process for: the network,
/// HID and the process list, the login item, and the updater's files.
/// [`SystemHost`] is the real one; the tests pass a fake, so
/// [`start_background`] runs without any of them.
trait Host: Clone + Send + Sync + 'static {
    fn refresh_game_catalog(
        &self,
        service: BridgeService,
    ) -> impl Future<Output = ()> + Send + use<Self>;
    fn enable_autostart_once(&self, service: &BridgeService) -> impl Future<Output = Result<()>>;
    /// The game and battery monitors.
    fn start_device_monitors(&self, service: &BridgeService);
    fn is_stub_install(&self) -> bool;
    fn take_last_update_failure(&self) -> Option<String>;
    fn finish_stub_install(
        &self,
        events: SyncSender<UpdateEvent>,
    ) -> impl Future<Output = ()> + Send + use<Self>;
    fn run_update_job(
        &self,
        job: UpdateJob,
        events: SyncSender<UpdateEvent>,
    ) -> impl Future<Output = ()> + Send + use<Self>;
}

#[derive(Clone, Copy)]
struct SystemHost;

impl Host for SystemHost {
    fn refresh_game_catalog(
        &self,
        service: BridgeService,
    ) -> impl Future<Output = ()> + Send + use<> {
        async move { service.refresh_game_catalog().await }
    }

    fn enable_autostart_once(&self, service: &BridgeService) -> impl Future<Output = Result<()>> {
        service.enable_autostart_once()
    }

    fn start_device_monitors(&self, service: &BridgeService) {
        service.start_game_monitor(Arc::new(AtomicBool::new(true)));
        service.start_battery_monitor();
    }

    fn is_stub_install(&self) -> bool {
        updater::is_stub_install()
    }

    fn take_last_update_failure(&self) -> Option<String> {
        updater::take_last_failure()
    }

    fn finish_stub_install(
        &self,
        events: SyncSender<UpdateEvent>,
    ) -> impl Future<Output = ()> + Send + use<> {
        finish_stub_install(events)
    }

    fn run_update_job(
        &self,
        job: UpdateJob,
        events: SyncSender<UpdateEvent>,
    ) -> impl Future<Output = ()> + Send + use<> {
        run_update_job(job, events)
    }
}

/// The tasks [`start_background`] spawned, stopped together with the server.
struct BackgroundTasks {
    catalog_refresh: JoinHandle<()>,
    snapshot_publisher: JoinHandle<()>,
    update_timer: JoinHandle<()>,
    command_handler: JoinHandle<()>,
    update_worker: JoinHandle<()>,
}

impl BackgroundTasks {
    fn abort(self) {
        self.catalog_refresh.abort();
        self.snapshot_publisher.abort();
        self.update_timer.abort();
        self.command_handler.abort();
        self.update_worker.abort();
    }
}

/// Everything the server runs beside its HTTP API once the config is loaded:
/// the catalog refresh, the monitors, the snapshot publisher, the update
/// worker and timer, and the tray command loop.
async fn start_background(
    service: &BridgeService,
    host: impl Host,
    snapshots: SyncSender<BridgeSnapshot>,
    mut commands: tokio_mpsc::UnboundedReceiver<BridgeCommand>,
    update_events: SyncSender<UpdateEvent>,
    automatic_updates: bool,
) -> BackgroundTasks {
    // The cached catalog serves until the fresh one arrives.
    let catalog_refresh = tokio::spawn(host.refresh_game_catalog(service.clone()));
    if let Err(error) = host.enable_autostart_once(service).await {
        tracing::warn!(%error, "Could not turn on launch at login");
    }
    host.start_device_monitors(service);
    // Hybrid mode: restores the stored settings when the web panel lets
    // go of the mouse (does nothing with Hybrid mode off).
    service.start_mode_monitor();

    let snapshot_service = service.clone();
    let snapshot_publisher = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            // Nobody listens in headless mode: stop building snapshots.
            if let Err(mpsc::TrySendError::Disconnected(_)) =
                snapshots.try_send(snapshot_service.snapshot().await)
            {
                return;
            }
        }
    });

    // Update work runs apart from the tray commands, one job at a time in
    // the order asked, so a 10-minute download never holds up a setting.
    let (update_jobs, queued_update_jobs) = tokio_mpsc::unbounded_channel();
    let worker_events = update_events.clone();
    let worker_host = host.clone();
    let update_worker = tokio::spawn(run_serially(queued_update_jobs, move |job| {
        worker_host.run_update_job(job, worker_events.clone())
    }));

    let timer_service = service.clone();
    let timer_jobs = update_jobs.clone();
    let timer_host = host.clone();
    let update_timer = tokio::spawn(async move {
        if timer_host.is_stub_install() {
            return;
        }
        loop {
            tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
            if timer_service.snapshot().await.automatic_updates {
                let _ = timer_jobs.send(UpdateJob::Check {
                    install_automatically: true,
                });
            }
        }
    });

    let command_service = service.clone();
    let command_handler = tokio::spawn(async move {
        // An update helper that failed left a note for this launch.
        if let Some(error) = host.take_last_update_failure() {
            tracing::warn!(%error, "the last Bridge update did not install");
            let _ = update_events.try_send(UpdateEvent::Failed(error));
        }
        // A stub install can't run its HID helper yet; finishing it comes
        // before any ordinary update check.
        if host.is_stub_install() {
            tokio::spawn(host.finish_stub_install(update_events.clone()));
        } else if automatic_updates {
            let _ = update_jobs.send(UpdateJob::Check {
                install_automatically: true,
            });
        }
        while let Some(command) = commands.recv().await {
            match command {
                BridgeCommand::SetBatteryThreshold(percent) => {
                    if let Err(error) = command_service.set_battery_threshold(percent).await {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
                BridgeCommand::SetNotificationSound(enabled) => {
                    if let Err(error) = command_service.set_notification_sound(enabled).await {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
                BridgeCommand::SetNotificationVolume(percent) => {
                    if let Err(error) = command_service.set_notification_volume(percent).await {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
                BridgeCommand::SetAutomaticUpdates(enabled) => {
                    match command_service.set_automatic_updates(enabled).await {
                        Ok(()) if enabled => {
                            let _ = update_jobs.send(UpdateJob::Check {
                                install_automatically: true,
                            });
                        }
                        Ok(()) => {}
                        Err(error) => {
                            tracing::error!(%error, "could not save Bridge settings");
                        }
                    }
                }
                BridgeCommand::SetHybridMode(enabled) => {
                    if let Err(error) = command_service.set_hybrid_mode(enabled).await {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
                BridgeCommand::SetButtonRemap(bit, action) => {
                    if let Err(error) = command_service.set_button_remap(bit, action).await {
                        tracing::error!(%error, "could not save the button assignments");
                    }
                }
                BridgeCommand::SetTrayBatteryStyle(style) => {
                    if let Err(error) = command_service
                        .set_tray_battery_style(style.as_config_str())
                        .await
                    {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
                BridgeCommand::CheckForUpdates => {
                    let _ = update_jobs.send(UpdateJob::Check {
                        install_automatically: false,
                    });
                }
                BridgeCommand::InstallUpdate(update) => {
                    let _ = update_jobs.send(UpdateJob::Install(update));
                }
                // Queued behind a download still running, as before, so
                // it restarts into what that download stages.
                BridgeCommand::ApplyUpdate => {
                    let _ = update_jobs.send(UpdateJob::Apply);
                }
                BridgeCommand::ForgetBattery(device_id) => {
                    command_service.forget_battery(&device_id).await;
                }
                BridgeCommand::AcknowledgeTrayHint => {
                    if let Err(error) = command_service.acknowledge_tray_hint().await {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
                BridgeCommand::AcknowledgeWhatsNew => {
                    if let Err(error) = command_service.acknowledge_whats_new().await {
                        tracing::error!(%error, "could not save Bridge settings");
                    }
                }
            }
        }
    });

    BackgroundTasks {
        catalog_refresh,
        snapshot_publisher,
        update_timer,
        command_handler,
        update_worker,
    }
}

/// Update work the tray asks for, run one at a time by [`run_update_job`].
enum UpdateJob {
    Check {
        install_automatically: bool,
    },
    Install(UpdateInfo),
    /// Restart into the staged update.
    Apply,
}

async fn run_update_job(job: UpdateJob, events: SyncSender<UpdateEvent>) {
    match job {
        UpdateJob::Check {
            install_automatically,
        } => check_for_updates(install_automatically, &events).await,
        UpdateJob::Install(update) => install_update(update, &events).await,
        UpdateJob::Apply => match updater::apply_prepared() {
            Ok(version) => {
                tracing::info!(%version, "restarting into the staged update");
                let _ = events.try_send(UpdateEvent::Restarting(version));
            }
            Err(error) => {
                tracing::error!(%error, "could not start the Bridge update");
                let _ = events.try_send(UpdateEvent::Failed(format!("{error:#}")));
            }
        },
    }
}

/// Runs each queued job to completion before starting the next, in the order
/// queued, until every sender is gone.
async fn run_serially<J, F, Fut>(mut jobs: tokio_mpsc::UnboundedReceiver<J>, mut run: F)
where
    F: FnMut(J) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    while let Some(job) = jobs.recv().await {
        run(job).await;
    }
}

/// Checks `latest.json`. With `install_automatically`, an update this install
/// can apply is downloaded and staged straight away; otherwise (or for one it
/// cannot apply) the panel offers it.
async fn check_for_updates(install_automatically: bool, events: &SyncSender<UpdateEvent>) {
    let _ = events.try_send(UpdateEvent::Checking);
    match updater::check_for_update().await {
        Ok(Some(update)) if install_automatically && update.manual_download_url().is_none() => {
            install_update(update, events).await
        }
        Ok(Some(update)) => {
            let _ = events.try_send(UpdateEvent::Available(update));
        }
        Ok(None) => {
            let _ = events.try_send(UpdateEvent::UpToDate);
        }
        Err(error) => {
            tracing::warn!(%error, "Bridge update check failed");
            let _ = events.try_send(UpdateEvent::Failed(format!("{error:#}")));
        }
    }
}

/// Fetches the full package a bridge install is missing, retrying with a
/// growing delay until it is staged; the update helper then restarts Bridge.
async fn finish_stub_install(events: SyncSender<UpdateEvent>) {
    let mut delay = Duration::from_secs(15);
    loop {
        let _ = events.try_send(UpdateEvent::Checking);
        let outcome = match updater::check_for_stub_completion().await {
            Ok(update) => {
                let version = update.version.clone();
                let _ = events.try_send(UpdateEvent::Downloading(version.clone()));
                // The install is unusable until it has its payload, so it
                // restarts into it at once rather than waiting for the user.
                match updater::prepare(&update).await {
                    Ok(()) => updater::apply_prepared(),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error),
        };
        match outcome {
            Ok(version) => {
                let _ = events.try_send(UpdateEvent::Restarting(version));
                return;
            }
            Err(error) => {
                tracing::warn!(%error, "could not finish installing Bridge; will retry");
                let _ = events.try_send(UpdateEvent::Failed(format!("{error:#}")));
            }
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(15 * 60));
    }
}

/// Downloads and stages `update`; it applies when the user restarts into it.
async fn install_update(update: UpdateInfo, events: &SyncSender<UpdateEvent>) {
    let version = update.version.clone();
    let _ = events.try_send(UpdateEvent::Notes(version.clone(), update.notes.clone()));
    if updater::prepared_version().as_deref() != Some(version.as_str()) {
        let _ = events.try_send(UpdateEvent::Downloading(version.clone()));
    }
    match updater::prepare(&update).await {
        Ok(()) => {
            let _ = events.try_send(UpdateEvent::Ready(version));
        }
        Err(error) => {
            tracing::error!(%error, "Bridge update failed");
            let _ = events.try_send(UpdateEvent::Failed(format!("{error:#}")));
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn configure_tray_only_application() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(marker) = MainThreadMarker::new() else {
        tracing::error!("OpenMouse Bridge must initialize its tray on the macOS main thread");
        return;
    };
    let application = NSApplication::sharedApplication(marker);
    if !application.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        tracing::warn!("macOS refused the tray-only application policy");
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub(crate) fn configure_tray_only_application() {}

pub(crate) fn notify_natively(notice: &Notice) {
    if let Err(error) = platform::notify(&notice.title, &notice.detail) {
        tracing::warn!(%error, "Could not show the notification");
    }
}

pub(crate) fn openmouse_icon_rgba() -> Result<(Vec<u8>, u32, u32)> {
    let mut decoder =
        png::Decoder::new(Cursor::new(include_bytes!("../assets/openmouse-logo.png")));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .context("could not decode the OpenMouse logo")?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .expect("logo is within PNG limits")
    ];
    let info = reader
        .next_frame(&mut pixels)
        .context("could not read the OpenMouse logo")?;
    if info.color_type != png::ColorType::Rgba {
        return Err(anyhow!("the OpenMouse logo must decode as RGBA"));
    }
    pixels.truncate(info.buffer_size());
    Ok((pixels, info.width, info.height))
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(crate) fn tray_icon() -> Result<Icon> {
    let (rgba, width, height) = openmouse_icon_rgba()?;
    Icon::from_rgba(rgba, width, height).context("could not create the OpenMouse tray icon")
}

pub(crate) fn open_logs() -> Result<()> {
    let directory = config::log_dir()?;
    #[cfg(target_os = "windows")]
    let program = "explorer.exe";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(directory)
        .spawn()
        .context("Could not open the Bridge log folder")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_exit_does_not_relaunch() {
        assert_eq!(
            next_relaunch_generation(0, Duration::from_secs(5), false),
            None
        );
    }

    #[test]
    fn unexpected_gui_exit_relaunches() {
        assert_eq!(
            next_relaunch_generation(0, Duration::from_secs(5), true),
            Some(1)
        );
    }

    #[test]
    fn a_stable_gui_resets_the_relaunch_counter() {
        assert_eq!(next_relaunch_generation(4, GUI_STABLE_AFTER, true), Some(0));
    }

    #[test]
    fn normal_ticks_are_not_a_resume() {
        let now = SystemTime::now();
        assert_eq!(resume_gap(now - Duration::from_secs(1), now), None);
        assert_eq!(resume_gap(now - Duration::from_secs(5), now), None);
    }

    #[test]
    fn a_long_gap_is_a_resume() {
        let now = SystemTime::now();
        let slept = Duration::from_secs(3600);
        assert_eq!(resume_gap(now - slept, now), Some(slept));
        assert_eq!(
            resume_gap(now - RESUME_GAP, now),
            Some(RESUME_GAP),
            "the threshold itself counts"
        );
    }

    #[test]
    fn a_clock_set_backwards_is_not_a_resume() {
        let now = SystemTime::now();
        assert_eq!(resume_gap(now + Duration::from_secs(3600), now), None);
    }

    #[tokio::test]
    async fn update_jobs_run_one_at_a_time_in_the_order_queued() {
        let (jobs, queued) = tokio_mpsc::unbounded_channel();
        let (log_tx, mut log) = tokio_mpsc::unbounded_channel::<String>();
        let worker = tokio::spawn(run_serially(
            queued,
            move |(name, gate): (&'static str, oneshot::Receiver<()>)| {
                let log = log_tx.clone();
                async move {
                    let _ = log.send(format!("start {name}"));
                    let _ = gate.await;
                    let _ = log.send(format!("end {name}"));
                }
            },
        ));
        let quiet = Duration::from_millis(50);
        let (open_first, first) = oneshot::channel();
        let (open_second, second) = oneshot::channel();
        // Queuing never waits for the job ahead to finish.
        jobs.send(("first", first)).unwrap();
        jobs.send(("second", second)).unwrap();
        assert_eq!(log.recv().await.as_deref(), Some("start first"));
        assert!(tokio::time::timeout(quiet, log.recv()).await.is_err());
        // A later job that could finish still waits for the one ahead.
        open_second.send(()).unwrap();
        assert!(tokio::time::timeout(quiet, log.recv()).await.is_err());
        open_first.send(()).unwrap();
        for expected in ["end first", "start second", "end second"] {
            assert_eq!(log.recv().await.as_deref(), Some(expected));
        }
        drop(jobs);
        worker.await.unwrap();
    }

    #[test]
    fn quick_crash_loops_stop_relaunching() {
        assert_eq!(
            next_relaunch_generation(MAX_QUICK_RELAUNCHES, Duration::from_secs(1), true),
            None
        );
    }

    /// A [`Host`] with no network, HID, process list or files. Its update
    /// jobs report that they started and then never finish, like a download
    /// that takes its time.
    #[derive(Clone)]
    struct FakeHost {
        update_jobs_started: tokio_mpsc::UnboundedSender<()>,
    }

    impl Host for FakeHost {
        fn refresh_game_catalog(
            &self,
            _: BridgeService,
        ) -> impl Future<Output = ()> + Send + use<> {
            async {}
        }

        async fn enable_autostart_once(&self, _: &BridgeService) -> Result<()> {
            Ok(())
        }

        fn start_device_monitors(&self, _: &BridgeService) {}

        fn is_stub_install(&self) -> bool {
            false
        }

        fn take_last_update_failure(&self) -> Option<String> {
            None
        }

        fn finish_stub_install(
            &self,
            _: SyncSender<UpdateEvent>,
        ) -> impl Future<Output = ()> + Send + use<> {
            async {}
        }

        fn run_update_job(
            &self,
            _: UpdateJob,
            _: SyncSender<UpdateEvent>,
        ) -> impl Future<Output = ()> + Send + use<> {
            let started = self.update_jobs_started.clone();
            async move {
                let _ = started.send(());
                std::future::pending::<()>().await
            }
        }
    }

    /// [`start_background`] over a service whose config lives in a fresh
    /// temp directory, with the ends of its channels the tray would hold.
    struct Wired {
        service: BridgeService,
        tasks: BackgroundTasks,
        commands: tokio_mpsc::UnboundedSender<BridgeCommand>,
        update_jobs_started: tokio_mpsc::UnboundedReceiver<()>,
        _snapshots: Receiver<BridgeSnapshot>,
        _updates: Receiver<UpdateEvent>,
    }

    async fn wire() -> Wired {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = env::temp_dir().join(format!(
            "openmouse-runtime-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        // Hybrid mode off, so the mode monitor never restores anything to a
        // real mouse.
        let bridge_config = config::BridgeConfig {
            automatic_updates: false,
            hybrid_mode: false,
            ..Default::default()
        };
        let service = BridgeService::new(bridge_config, directory.join("config.json"));
        let (snapshot_tx, snapshots) = mpsc::sync_channel(1);
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (update_tx, updates) = mpsc::sync_channel(8);
        let (started_tx, update_jobs_started) = tokio_mpsc::unbounded_channel();
        let host = FakeHost {
            update_jobs_started: started_tx,
        };
        let tasks =
            start_background(&service, host, snapshot_tx, command_rx, update_tx, false).await;
        Wired {
            service,
            tasks,
            commands: command_tx,
            update_jobs_started,
            _snapshots: snapshots,
            _updates: updates,
        }
    }

    #[tokio::test]
    async fn the_server_starts_the_mode_monitor() {
        let wired = wire().await;
        assert_eq!(wired.service.snapshot().await.device_mode, "onboard");

        wired.service.record_client_heartbeat().await;

        // The mode monitor ticks every second; without it the mode never
        // leaves onboard and Hybrid mode does nothing.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while wired.service.snapshot().await.device_mode != "live" {
            assert!(
                std::time::Instant::now() < deadline,
                "the mode did not follow the panel connecting"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        wired.tasks.abort();
    }

    #[tokio::test]
    async fn a_setting_lands_while_an_update_job_is_still_running() {
        let mut wired = wire().await;
        wired.commands.send(BridgeCommand::CheckForUpdates).unwrap();
        wired
            .commands
            .send(BridgeCommand::SetBatteryThreshold(42))
            .unwrap();

        tokio::time::timeout(Duration::from_secs(5), wired.update_jobs_started.recv())
            .await
            .expect("the update check never started")
            .unwrap();
        // The check never finishes; the setting after it still has to land.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while wired.service.config().await.battery_threshold_percent != 42 {
            assert!(
                std::time::Instant::now() < deadline,
                "the tray command waited on the update job"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        wired.tasks.abort();
    }
}
