use eyre::{Result, eyre};
use teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID;
use teamy_studio_cursor_gallery::CursorGalleryState;
use teamy_studio_event_core::{PublishedEvent, WritableArena};
use teamy_studio_main_menu::{
    FeatureValidationState, MainMenuLogicalButtonId, MainMenuSnapshot, registered_button_classes,
};
use teamy_studio_registration_core::{
    RegistrationSnapshot, snapshot, trigger_registrations_for, validate_registrations,
};
use teamy_studio_shell::{HostedWindowRecord, ShellRuntime, ShellState};
use teamy_studio_timeline_core::{CanonicalTimeKey, ConstructedTimeline, TriggerRuntime};

#[derive(Debug)]
pub struct AppComposition {
    pub registration_snapshot: RegistrationSnapshot,
    pub menu_snapshot: MainMenuSnapshot,
    pub cursor_gallery_state: CursorGalleryState,
    pub shell_hosted_windows: Vec<HostedWindowRecord>,
    pub shell_state: ShellState,
    pub timeline: ConstructedTimeline<PublishedEvent>,
}

#[derive(Debug)]
pub struct StartupSession {
    registration_snapshot: RegistrationSnapshot,
    menu_snapshot: MainMenuSnapshot,
    runtime: StartupRuntime,
}

#[derive(Debug)]
pub struct StartupRuntime {
    timeline: ConstructedTimeline<PublishedEvent>,
    trigger_runtime: TriggerRuntime,
    cursor_gallery_state: CursorGalleryState,
    shell_runtime: ShellRuntime,
}

impl Default for StartupRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl StartupRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_shell_runtime(ShellRuntime::new())
    }

    #[must_use]
    pub fn new_with_native_shell() -> Self {
        Self::new_with_shell_runtime(ShellRuntime::new_with_native_hosting())
    }

    #[must_use]
    pub fn new_with_shell_runtime(shell_runtime: ShellRuntime) -> Self {
        Self {
            timeline: ConstructedTimeline::new(),
            trigger_runtime: TriggerRuntime::default(),
            cursor_gallery_state: CursorGalleryState::default(),
            shell_runtime,
        }
    }

    pub fn publish(
        &mut self,
        arena_name: &'static str,
        time_key: CanonicalTimeKey,
        event: PublishedEvent,
    ) {
        let mut epoch = WritableArena::new(arena_name);
        epoch.push(event);
        self.timeline.ingest(time_key, epoch.seal());
    }

    pub fn run_registered_trigger_stage(&mut self, time_key: CanonicalTimeKey) -> usize {
        let mut emitted_events = Vec::new();
        self.trigger_runtime
            .pump_unseen(&self.timeline, |_, event| {
                for registration in trigger_registrations_for(event.definition_id()) {
                    emitted_events.extend((registration.handler)(event));
                }
                Ok::<(), ()>(())
            })
            .expect("registered trigger stage should be infallible");

        if emitted_events.is_empty() {
            return 0;
        }

        for event in &emitted_events {
            teamy_studio_cursor_gallery::apply_published_event(
                &mut self.cursor_gallery_state,
                event,
            );
            self.shell_runtime.apply_published_event(event);
        }

        let mut epoch = WritableArena::new("teamy_studio.trigger_runtime");
        for event in emitted_events {
            epoch.push(event);
        }
        self.timeline.ingest(time_key, epoch.seal());
        1
    }

    pub fn run_shell_host_stage(&mut self, time_key: CanonicalTimeKey) -> Result<usize> {
        let emitted_epoch_count = self
            .shell_runtime
            .run_host_stage(&mut self.timeline, time_key)?;

        if emitted_epoch_count == 0 {
            return Ok(0);
        }

        let latest_epoch = self
            .timeline
            .published_epochs()
            .last()
            .expect("shell runtime emitted an epoch but timeline is empty");
        for event in latest_epoch.1.events() {
            teamy_studio_cursor_gallery::apply_published_event(
                &mut self.cursor_gallery_state,
                event,
            );
        }

        Ok(emitted_epoch_count)
    }

    #[must_use]
    pub fn cursor_gallery_state(&self) -> &CursorGalleryState {
        &self.cursor_gallery_state
    }

    #[must_use]
    pub fn shell_state(&self) -> &ShellState {
        self.shell_runtime.state()
    }

    #[must_use]
    pub fn shell_hosted_windows(&self) -> &[HostedWindowRecord] {
        self.shell_runtime.hosted_windows()
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        ConstructedTimeline<PublishedEvent>,
        CursorGalleryState,
        Vec<HostedWindowRecord>,
        ShellState,
    ) {
        let (shell_host_scaffold, shell_state) = self.shell_runtime.into_parts();

        (
            self.timeline,
            self.cursor_gallery_state,
            shell_host_scaffold.hosted_windows().to_vec(),
            shell_state,
        )
    }
}

