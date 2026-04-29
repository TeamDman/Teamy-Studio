I have a spritesheet for a gun firing and reloading.
I want to have a desktop toy behaviour where we can shoot windows and it tells them to close.
Weapons will have damage values, windows will take damage, and there will be thresholds where after enough damage is taken we issue sigkill and sigterm.

Bigger weapons like RPG will deal more damage, leading to a more aggressive termination behaviour.

we could add an orbital laser for instakill.

---

the transcription should use the rust implementation, not the python one.
we should add a "jobs" window to the main menu that lists the current asynchronous work going on. we will manually keep track however we wish that "we are in the middle of sending a transcription chunk through the ML pipeline" such that we always know what the system is up to. we can automatically open the window the first time a job is launched, letting it be closed by the user if they don't want it open. it occurs to me that we are currently using tracing and stuff for reporting information to the tracy-capture program when we do .\run-profiler.ps1 so we could have our own tracing-subscriber that tracks the spans and stuff and then we build our efforts from there. in fact, this is what our timeline is all about; giving a place to show information like tracing spans. we can load information files that  tracy-capture outputs and load them into tracy-profiler.exe but we are progressing towards also being able to open those files.

so, you are going to update our logic to use the rust ml pipeline, working seamlessly in the background to ensure the thread for the timeline window isn't held up.

we can add a new track type to our add-a-track menu; "tracing spans" which when added will show tracing information. this reminds me of a tracing add-on for egui.

G:\Programming\Repos\egui_tracing

We can look in there for some reference.
We should always capture our own log events by default, we can have the "tracing spans" track reference a global thing we will store our copy of our tracing subscriber stuff into so that two "tracing spans" tracks will display the same underlying data. That track should have its own settings menu where we have a target puck thing that shows "tracking spans" connected to "in-memory cache" by default, if the user wants to turn off the in memory cache the user can remove the puck.
the in-memory cache visual element in this dialog should show in its size how large it is, with a humansize text displaying somewhere, it should have a hover effect and tooltip and should react when the mouse gets near and stuff.
thus, the user can turn off the in-memory tracing logging for purposes of displaying on the timeline by removing the target. this will leave the in-memory cache with its latest contents, but won't flush it.
we can add a trashcan element to the gui with a target socket on the in-memory cache element that we can drag and drop on the trash can to empty the cache.

That way, we can fill up the cache by the default behaviour of it being connected, then we can "pipe" it into the trash to empty it, and we can have both happen where we are logging it instead of ignoring it but we are also piping it to the trash, this would make sense if we have an age limit where it will only dispose messages older than X seconds long and Y messages ago; both can be text inputs on the dialog.
We should be able to adjust these inputs by clicking and dragging like those inputs in davinci resolve; we can have a text field and a little square next to it where the text field has the text-input cursor and the square has the grab cursor and the grab cursor square is where starting a drag begins the manipulation.

in fact, this should be a knob element.
The idea of "dragology" from G:\Programming\Repos\draggable-diagrams gives us motivation for using numerical methods for knowing the position of the elements and our drag behaviour. the knob should be able to turn more than 360 degrees while still incrementing; it winds it doesn't reset like a clock.
we can implement these however, i've included the reading material for reference if you are stuck and don't have an implicit idea on how to do what I've asked.

the spans should show in our timeline, where the visuals is a nested timeline; the tracing-spans timeline row is actually its own entire timeline that the row becomes a minimap for; clicking the minimap lets us pan around and stuff and we can maximize the timeline by having floating action buttons in the bottom right of the preview of it that would encompass this window or pop-out in a new window.

everything with hover texts, animations over time, reactions to the mouse pointer.
