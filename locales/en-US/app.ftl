# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: CC-BY-SA-4.0

# The app shell: tray menu and window titles (05).

app-name = UCA
tray-settings = Settings
tray-pause = Pause
tray-resume = Resume
tray-safe-board = Safe Board
tray-quit = Quit

# The Settings window (05 "Settings window").

settings-title = UCA Settings
settings-profile = Profile: { $name }
settings-start-board = Start board
settings-start-board-hint = Used the next time UCA starts.
settings-pause = Pause
settings-resume = Resume
settings-close = Close
settings-save-error = Could not save the start board: { $reason }
settings-diagnostics = Diagnostics
settings-debug-session = Debug session
settings-session-15 = 15 min
settings-session-60 = 60 min
settings-session-240 = 4 hours
settings-level-debug = Debug
settings-level-trace = Trace
settings-sensitive = Sensitive logging (records typed text)
settings-session-start = Start debug session
settings-session-stop = Stop debug session
settings-include-config = Include my boards and profile
settings-export = Export diagnostics
settings-export-saved = Saved to { $path }
settings-export-failed = Could not export: { $reason }

# The debug-session indicator strip (11).

debug-strip-on = Debug logging on · { $minutes } min left
debug-strip-sensitive = Sensitive logging is on — typed text is recorded
debug-strip-recording = Recording input for diagnostics

# The debug overlay's text panel (11).

debug-overlay-none = none
debug-overlay-board = Board: { $board }
debug-overlay-hovered = Hovered: { $item }
debug-overlay-dwell = Dwell: { $percent }% · { $ms } ms left
debug-overlay-grace = Grace: { $percent }% · { $ms } ms left
debug-overlay-dwell-idle = Dwell: idle
debug-overlay-source = { $source }: { $rate } Hz · confidence { $confidence } · presence { $presence }% · dropped { $dropped }
debug-overlay-fps = FPS: { $fps }
debug-overlay-latency = Latency: { $ms } ms
debug-overlay-latency-none = Latency: —

# README.txt inside a diagnostics bundle (11).

diag-readme-title = UCA diagnostics
diag-readme-intro = UCA, an AAC app that also gives computer access and game control, made this file so that someone helping you can look into a problem. Nothing was uploaded: you decide whom to send it to.
diag-readme-logs = logs/ — UCA's log files from the last 3 days. Typed text and typing keys are hidden unless sensitive logging was switched on in a debug session.
diag-readme-crashes = crashes/ — reports written when UCA crashed.
diag-readme-recordings = recordings/ — input recordings from debug sessions in the last 3 days.
diag-readme-system = system.json — UCA's version, the operating system, the monitors and what UCA can do on this computer.
diag-readme-config = config/ — your boards and profile, included because you chose to.
diag-readme-no-config = Your boards and profile are not included.

# Notices (11).

notice-crash = UCA closed unexpectedly last time.
notice-safe-profile = UCA closed unexpectedly twice soon after starting, so it started with the built-in safe profile. Your profile was not changed.
notice-export-saved = Diagnostics saved to { $path }
notice-export-failed = Could not export diagnostics: { $reason }
notice-export = Export diagnostics
notice-dismiss = Dismiss
notice-speech-off = Speech is turned off.
notice-speech-too-long = This message is too long to speak (limit: { $max } characters).
notice-speech-fallback = Speaker not found. Speaking through the default output instead.
notice-speech-voice-missing = Your chosen voice is not installed. Using the default voice.
notice-speech-failed = Speech is not working.