impl StartupSession {
    #[must_use]
    pub fn registration_snapshot(&self) -> RegistrationSnapshot {
        self.registration_snapshot
    }

    #[must_use]
    pub fn menu_snapshot(&self) -> &MainMenuSnapshot {
        &self.menu_snapshot
    }

    pub fn publish_event(
        &mut self,
        arena_name: &'static str,
        time_key: CanonicalTimeKey,
        event: PublishedEvent,
    ) {
        self.runtime.publish(arena_name, time_key, event);
    }

    /// Publish a main-menu click event into the startup session timeline.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested logical button cannot be resolved into
    /// a publishable click event from the current menu snapshot.
    pub fn publish_main_menu_click(
        &mut self,
        logical_button_id: MainMenuLogicalButtonId,
        pointer_x: i32,
        pointer_y: i32,
        layout_revision: u64,
        time_key: CanonicalTimeKey,
    ) -> Result<()> {
        let click_event = self
            .menu_snapshot
            .publish_click(logical_button_id, pointer_x, pointer_y, layout_revision)
            .ok_or_else(|| eyre!("requested main menu button could not publish a click event"))?;
        self.publish_event("teamy_studio.main_menu", time_key, click_event);
        Ok(())
    }

    /// Pump registered triggers and shell host stages until the session goes idle.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the shell host stages fails while materializing
    /// a native window.
    pub fn pump_to_idle(
        &mut self,
        start_time_key: CanonicalTimeKey,
        max_stages: usize,
    ) -> Result<usize> {
        let mut emitted_epoch_count = 0;
        let mut next_time_key = start_time_key.0;
        for _ in 0..max_stages {
            let registered_emitted = self
                .runtime
                .run_registered_trigger_stage(CanonicalTimeKey(next_time_key));
            emitted_epoch_count += registered_emitted;
            next_time_key += 1;

            let shell_emitted = self
                .runtime
                .run_shell_host_stage(CanonicalTimeKey(next_time_key))?;
            emitted_epoch_count += shell_emitted;
            next_time_key += 1;

            if registered_emitted == 0 && shell_emitted == 0 {
                break;
            }
        }
        Ok(emitted_epoch_count)
    }

    #[must_use]
    pub fn cursor_gallery_state(&self) -> &CursorGalleryState {
        self.runtime.cursor_gallery_state()
    }

    #[must_use]
    pub fn shell_state(&self) -> &ShellState {
        self.runtime.shell_state()
    }

    #[must_use]
    pub fn shell_hosted_windows(&self) -> &[HostedWindowRecord] {
        self.runtime.shell_hosted_windows()
    }

    #[must_use]
    pub fn into_composition(self) -> AppComposition {
        let (timeline, cursor_gallery_state, shell_hosted_windows, shell_state) =
            self.runtime.into_parts();

        AppComposition {
            registration_snapshot: self.registration_snapshot,
            menu_snapshot: self.menu_snapshot,
            cursor_gallery_state,
            shell_hosted_windows,
            shell_state,
            timeline,
        }
    }

    /// Destroy any native windows currently owned by this session.
    ///
    /// # Errors
    ///
    /// Returns an error if the session cannot destroy one of its native windows.
    pub fn destroy_native_windows(&mut self) -> Result<()> {
        self.runtime.shell_runtime.destroy_native_windows()
    }
}

/// Build a validated MVP startup session without publishing any initial user event.
///
/// # Errors
///
/// Returns an error if static registration validation fails while bootstrapping
/// the menu snapshot and registration snapshot for the MVP stack.
pub fn build_mvp_session() -> Result<StartupSession> {
    build_mvp_session_with_runtime(StartupRuntime::new())
}

