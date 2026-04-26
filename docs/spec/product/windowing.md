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

windowing[launcher.buttons.environment-variables-placeholder]
The launcher window must expose an Environment Variables action that currently reports the environment-variable inspector as unavailable rather than silently doing nothing.

windowing[launcher.buttons.application-windows-placeholder]
The launcher window must expose an Application Windows action that currently reports the application-window inspector as unavailable rather than silently doing nothing.

windowing[launcher.buttons.audio-picker]
The launcher window must expose an Audio action that opens a dedicated audio-source picker window.

windowing[launcher.buttons.cursor-gallery]
The launcher window must expose a Cursor Gallery action for inspecting the OS cursor sprites used by Teamy pointer rendering diagnostics.

windowing[launcher.buttons.demo-mode]
The launcher window must expose a Demo Mode action that opens demo privacy controls.

windowing[launcher.keyboard-navigation]
The launcher window must let keyboard users move between main-menu cards with arrow keys, Tab, or Shift+Tab and invoke the selected card with Enter or Space; arrow-key navigation must choose the next target from the active presentation's rendered shapes, including pretty card geometry and ratatui diagnostics rows, rather than assuming a perfect fixed grid, while Shift+Tab must move backward through sequential traversal.

## Virtual Cursor

windowing[virtual-cursor.os-cursor-sprite]
When launcher keyboard navigation has positioned the virtual cursor, Teamy Studio must draw an enlarged, tinted pointer at that virtual cursor position using an OS cursor texture from the renderer sprite atlas.

windowing[virtual-cursor.sdf-shader-roadmap]
The virtual cursor design must leave room for a future SDF-and-shader pipeline where stock OS cursor bitmaps can be converted into edge, distance-field, or curve data so shaders can render stylized cursor silhouettes from shape knowledge rather than only tinted sprites.

windowing[virtual-cursor.tooltips]
When keyboard navigation moves the virtual cursor over an element with hover text, Teamy Studio must show the same native tooltip style used for physical mouse hover.

windowing[cursor-gallery.stock-os-cursors]
The Cursor Gallery window must render a debugging gallery of stock OS cursor textures from the renderer sprite atlas.

windowing[cursor-gallery.virtual-navigation]
The Cursor Gallery window must expose each cursor cell as a navigable shape so arrow keys and Tab move the virtual cursor between cells.

windowing[cursor-gallery.hover-cursor-shape]
When a cursor gallery cell is hovered, the native mouse cursor and the virtual pointer rendering must use the cursor shape represented by that cell.

windowing[cursor-gallery.hover-glow-color]
The cursor gallery must glow the hovered or virtually selected cell using that cell's gallery color rather than the OS cursor color or the virtual pointer's default tint.

## Shared Chrome

windowing[chrome.pin-button]
Launcher, picker, and terminal windows must expose a left-edge title-bar pin button with hover/press animation, distinct active and inactive visual states, and a pinned state that keeps the window always on top until unpinned.

## Demo Mode

windowing[demo-mode.window]
The Demo Mode window must render a Demo Mode button and a shader-animated toggle labelled "scramble input device identifiers" with clear on/off colors and current-state text.

windowing[demo-mode.input-device-identifier-scramble]
Demo Mode must model fake input device identifiers with an Arbitrary-backed newtype so demos can retain realistic identifier shape while censoring microphone or input-device IDs, must avoid adding prefixes that the obscured values do not have, and the scramble toggle must expose hover text explaining that behavior.

windowing[demo-mode.persist-scramble-toggle]
Demo Mode must persist the scramble-input-device-identifiers toggle under the application home directory in a simple text file.

windowing[demo-mode.live-audio-device-scramble]
When the Demo Mode scramble toggle changes, open audio-device picker and selected-microphone windows must update their displayed endpoint identifiers without reopening the audio windows or changing the real endpoint identity used for capture.

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

windowing[diagnostics.launcher-tui]
When diagnostics mode is enabled for the launcher window, the window body must render as a ratatui-style main-menu diagnostics application that shows the selected card, available actions, and keyboard controls.

windowing[diagnostics.terminal.bottom-panel-toggle]
When diagnostics mode is disabled for a terminal window, the bottom diagnostics panel must collapse and the terminal panel must expand into the freed space.

windowing[diagnostics.text.selection-and-copy]
Diagnostics text in terminal and non-terminal windows must support mouse selection and clipboard copy.

windowing[scene.pretty-text.selection]
Non-clickable pretty-mode text in scene windows, including selected microphone details such as name, sample rate, state, and endpoint identifier, must support mouse selection and clipboard copy with the same text-selection model used by diagnostics.

## Garden Frame

windowing[garden-band.shared]
Terminal, launcher, and auxiliary picker windows must reserve a shared decorative garden band around the content frame.

windowing[garden-band.outward]
The custom border treatment must render in the garden band outside the content frame instead of overlapping terminal or scene content.

windowing[garden-band.feathered]
The garden band's outer edge must feather away instead of ending on a hard opaque border.