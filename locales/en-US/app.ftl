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