/// Build a validated MVP startup session configured to use native shell hosting.
///
/// # Errors
///
/// Returns an error if static registration validation fails while bootstrapping
/// the menu snapshot and registration snapshot for the MVP stack.
pub fn build_mvp_session_with_native_shell() -> Result<StartupSession> {
    build_mvp_session_with_runtime(StartupRuntime::new_with_native_shell())
}

fn build_mvp_session_with_runtime(runtime: StartupRuntime) -> Result<StartupSession> {
    validate_registrations()?;

    let button_classes = registered_button_classes();
    let mut menu_snapshot = MainMenuSnapshot::from_registrations(&button_classes);
    menu_snapshot.set_validation_state_for_class_id(
        CURSOR_GALLERY_BUTTON_CLASS_ID,
        FeatureValidationState::Validated,
    );
    let registration_snapshot = snapshot();

    Ok(StartupSession {
        registration_snapshot,
        menu_snapshot,
        runtime,
    })
}

/// Build the MVP composition using the default simulated shell-host path.
///
/// # Errors
///
/// Returns an error if startup validation fails or the initial cursor-gallery
/// event chain cannot be composed successfully.
pub fn build_mvp_composition() -> Result<AppComposition> {
    let session = build_mvp_session()?;
    run_default_cursor_gallery_flow(session)
}

/// Build the MVP composition while routing shell window creation through native hosting.
///
/// # Errors
///
/// Returns an error if startup validation fails or native shell window creation
/// fails during the initial cursor-gallery flow.
pub fn build_mvp_composition_with_native_shell() -> Result<AppComposition> {
    let session = build_mvp_session_with_native_shell()?;
    run_default_cursor_gallery_flow(session)
}

fn run_default_cursor_gallery_flow(mut session: StartupSession) -> Result<AppComposition> {
    let clicked_button = session
        .menu_snapshot()
        .button_by_class_id(CURSOR_GALLERY_BUTTON_CLASS_ID)
        .cloned()
        .ok_or_else(|| eyre!("cursor gallery button was not registered for the MVP composition"))?;

    session.publish_main_menu_click(
        clicked_button.logical_button_id,
        64,
        32,
        1,
        CanonicalTimeKey(0),
    )?;
    let emitted_epoch_count = session.pump_to_idle(CanonicalTimeKey(1), 8)?;

    if emitted_epoch_count == 0
        || session.cursor_gallery_state().windows().is_empty()
        || session.shell_state().windows().is_empty()
    {
        return Err(eyre!(
            "registered triggers did not produce the default cursor-gallery window flow"
        ));
    }

    Ok(session.into_composition())
}

#[cfg(test)]
mod tests {
    use super::{
        AppComposition, build_mvp_composition, build_mvp_session,
        build_mvp_session_with_native_shell,
    };
    use teamy_studio_cursor_gallery::CURSOR_GALLERY_OPEN_INTENT_DEFINITION;
    use teamy_studio_main_menu::{FeatureValidationState, MAIN_MENU_CLICKED_EVENT_DEFINITION};
    use teamy_studio_shell::{
        InitialWindowCommand, RendererHostMode, WINDOW_CREATE_REQUEST_EVENT_DEFINITION,
        WINDOW_CREATED_EVENT_DEFINITION, WindowHostOptions,
    };
    use teamy_studio_timeline_core::CanonicalTimeKey;

