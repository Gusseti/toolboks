//! Generic administrator helper launcher. The host knows no operation names or payloads.
use super::*;
use toolboks_addon_proto::{AdminOutcome, AdminRequest};

pub(crate) struct AdminJob {
    id: u64,
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<AdminOutcome>,
}
impl Drop for AdminJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}
impl AdminJob {
    pub(super) fn installed(shared: Shared, spec: LaunchSpec, request: AdminRequest) -> Self {
        let ctx = shared.ctx.clone();
        Self::start(ctx, request, move |cancel| {
            launch(&shared, &spec, true, cancel)
        })
    }
    fn start(
        ctx: egui::Context,
        request: AdminRequest,
        launch: impl FnOnce(&AtomicBool) -> Result<Started, LaunchError> + Send + 'static,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::channel();
        let thread_cancel = cancel.clone();
        let id = request.id;
        let fallback = tx.clone();
        if let Err(e) = std::thread::Builder::new()
            .name("administrator-helper".into())
            .spawn(move || {
                let outcome = match launch(&thread_cancel) {
                    Ok(started) => exchange(started, request, &thread_cancel),
                    Err(LaunchError::Stop(StopReason::Declined)) => AdminOutcome::Cancelled,
                    Err(LaunchError::Stop(reason)) => AdminOutcome::Failed {
                        reason: reason.text(),
                    },
                    Err(LaunchError::Failed(reason)) => AdminOutcome::Failed { reason },
                };
                let _ = tx.send(outcome);
                ctx.request_repaint();
            })
        {
            let _ = fallback.send(AdminOutcome::Failed {
                reason: e.to_string(),
            });
        }
        Self { id, cancel, result }
    }
    pub(crate) fn poll(&self) -> Option<(u64, AdminOutcome)> {
        match self.result.try_recv() {
            Ok(outcome) => Some((self.id, outcome)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some((
                self.id,
                AdminOutcome::Failed {
                    reason: "The administrator helper closed without a result.".into(),
                },
            )),
        }
    }
}

fn exchange(started: Started, request: AdminRequest, cancel: &AtomicBool) -> AdminOutcome {
    let Started {
        process: _process,
        mut reader,
        writer,
        admin_capabilities,
        ..
    } = started;
    let mut writer = FrameWriter::new(writer);
    if cancel.load(Ordering::SeqCst) {
        return AdminOutcome::Cancelled;
    }
    let id = request.id;
    if let Err(e) = writer.write(&HostMessage::AdminOperation {
        request,
        capabilities: admin_capabilities,
    }) {
        return AdminOutcome::Failed {
            reason: e.to_string(),
        };
    }
    let (tx, rx) = mpsc::channel();
    if let Err(e) = std::thread::Builder::new()
        .name("administrator-result".into())
        .spawn(move || {
            let _ = tx.send(reader.read::<AddonMessage>());
        })
    {
        return AdminOutcome::Failed {
            reason: e.to_string(),
        };
    }
    loop {
        if cancel.load(Ordering::SeqCst) {
            // Dropping the writer closes the pipe; the elevated process cancels its dialog.
            return AdminOutcome::Cancelled;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(AddonMessage::AdminResult {
                id: returned,
                outcome,
            })) if returned == id => return outcome,
            Ok(Err(e)) => {
                return AdminOutcome::Failed {
                    reason: e.to_string(),
                }
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                return AdminOutcome::Failed {
                    reason: "The administrator helper returned an invalid result.".into(),
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}
