# Windowing

This specification covers the non-terminal window surfaces in Teamy Studio: the launcher, auxiliary picker windows, and shared diagnostics presentation across window types.

## Launcher

windowing[launcher.startup.default]
Launching Teamy Studio with no explicit terminal command must open a launcher window instead of immediately spawning a shell.

windowing[launcher.buttons.large-image-cards]
The launcher window must present large image-backed action cards for the primary entry points.

windowing[launcher.buttons.terminal]
The launcher window must expose a Terminal action that opens a terminal window using the selected VT engine.

windowing[launcher.buttons.storage-placeholder]
The launcher window must expose a Storage action that currently reports storage as unavailable rather than silently doing nothing.

windowing[launcher.buttons.audio-picker]
The launcher window must expose an Audio action that opens a dedicated audio-source picker window.

## Audio Picker

windowing[audio-picker.buttons.windows]
The audio-source picker must expose a Windows bell option.

windowing[audio-picker.buttons.file]
The audio-source picker must expose a Pick File option for choosing a custom bell file.

windowing[audio-picker.selection.persisted]
Selecting an audio source must persist the chosen bell source under the resolved application home directory.

windowing[audio-picker.selection.preview]
Selecting an audio source must immediately preview the bell sound.

## Diagnostics

windowing[diagnostics.toggle.shared-titlebar-button]
Launcher, picker, and terminal windows must expose a shared title-bar diagnostics toggle button in the top-right corner.

windowing[diagnostics.scene-window.replaces-body]
When diagnostics mode is enabled for a non-terminal scene window, the window body must switch from the visual card layout to a text representation of the scene.

windowing[diagnostics.terminal.bottom-panel-toggle]
When diagnostics mode is disabled for a terminal window, the bottom diagnostics panel must collapse and the terminal panel must expand into the freed space.

windowing[diagnostics.text.selection-and-copy]
Diagnostics text in terminal and non-terminal windows must support mouse selection and clipboard copy.

## Garden Frame

windowing[garden-band.shared]
Terminal, launcher, and auxiliary picker windows must reserve a shared decorative garden band around the content frame.

windowing[garden-band.outward]
The custom border treatment must render in the garden band outside the content frame instead of overlapping terminal or scene content.

windowing[garden-band.feathered]
The garden band's outer edge must feather away instead of ending on a hard opaque border.