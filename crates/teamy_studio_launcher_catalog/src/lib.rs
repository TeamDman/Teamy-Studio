use linkme::distributed_slice;
use teamy_studio_main_menu::{
    MAIN_MENU_BUTTON_CLASS_REGISTRATIONS, MainMenuButtonClassId, MainMenuButtonClassRegistration,
};

pub const TERMINAL_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x31; 16]);
pub const CURSOR_INFO_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x32; 16]);
pub const CURSOR_LATENCY_PLAYGROUND_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x33; 16]);
pub const TEXT_RENDERING_PLAYGROUND_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x34; 16]);
pub const DEMO_MODE_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x35; 16]);
pub const STORAGE_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x36; 16]);
pub const ENVIRONMENT_VARIABLES_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x37; 16]);
pub const APPLICATION_WINDOWS_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x38; 16]);
pub const AUDIO_PICKER_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x39; 16]);
pub const AUDIO_DAEMON_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x3A; 16]);
pub const JOBS_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x3B; 16]);
pub const LOGS_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x3C; 16]);
pub const AUDIO_DEVICES_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x3D; 16]);
pub const TIMELINE_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x3E; 16]);
pub const TIMELINE_PLAYGROUND_BUTTON_CLASS_ID: MainMenuButtonClassId =
    MainMenuButtonClassId::from_bytes([0x3F; 16]);

macro_rules! register_launcher_button {
    ($name:ident, $id:expr, $title:literal, $tooltip:literal, $ordinal:expr) => {
        #[distributed_slice(MAIN_MENU_BUTTON_CLASS_REGISTRATIONS)]
        pub static $name: MainMenuButtonClassRegistration = MainMenuButtonClassRegistration {
            class_id: $id,
            title: $title,
            tooltip: $tooltip,
            ordinal: $ordinal,
        };
    };
}

register_launcher_button!(
    TERMINAL_MAIN_MENU_BUTTON,
    TERMINAL_BUTTON_CLASS_ID,
    "Terminal",
    "Open terminal",
    10
);
register_launcher_button!(
    CURSOR_INFO_MAIN_MENU_BUTTON,
    CURSOR_INFO_BUTTON_CLASS_ID,
    "Cursor Info",
    "Open cursor-info",
    20
);
register_launcher_button!(
    CURSOR_LATENCY_PLAYGROUND_MAIN_MENU_BUTTON,
    CURSOR_LATENCY_PLAYGROUND_BUTTON_CLASS_ID,
    "Cursor Latency Playground",
    "Compare app-drawn cursor behavior against the OS cursor",
    40
);
register_launcher_button!(
    TEXT_RENDERING_PLAYGROUND_MAIN_MENU_BUTTON,
    TEXT_RENDERING_PLAYGROUND_BUTTON_CLASS_ID,
    "Text Rendering Playground",
    "Open the multi-window text rendering playground",
    50
);
register_launcher_button!(
    DEMO_MODE_MAIN_MENU_BUTTON,
    DEMO_MODE_BUTTON_CLASS_ID,
    "Demo Mode",
    "Open demo privacy controls",
    60
);
register_launcher_button!(
    STORAGE_MAIN_MENU_BUTTON,
    STORAGE_BUTTON_CLASS_ID,
    "Storage",
    "Storage is not implemented yet",
    70
);
register_launcher_button!(
    ENVIRONMENT_VARIABLES_MAIN_MENU_BUTTON,
    ENVIRONMENT_VARIABLES_BUTTON_CLASS_ID,
    "Environment Variables",
    "Environment-variable inspector is not implemented yet",
    80
);
register_launcher_button!(
    APPLICATION_WINDOWS_MAIN_MENU_BUTTON,
    APPLICATION_WINDOWS_BUTTON_CLASS_ID,
    "Application Windows",
    "Application-window inspector is not implemented yet",
    90
);
register_launcher_button!(
    AUDIO_PICKER_MAIN_MENU_BUTTON,
    AUDIO_PICKER_BUTTON_CLASS_ID,
    "Audio",
    "Choose audio source",
    100
);
register_launcher_button!(
    AUDIO_DAEMON_MAIN_MENU_BUTTON,
    AUDIO_DAEMON_BUTTON_CLASS_ID,
    "Audio Daemon",
    "Inspect transcription daemon status",
    110
);
register_launcher_button!(
    JOBS_MAIN_MENU_BUTTON,
    JOBS_BUTTON_CLASS_ID,
    "Jobs",
    "Inspect background work",
    120
);
register_launcher_button!(
    LOGS_MAIN_MENU_BUTTON,
    LOGS_BUTTON_CLASS_ID,
    "Logs",
    "Inspect application logs",
    130
);
register_launcher_button!(
    AUDIO_DEVICES_MAIN_MENU_BUTTON,
    AUDIO_DEVICES_BUTTON_CLASS_ID,
    "Audio Devices",
    "Choose microphone input device",
    140
);
register_launcher_button!(
    TIMELINE_MAIN_MENU_BUTTON,
    TIMELINE_BUTTON_CLASS_ID,
    "Timeline",
    "Open timeline workspace",
    150
);
register_launcher_button!(
    TIMELINE_PLAYGROUND_MAIN_MENU_BUTTON,
    TIMELINE_PLAYGROUND_BUTTON_CLASS_ID,
    "Timeline Playground",
    "Play with synthetic timeline render plans",
    160
);

#[cfg(test)]
mod tests {
    use teamy_studio_main_menu::registered_button_classes;

    #[test]
    fn launcher_catalog_registers_legacy_launcher_buttons() {
        let titles = registered_button_classes()
            .into_iter()
            .map(|registration| registration.title)
            .collect::<Vec<_>>();

        assert!(titles.contains(&"Terminal"));
        assert!(titles.contains(&"Cursor Info"));
        assert!(titles.contains(&"Cursor Latency Playground"));
        assert!(titles.contains(&"Text Rendering Playground"));
        assert!(titles.contains(&"Demo Mode"));
        assert!(titles.contains(&"Storage"));
        assert!(titles.contains(&"Environment Variables"));
        assert!(titles.contains(&"Application Windows"));
        assert!(titles.contains(&"Audio"));
        assert!(titles.contains(&"Audio Daemon"));
        assert!(titles.contains(&"Jobs"));
        assert!(titles.contains(&"Logs"));
        assert!(titles.contains(&"Audio Devices"));
        assert!(titles.contains(&"Timeline"));
        assert!(titles.contains(&"Timeline Playground"));
    }
}