    #[test]
    fn mvp_composition_publishes_pure_event_chain() {
        let composition: AppComposition =
            build_mvp_composition().expect("mvp composition should build");
        let published = composition.timeline.published_epochs();

        assert_eq!(composition.menu_snapshot.buttons().len(), 16);
        assert_eq!(
            composition
                .menu_snapshot
                .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
                .expect("cursor gallery button should exist")
                .validation_state,
            FeatureValidationState::Validated
        );
        assert_eq!(
            composition.menu_snapshot.buttons()[0].validation_state,
            FeatureValidationState::Pending
        );
        assert!(
            composition
                .cursor_gallery_state
                .pending_window_ids()
                .is_empty()
        );
        assert_eq!(composition.cursor_gallery_state.windows().len(), 1);
        assert_eq!(composition.shell_hosted_windows.len(), 1);
        assert_eq!(
            composition.shell_hosted_windows[0].hosted_window_id.raw(),
            1
        );
        assert_eq!(
            composition.shell_hosted_windows[0].host_options,
            WindowHostOptions::standard_foreground()
        );
        assert_eq!(
            composition.shell_hosted_windows[0]
                .native_host_plan
                .initial_command,
            InitialWindowCommand::Show
        );
        assert!(
            composition.shell_hosted_windows[0]
                .native_host_plan
                .bring_to_front
        );
        assert_eq!(
            composition.shell_hosted_windows[0].renderer_host_mode,
            RendererHostMode::LowLatencyHwnd
        );
        assert!(composition.shell_state.pending_requests().is_empty());
        assert_eq!(composition.shell_state.windows().len(), 1);
        assert_eq!(published.len(), 4);
        assert_eq!(published[0].1.events().len(), 1);
        assert_eq!(published[1].1.events().len(), 1);
        assert_eq!(published[2].1.events().len(), 1);
        assert_eq!(published[3].1.events().len(), 1);
        assert_eq!(
            published[0].1.events()[0].definition().id,
            MAIN_MENU_CLICKED_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[1].1.events()[0].definition().id,
            CURSOR_GALLERY_OPEN_INTENT_DEFINITION.id
        );
        assert_eq!(
            published[2].1.events()[0].definition().id,
            WINDOW_CREATE_REQUEST_EVENT_DEFINITION.id
        );
        assert_eq!(
            published[3].1.events()[0].definition().id,
            WINDOW_CREATED_EVENT_DEFINITION.id
        );
    }

    #[test]
    fn mvp_session_can_accept_external_menu_clicks() {
        let mut session = build_mvp_session().expect("mvp session should build");
        let button = session
            .menu_snapshot()
            .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
            .expect("cursor gallery button should exist")
            .clone();

        session
            .publish_main_menu_click(button.logical_button_id, 11, 22, 7, CanonicalTimeKey(50))
            .expect("session should publish menu clicks");

        let emitted_epoch_count = session
            .pump_to_idle(CanonicalTimeKey(51), 8)
            .expect("session should pump to idle");

        assert_eq!(session.shell_hosted_windows().len(), 1);
        assert_eq!(
            session.shell_hosted_windows()[0].host_options,
            WindowHostOptions::standard_foreground()
        );
        assert_eq!(
            session.shell_hosted_windows()[0]
                .native_host_plan
                .initial_command,
            InitialWindowCommand::Show
        );
        assert_eq!(
            session.shell_hosted_windows()[0].renderer_host_mode,
            RendererHostMode::LowLatencyHwnd
        );

        let composition = session.into_composition();

        assert!(emitted_epoch_count >= 1);
        assert_eq!(
            composition
                .menu_snapshot
                .button_by_class_id(teamy_studio_cursor_gallery::CURSOR_GALLERY_BUTTON_CLASS_ID)
                .expect("cursor gallery button should exist")
                .logical_button_id,
            button.logical_button_id
        );
        assert_eq!(composition.cursor_gallery_state.windows().len(), 1);
        assert_eq!(composition.shell_hosted_windows.len(), 1);
        assert_eq!(composition.shell_state.windows().len(), 1);
        assert_eq!(composition.timeline.published_epochs()[0].0.time_key.0, 50);
    }

    #[test]
    fn mvp_session_exposes_legacy_launcher_button_catalog() {
        let session = build_mvp_session().expect("mvp session should build");
        let titles = session
            .menu_snapshot()
            .buttons()
            .iter()
            .map(|button| button.title)
            .collect::<Vec<_>>();

        assert_eq!(
            titles,
            vec![
                "Terminal",
                "Cursor Info",
                "Cursor Gallery",
                "Cursor Latency Playground",
                "Text Rendering Playground",
                "Demo Mode",
                "Storage",
                "Environment Variables",
                "Application Windows",
                "Audio",
                "Audio Daemon",
                "Jobs",
                "Logs",
                "Audio Devices",
                "Timeline",
                "Timeline Playground",
            ]
        );
    }

    #[test]
    fn native_shell_session_starts_with_native_host_mode_and_no_windows() {
        let mut session =
            build_mvp_session_with_native_shell().expect("native-shell mvp session should build");

        assert!(session.shell_hosted_windows().is_empty());

        session
            .destroy_native_windows()
            .expect("empty native-shell session should clean up");
    }
}
