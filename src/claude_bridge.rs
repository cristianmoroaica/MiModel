use crate::claude;
use crate::tui::BackgroundResult;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

/// Whether a background task is running.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BusyState {
    Idle,
    Thinking,
    Building,
}

/// Owns all Claude CLI interaction state: channels, PID tracking,
/// model selection, session continuity, and streaming text buffer.
pub struct ClaudeBridge {
    // Channels
    bg_tx: mpsc::Sender<BackgroundResult>,
    bg_rx: mpsc::Receiver<BackgroundResult>,
    stream_tx: mpsc::Sender<String>,
    stream_rx: mpsc::Receiver<String>,
    bg_pid: Arc<AtomicU32>,

    // State
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub streaming_text: String,
    pub busy: BusyState,
}

impl ClaudeBridge {
    /// Create a new bridge with channels and initial state.
    pub fn new(model: Option<String>) -> Self {
        let (bg_tx, bg_rx) = mpsc::channel::<BackgroundResult>();
        let (stream_tx, stream_rx) = mpsc::channel::<String>();
        let bg_pid = Arc::new(AtomicU32::new(0));

        ClaudeBridge {
            bg_tx,
            bg_rx,
            stream_tx,
            stream_rx,
            bg_pid,
            model,
            session_id: None,
            streaming_text: String::new(),
            busy: BusyState::Idle,
        }
    }

    /// Drain stream_rx via try_recv loop, appending to streaming_text.
    /// Returns true if any chunks were received.
    pub fn drain_streaming(&mut self) -> bool {
        let mut got = false;
        while let Ok(chunk) = self.stream_rx.try_recv() {
            self.streaming_text.push_str(&chunk);
            got = true;
        }
        got
    }

    /// Non-blocking check for a completed background result.
    pub fn try_recv_result(&self) -> Option<BackgroundResult> {
        self.bg_rx.try_recv().ok()
    }

    /// Send SIGTERM to the background Claude subprocess (if any).
    pub fn cancel(&self) {
        let pid = self.bg_pid.load(Ordering::SeqCst);
        if pid != 0 {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }

    /// Spawn a background thread that calls `claude::send_with_phase_prompt`
    /// and sends the result via bg_tx.
    ///
    /// This replaces the duplicated thread-spawn pattern across all send methods.
    pub fn send_phase_prompt(
        &mut self,
        phase_name: &str,
        prompt: &str,
        images: &[PathBuf],
        ref_context: Option<&str>,
    ) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.model.clone();
        let session_id = self.session_id.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let phase_name = phase_name.to_string();
        let prompt = prompt.to_string();
        let images = images.to_vec();
        let ref_context = ref_context.map(|s| s.to_string());

        std::thread::spawn(move || {
            let result = claude::send_with_phase_prompt(
                &model,
                &phase_name,
                session_id.as_deref(),
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
                ref_context.as_deref(),
            );
            bg_pid.store(0, Ordering::SeqCst);
            match result {
                Ok((response, new_sid)) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Ok(response),
                        session_id: new_sid.or(session_id),
                    });
                }
                Err(e) => {
                    let _ = tx.send(BackgroundResult::ClaudeResponse {
                        result: Err(e),
                        session_id: None,
                    });
                }
            }
        });
    }

    /// Spawn a background thread that calls `claude::send_prompt` directly
    /// (no phase system prompt). Used for `/ref` research.
    pub fn send_raw_prompt(
        &mut self,
        system_prompt: &str,
        prompt: &str,
        images: &[PathBuf],
        result_name: &str,
    ) {
        self.busy = BusyState::Thinking;
        self.streaming_text.clear();

        let model = self.model.clone();
        let tx = self.bg_tx.clone();
        let stream_tx = self.stream_tx.clone();
        let bg_pid = Arc::clone(&self.bg_pid);
        let system_prompt = system_prompt.to_string();
        let prompt = prompt.to_string();
        let images = images.to_vec();
        let result_name = result_name.to_string();

        std::thread::spawn(move || {
            let result = claude::send_prompt(
                &model,
                &system_prompt,
                None,
                &prompt,
                &images,
                Some(&stream_tx),
                Some(&bg_pid),
            );
            bg_pid.store(0, Ordering::SeqCst);
            let _ = tx.send(BackgroundResult::ReferenceResearch {
                name: result_name,
                result: result.map(|(response, _sid)| response),
            });
        });
    }
}
