use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn run_teamy_studio(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teamy-studio"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    command
        .output()
        .expect("teamy-studio command should launch")
}

fn output_text(output: &Output) -> String {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{unique}"));
        std::fs::create_dir_all(&path).expect("temporary directory should be created");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// tool[verify cli.version.includes-semver]
// tool[verify cli.version.includes-git-revision]
#[test]
fn test_version_includes_semver_and_git_revision() {
    let output = run_teamy_studio(&["--version"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "version command failed:\n{text}");
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "missing semver:\n{text}"
    );
    assert!(
        text.contains("(rev "),
        "missing git revision marker:\n{text}"
    );
}

// tool[verify cli.help.describes-behavior]
// tool[verify cli.help.describes-terminal]
// tool[verify cli.help.describes-self-test]
// tool[verify cli.help.describes-argv]
// tool[verify cli.help.describes-environment]
// tool[verify cli.global.debug]
// tool[verify cli.global.log-filter]
// tool[verify cli.global.log-file]
// tool[verify cli.global.output-format]
// tool[verify cli.surface.cursor-info]
// tool[verify cli.surface.terminal]
// tool[verify cli.surface.self-test]
// audio[verify cli.audio-command]
// image[verify cli.image-command]
#[test]
fn test_root_help_describes_commands_args_and_environment() {
    let output = run_teamy_studio(&["--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "help command failed:\n{text}");
    assert!(
        text.contains("cursor-info"),
        "missing cursor-info command in help:\n{text}"
    );
    assert!(
        text.contains("terminal"),
        "missing terminal command in help:\n{text}"
    );
    assert!(
        text.contains("self-test"),
        "missing self-test command in help:\n{text}"
    );
    assert!(
        text.contains("audio"),
        "missing audio command in help:\n{text}"
    );
    assert!(
        text.contains("image"),
        "missing image command in help:\n{text}"
    );
    assert!(
        !text.contains("\n    workspace\n"),
        "workspace command should not appear in help:\n{text}"
    );
    assert!(
        !text.contains("\n    window\n"),
        "window command should not appear in help:\n{text}"
    );
    assert!(text.contains("--debug"), "missing --debug in help:\n{text}");
    assert!(
        text.contains("--log-filter"),
        "missing --log-filter in help:\n{text}"
    );
    assert!(
        text.contains("--log-file"),
        "missing --log-file in help:\n{text}"
    );
    assert!(
        text.contains("--output-format"),
        "missing --output-format in help:\n{text}"
    );
    assert!(
        text.contains("TEAMY_STUDIO_HOME_DIR"),
        "missing TEAMY_STUDIO_HOME_DIR in help:\n{text}"
    );
    assert!(
        text.contains("TEAMY_STUDIO_CACHE_DIR"),
        "missing TEAMY_STUDIO_CACHE_DIR in help:\n{text}"
    );
    assert!(
        text.contains("RUST_LOG"),
        "missing RUST_LOG in help:\n{text}"
    );
}

// image[verify cli.image-command]
// image[verify cli.upscale-command]
// image[verify cli.model-command]
// image[verify cli.model-list]
// image[verify cli.model-prepare]
// image[verify cli.model-show]
// image[verify cli.upscale-defaults]
// image[verify cli.tta]
// image[verify cli.disable-tta]
#[test]
fn test_image_help_is_available() {
    let output = run_teamy_studio(&["image", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "image help failed:\n{text}");
    assert!(
        text.contains("upscale"),
        "missing upscale subcommand in help:\n{text}"
    );
    assert!(
        text.contains("model"),
        "missing model subcommand in help:\n{text}"
    );

    let output = run_teamy_studio(&["image", "upscale", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "image upscale help failed:\n{text}"
    );
    assert!(text.contains("--style"), "missing --style:\n{text}");
    assert!(text.contains("--scale"), "missing --scale:\n{text}");
    assert!(text.contains("--tile-size"), "missing --tile-size:\n{text}");
    assert!(
        text.contains("--batch-size"),
        "missing --batch-size:\n{text}"
    );
    assert!(text.contains("--device"), "missing --device:\n{text}");
    assert!(
        text.contains("--output-format"),
        "missing --output-format:\n{text}"
    );
    assert!(text.contains("--tta"), "missing --tta:\n{text}");
    assert!(
        text.contains("--disable-tta"),
        "missing --disable-tta:\n{text}"
    );

    let output = run_teamy_studio(&["image", "model", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "image model help failed:\n{text}");
    assert!(text.contains("list"), "missing list subcommand:\n{text}");
    assert!(
        text.contains("prepare"),
        "missing prepare subcommand:\n{text}"
    );
    assert!(text.contains("show"), "missing show subcommand:\n{text}");
}

// image[verify cli.output-format-conflict]
#[test]
fn test_image_upscale_bails_when_explicit_output_format_conflicts_with_output_path() {
    let output = run_teamy_studio(
        &[
            "image",
            "upscale",
            "input.png",
            "output.jpg",
            "--output-format",
            "png",
        ],
        &[],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "conflicting output format unexpectedly succeeded:\n{text}"
    );
    assert!(
        text.contains("conflicts with output path extension"),
        "missing conflict explanation:\n{text}"
    );
    assert!(
        !text.contains("not implemented yet"),
        "conflict should be reported before inference/model scaffolding:\n{text}"
    );
}

// image[verify cli.model-list]
// image[verify cli.model-show]
// image[verify cli.model-prepare]
// image[verify model.cache-layout]
#[test]
fn test_image_model_commands_report_and_prepare_managed_metadata() {
    let cache_dir = TempDirGuard::new("teamy-image-model-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &["--output-format", "json", "image", "model", "list"],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(output.status.success(), "image model list failed:\n{text}");
    assert!(
        text.contains("waifu2x-art-2x"),
        "missing default model:\n{text}"
    );
    assert!(
        text.contains("waifu2x-art-denoise-3-4x"),
        "list should include denoise-aware inventory variants:\n{text}"
    );
    assert!(
        text.contains("inventory-only"),
        "list should surface inventory-only runtime status for unsupported variants:\n{text}"
    );
    assert!(
        text.contains("models\\\\image") || text.contains("models/image"),
        "missing managed image model root:\n{text}"
    );
    assert!(
        text.contains("Missing"),
        "model should start missing:\n{text}"
    );

    let output = run_teamy_studio(
        &["--output-format", "json", "image", "model", "prepare"],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "image model prepare failed:\n{text}"
    );
    assert!(
        text.contains("waifu2x.swin_unet_2x"),
        "missing architecture metadata:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-art-2x")
            .join("model-metadata.json")
            .is_file(),
        "prepare should write managed image model metadata"
    );

    let output = run_teamy_studio(
        &["--output-format", "json", "image", "model", "show"],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(output.status.success(), "image model show failed:\n{text}");
    assert!(
        text.contains("\"state\": \"Prepared\""),
        "show should report the prepared managed Burnpack state:\n{text}"
    );
    assert!(
        text.contains("20250502"),
        "show should include source archive version:\n{text}"
    );
}

// image[verify cli.model-prepare]
#[test]
fn test_image_model_prepare_rejects_inventory_only_variants() {
    let cache_dir = TempDirGuard::new("teamy-image-model-inventory-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &["image", "model", "prepare", "waifu2x-art-denoise-0"],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "inventory-only prepare should fail clearly:\n{text}"
    );
    assert!(
        text.contains("inventory-only"),
        "prepare failure should explain that the variant is inventory-only:\n{text}"
    );
    assert!(
        text.contains("denoise-only Burn prep/runtime"),
        "prepare failure should explain why the variant is not runnable yet:\n{text}"
    );
}

// image[verify cli.auto-prepare-default-model]
// image[verify model.cache-layout]
#[test]
fn test_image_upscale_auto_prepares_default_model_metadata_before_inference() {
    let cache_dir = TempDirGuard::new("teamy-image-upscale-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();
    let missing_input_path = cache_dir.path().join("input.png");
    let missing_input_path_text = missing_input_path.to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &["image", "upscale", &missing_input_path_text],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "upscale should fail because the test input path does not exist:\n{text}"
    );
    assert!(
        text.contains("failed to open input image"),
        "upscale should auto-prepare the model before failing on the missing input path:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-art-denoise-0-2x")
            .join("model-metadata.json")
            .is_file(),
        "upscale should auto-prepare default image model metadata before inference"
    );
}

// image[verify cli.upscale-command]
#[test]
fn test_image_upscale_noise_level_routes_to_supported_denoise_model() {
    let cache_dir = TempDirGuard::new("teamy-image-upscale-noise-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();
    let missing_input_path = cache_dir.path().join("input.png");
    let missing_input_path_text = missing_input_path.to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &[
            "image",
            "upscale",
            &missing_input_path_text,
            "--noise-level",
            "3",
        ],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "denoise-aware upscale should still fail on the missing test input path:\n{text}"
    );
    assert!(
        text.contains("waifu2x-art-denoise-3-2x"),
        "upscale should resolve the denoise-aware managed model name during prepare/logging:\n{text}"
    );
    assert!(
        text.contains("failed to open input image"),
        "upscale should get past model preparation and fail on the missing input path:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-art-denoise-3-2x")
            .join("model-metadata.json")
            .is_file(),
        "upscale should auto-prepare the denoise-aware 2x image model before inference"
    );
}

// image[verify cli.fast-preset]
// image[verify cli.preset-seeds-optional-defaults]
#[test]
fn test_image_upscale_fast_preset_routes_to_scale_only_art_model() {
    let cache_dir = TempDirGuard::new("teamy-image-upscale-fast-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();
    let missing_input_path = cache_dir.path().join("input.png");
    let missing_input_path_text = missing_input_path.to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &[
            "image",
            "upscale",
            &missing_input_path_text,
            "--preset",
            "fast",
        ],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "fast-preset upscale should still fail on the missing test input path:\n{text}"
    );
    assert!(
        text.contains("waifu2x-art-2x"),
        "fast preset should resolve the scale-only managed art model during prepare/logging:\n{text}"
    );
    assert!(
        text.contains("failed to open input image"),
        "fast-preset upscale should get past model preparation and fail on the missing input path:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-art-2x")
            .join("model-metadata.json")
            .is_file(),
        "fast preset should auto-prepare the scale-only image model before inference"
    );
}

// image[verify cli.model-selection]
// image[verify cli.photo-derived-2x]
#[test]
fn test_image_upscale_photo_style_routes_to_supported_derived_2x_model() {
    let cache_dir = TempDirGuard::new("teamy-image-upscale-photo-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();
    let missing_input_path = cache_dir.path().join("input.png");
    let missing_input_path_text = missing_input_path.to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &[
            "image",
            "upscale",
            &missing_input_path_text,
            "--style",
            "photo",
        ],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "photo-style upscale should still fail on the missing test input path:\n{text}"
    );
    assert!(
        text.contains("waifu2x-photo-2x"),
        "upscale should resolve the photo managed model name during prepare/logging:\n{text}"
    );
    assert!(
        text.contains("failed to open input image"),
        "photo-style upscale should get past model preparation and fail on the missing input path:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-photo-2x")
            .join("model-metadata.json")
            .is_file(),
        "photo-style upscale should auto-prepare the derived 2x image model before inference"
    );
}

// image[verify cli.model-selection]
// image[verify cli.art-native-4x]
#[test]
fn test_image_upscale_scale_4_routes_to_native_art_4x_model() {
    let cache_dir = TempDirGuard::new("teamy-image-upscale-art-4x-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();
    let missing_input_path = cache_dir.path().join("input.png");
    let missing_input_path_text = missing_input_path.to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &["image", "upscale", &missing_input_path_text, "--scale", "4"],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "scale-4 art upscale should still fail on the missing test input path:\n{text}"
    );
    assert!(
        text.contains("waifu2x-art-denoise-0-4x"),
        "upscale should resolve the native art 4x managed model name during prepare/logging:\n{text}"
    );
    assert!(
        text.contains("failed to open input image"),
        "scale-4 art upscale should get past model preparation and fail on the missing input path:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-art-denoise-0-4x")
            .join("model-metadata.json")
            .is_file(),
        "scale-4 art upscale should auto-prepare the native 4x image model before inference"
    );
}

// image[verify cli.model-selection]
// image[verify cli.photo-derived-2x]
#[test]
fn test_image_upscale_scan_style_routes_to_supported_derived_2x_model() {
    let cache_dir = TempDirGuard::new("teamy-image-upscale-scan-cache");
    let cache_dir_text = cache_dir.path().to_string_lossy().into_owned();
    let missing_input_path = cache_dir.path().join("input.png");
    let missing_input_path_text = missing_input_path.to_string_lossy().into_owned();

    let output = run_teamy_studio(
        &[
            "image",
            "upscale",
            &missing_input_path_text,
            "--style",
            "scan",
        ],
        &[("TEAMY_STUDIO_CACHE_DIR", &cache_dir_text)],
    );
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "scan-style upscale should still fail on the missing test input path:\n{text}"
    );
    assert!(
        text.contains("waifu2x-art-scan-2x"),
        "upscale should resolve the art_scan managed model name during prepare/logging:\n{text}"
    );
    assert!(
        text.contains("failed to open input image"),
        "scan-style upscale should get past model preparation and fail on the missing input path:\n{text}"
    );
    assert!(
        cache_dir
            .path()
            .join("models")
            .join("image")
            .join("waifu2x-art-scan-2x")
            .join("model-metadata.json")
            .is_file(),
        "scan-style upscale should auto-prepare the derived art_scan 2x image model before inference"
    );
}

// audio[verify cli.audio-command]
// audio[verify cli.input-device-command]
// audio[verify cli.input-device-list]
#[test]
fn test_audio_input_device_help_is_available() {
    let output = run_teamy_studio(&["audio", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "audio help failed:\n{text}");
    assert!(
        text.contains("input-device"),
        "missing input-device subcommand in help:\n{text}"
    );

    let output = run_teamy_studio(&["audio", "input-device", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "input-device help failed:\n{text}");
    assert!(
        text.contains("list"),
        "missing list subcommand in help:\n{text}"
    );

    let output = run_teamy_studio(&["audio", "input-device", "list", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "input-device list help failed:\n{text}"
    );
}

// tool[verify cli.help.position-independent]
// cli[verify command.surface.terminal]
// cli[verify command.surface.terminal-default-shell]
// cli[verify command.surface.terminal-list]
// cli[verify command.surface.terminal-open]
#[test]
fn test_terminal_help_is_available() {
    let output = run_teamy_studio(&["terminal", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "terminal help failed:\n{text}");
    assert!(
        text.contains("default-shell"),
        "missing default-shell subcommand in help:\n{text}"
    );
    assert!(
        text.contains("list"),
        "missing list subcommand in help:\n{text}"
    );
    assert!(
        text.contains("open"),
        "missing open subcommand in help:\n{text}"
    );
    assert!(
        !text.contains("attach"),
        "attach subcommand should not appear in help:\n{text}"
    );
    assert!(
        !text.contains("create"),
        "create subcommand should not appear in help:\n{text}"
    );
    assert!(
        !text.contains("show-window"),
        "show-window subcommand should not appear in help:\n{text}"
    );
}

// cli[verify terminal.open.default-shell-when-program-omitted]
// cli[verify terminal.open.double-dash-trailing-args]
// cli[verify terminal.open.stdin-flag]
// cli[verify terminal.open.title-flag]
#[test]
fn test_terminal_open_help_is_available() {
    let output = run_teamy_studio(&["terminal", "open", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "terminal open help failed:\n{text}"
    );
    assert!(
        text.contains("Optional program to launch"),
        "missing optional program description:\n{text}"
    );
    assert!(text.contains("--stdin"), "missing --stdin in help:\n{text}");
    assert!(text.contains("--title"), "missing --title in help:\n{text}");
}

// cli[verify terminal.list.enumerates-live-windows]
// cli[verify terminal.list.prints-hwnd-pid-and-title]
#[test]
fn test_terminal_list_help_and_command_succeed() {
    let help_output = run_teamy_studio(&["terminal", "list", "--help"], &[]);
    let help_text = output_text(&help_output);
    assert!(
        help_output.status.success(),
        "terminal list help failed:\n{help_text}"
    );

    let output = run_teamy_studio(&["terminal", "list"], &[]);
    let text = output_text(&output);
    assert!(output.status.success(), "terminal list failed:\n{text}");
}

// cli[verify command.surface.terminal-default-shell]
// cli[verify command.surface.terminal-default-shell-set]
// cli[verify command.surface.terminal-default-shell-show]
#[test]
fn test_terminal_default_shell_help_is_available() {
    let output = run_teamy_studio(&["terminal", "default-shell", "--help"], &[]);
    let text = output_text(&output);

    assert!(
        output.status.success(),
        "terminal default-shell help failed:\n{text}"
    );
    assert!(
        text.contains("set"),
        "missing set subcommand in help:\n{text}"
    );
    assert!(
        text.contains("show"),
        "missing show subcommand in help:\n{text}"
    );
}

// cli[verify shell.default.set.double-dash-trailing-args]
// cli[verify shell.default.show-effective]
// cli[verify shell.default.persisted-in-app-home]
#[test]
fn test_terminal_default_shell_set_and_show_roundtrip() {
    let app_home = TempDirGuard::new("teamy-studio-cli-app-home");
    let app_home_value = app_home.path().to_string_lossy().into_owned();
    let envs = [("TEAMY_STUDIO_HOME_DIR", app_home_value.as_str())];

    let set_output = run_teamy_studio(
        &[
            "terminal",
            "default-shell",
            "set",
            "pwsh",
            "--",
            "-NoProfile",
        ],
        &envs,
    );
    let set_text = output_text(&set_output);
    assert!(
        set_output.status.success(),
        "terminal default-shell set failed:\n{set_text}"
    );

    let show_output = run_teamy_studio(&["terminal", "default-shell", "show"], &envs);
    let show_text = output_text(&show_output);
    assert!(
        show_output.status.success(),
        "terminal default-shell show failed:\n{show_text}"
    );
    assert!(
        show_text.contains("pwsh"),
        "missing program in show output:\n{show_text}"
    );
    assert!(
        show_text.contains("-NoProfile"),
        "missing trailing argument in show output:\n{show_text}"
    );
}

#[test]
fn test_shell_surface_is_removed() {
    let output = run_teamy_studio(&["shell"], &[]);
    let text = output_text(&output);

    assert!(
        !output.status.success(),
        "shell invocation unexpectedly succeeded:\n{text}"
    );
    assert!(
        text.contains("unexpected argument: shell"),
        "removed shell command should be rejected explicitly:\n{text}"
    );
}

// cli[verify command.surface.self-test]
// cli[verify command.surface.self-test-keyboard-input]
// cli[verify command.surface.self-test-terminal-throughput]
// cli[verify command.surface.self-test-terminal-replay]
// cli[verify command.surface.self-test-render-offscreen]
// image[verify self-test.reference-command]
#[test]
fn test_self_test_help_is_available() {
    let output = run_teamy_studio(&["self-test", "--help"], &[]);
    let text = output_text(&output);

    assert!(output.status.success(), "self-test help failed:\n{text}");
    assert!(
        text.contains("keyboard-input"),
        "missing keyboard-input subcommand in help:\n{text}"
    );
    assert!(
        text.contains("terminal-throughput"),
        "missing terminal-throughput subcommand in help:\n{text}"
    );
    assert!(
        text.contains("terminal-replay"),
        "missing terminal-replay subcommand in help:\n{text}"
    );
    assert!(
        text.contains("render-offscreen"),
        "missing render-offscreen subcommand in help:\n{text}"
    );
    assert!(
        text.contains("image-upscale-reference"),
        "missing image-upscale-reference subcommand in help:\n{text}"
    );
}
