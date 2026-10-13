# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: CC-BY-SA-4.0

# Plain-language descriptions of the user-adjustable settings (02).

setting-behavior-dwell-ms = How long to keep looking at or pointing at an item before it is selected. 0 turns dwell off.
setting-behavior-dwell-grace-ms = How long the pointer may leave an item while dwelling, before the dwell starts over.
setting-behavior-cooldown-ms = After an item is used, how long before it can be selected again.
setting-behavior-repeat-delay-ms = For items that repeat, how long to wait before the first repeat.
setting-behavior-repeat-interval-ms = For items that repeat, the time between repeats.
setting-behavior-confirm-window-ms = For items that ask for confirmation, how long you have to select them a second time.
setting-trigger-debounce-ms = Ignore a switch that changes state faster than this, so one press counts once.
setting-trigger-min-press-ms = Ignore presses shorter than this, for example from a tremor.
setting-trigger-acceptance-delay-ms = A switch must be held this long before the press counts.
setting-trigger-max-rate-hz = Ignore presses that come faster than this many per second. Off by default.
setting-filter-min-confidence = Ignore pointer samples the tracker is less sure of than this (0 to 1).
setting-filter-max-velocity-px-s = Ignore pointer jumps faster than this, unless the next samples agree.
setting-filter-outlier-agree-samples = How many samples in a row must agree before a fast jump is accepted.
setting-filter-one-euro-min-cutoff = Smoothing when the pointer is still. Lower is steadier but slower to follow.
setting-filter-one-euro-beta = How quickly smoothing lets go when the pointer moves fast. Higher follows quick moves better.
setting-filter-one-euro-d-cutoff = Smoothing of the speed estimate used by the filter. Rarely needs changing.
setting-filter-ema-alpha = Pointer smoothing. Lower is smoother but slower to follow; 1 means no smoothing.
setting-presence-resume-delay-ms = After you are detected again, how long to wait before dwell and scanning resume.
setting-behavior-enter-delay-ms = How long the pointer must be inside a game zone before it acts, so a passing glance does nothing.
setting-behavior-active-flash-ms = How long a selected item flashes, so you can see it was chosen. 0 turns the flash off.
setting-notice-timeout-ms = How long a message about an export stays on screen. 0 keeps it until you dismiss it.
