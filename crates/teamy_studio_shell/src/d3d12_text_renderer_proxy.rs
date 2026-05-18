use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread;

use windows::Win32::Foundation::HWND;

use crate::{RenderScene, TextRendererHost};

pub struct TextRendererThreadProxy {
    shared: Arc<(Mutex<TextRendererThreadState>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
}

#[derive(Default)]
struct TextRendererThreadState {
    pending_scene: Option<RenderScene>,
    submitted_generation: u64,
    completed_generation: u64,
    shutdown: bool,
    error: Option<String>,
}

impl TextRendererThreadProxy {
    pub fn new(hwnd: HWND, initial_scene: RenderScene) -> eyre::Result<Self> {
        let shared = Arc::new((
            Mutex::new(TextRendererThreadState::default()),
            Condvar::new(),
        ));
        let shared_for_worker = Arc::clone(&shared);
        let (startup_tx, startup_rx) = mpsc::sync_channel(1);
        let raw_hwnd = hwnd.0 as isize;

        let worker = thread::Builder::new()
            .name("teamy-shell-text-renderer".to_owned())
            .spawn(move || {
                let startup =
                    TextRendererHost::new(HWND(raw_hwnd as *mut core::ffi::c_void), &initial_scene);
                match startup {
                    Ok(mut host) => {
                        let _ = startup_tx.send(Ok(()));
                        run_text_renderer_thread_loop(&shared_for_worker, &mut host);
                    }
                    Err(error) => {
                        let message =
                            format!("failed to create shell text renderer thread: {error:#}");
                        if let Ok(mut state) = shared_for_worker.0.lock() {
                            state.error = Some(message.clone());
                        }
                        let _ = startup_tx.send(Err(eyre::eyre!(message)));
                    }
                }
            })
            .map_err(|error| eyre::eyre!("failed to spawn shell text renderer thread: {error}"))?;

        startup_rx.recv().map_err(|error| {
            eyre::eyre!("text renderer thread failed to report startup: {error}")
        })??;

        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub fn upload_scene(&self, scene: RenderScene) -> eyre::Result<()> {
        self.check_error()?;
        let (state_lock, wake) = &*self.shared;
        let mut state = state_lock
            .lock()
            .map_err(|error| eyre::eyre!("failed to lock text renderer thread state: {error}"))?;
        state.submitted_generation += 1;
        state.pending_scene = Some(scene);
        wake.notify_one();
        Ok(())
    }

    pub fn upload_scene_blocking(&self, scene: RenderScene) -> eyre::Result<()> {
        self.check_error()?;
        let (state_lock, wake) = &*self.shared;
        let mut state = state_lock
            .lock()
            .map_err(|error| eyre::eyre!("failed to lock text renderer thread state: {error}"))?;
        state.submitted_generation += 1;
        let target_generation = state.submitted_generation;
        state.pending_scene = Some(scene);
        wake.notify_one();

        while state.completed_generation < target_generation {
            if let Some(error) = state.error.as_ref() {
                eyre::bail!(error.clone());
            }
            state = wake.wait(state).map_err(|error| {
                eyre::eyre!("failed to wait for text renderer thread completion: {error}")
            })?;
        }

        if let Some(error) = state.error.as_ref() {
            eyre::bail!(error.clone());
        }
        Ok(())
    }

    fn check_error(&self) -> eyre::Result<()> {
        let state =
            self.shared.0.lock().map_err(|error| {
                eyre::eyre!("failed to lock text renderer thread state: {error}")
            })?;
        if let Some(error) = state.error.as_ref() {
            eyre::bail!(error.clone());
        }
        Ok(())
    }
}

impl Drop for TextRendererThreadProxy {
    fn drop(&mut self) {
        let (state_lock, wake) = &*self.shared;
        if let Ok(mut state) = state_lock.lock() {
            state.shutdown = true;
            wake.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_text_renderer_thread_loop(
    shared: &Arc<(Mutex<TextRendererThreadState>, Condvar)>,
    host: &mut TextRendererHost,
) {
    loop {
        let pending_scene = {
            let (state_lock, wake) = &**shared;
            let Ok(mut state) = state_lock.lock() else {
                return;
            };

            while !state.shutdown && state.pending_scene.is_none() && state.error.is_none() {
                match wake.wait(state) {
                    Ok(next_state) => state = next_state,
                    Err(_) => return,
                }
            }

            if state.shutdown {
                return;
            }

            state.pending_scene.take()
        };

        let Some(scene) = pending_scene else {
            continue;
        };

        let result = host.upload_scene(&scene);
        let (state_lock, wake) = &**shared;
        let Ok(mut state) = state_lock.lock() else {
            return;
        };
        match result {
            Ok(()) => {
                state.completed_generation = state.submitted_generation;
            }
            Err(error) => {
                state.error = Some(format!("text renderer thread upload failed: {error:#}"));
            }
        }
        wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::TextRendererThreadProxy;

    #[test]
    fn text_renderer_thread_proxy_type_exists() {
        assert!(std::mem::size_of::<TextRendererThreadProxy>() > 0);
    }
}
