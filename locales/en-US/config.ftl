# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: CC-BY-SA-4.0

# Config problems shown to the user (02 "Validation and loading").

## Binding patterns (04 "Pattern DSL")

config-pattern-empty = The pattern is empty.
config-pattern-expected = Column { $column }: expected { $expected }, found { $found }.
config-pattern-invalid-alias = Column { $column }: “{ $text }” is not a switch name or a trigger id.
config-pattern-invalid-trigger = Column { $column }: “{ $text }” is not a valid trigger: { $reason }
config-pattern-empty-gesture = Column { $column }: “gesture:” needs a gesture name.
config-pattern-not-a-number = Column { $column }: “{ $text }” is not a number of milliseconds.
config-pattern-ms-out-of-range = Column { $column }: { $value } ms is outside the allowed range { $min }–{ $max } ms.

## Semantic problems (02 "Validation rules")

config-missing-escape = The profile has no safety escape. Add one under safety: escape. It is required so the user can always reach the Safe Board.
config-invalid-pattern = The pattern “{ $pattern }” is not valid: { $error }
config-escape-clash = The binding “{ $pattern }” means the same as the safety escape. The escape cannot be overridden.
config-unknown-switch = There is no switch called “{ $name }”. Add it under profile inputs, switches.
config-action-and-actions = Use either action or actions on an item, not both.
config-do-and-stages = Use either do or stages (start, repeat, stop) on a binding, not both.
config-unknown-action-set = There is no action set called “{ $id }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-reserved-set-name = “{ $id }” cannot be used as an action set name. It is reserved.
config-hold-outside-start = action_set.hold can only be used in a start stage.
config-off-default = The default action set cannot be turned off.
config-duplicate-id = The id “{ $id }” is used more than once.
config-unknown-template = There is no template called “{ $name }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-template-cycle = These templates extend each other in a loop: { $chain }
config-unknown-macro = There is no macro called “{ $id }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-macro-cycle = These macros run each other in a loop: { $chain }
config-unknown-color-var = There is no colour variable called ${ $name }. Define it in the theme's vars.
config-unknown-board = There is no board called “{ $id }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-unknown-theme = There is no theme called “{ $name }”.
config-unknown-layer = No board has a layer called “{ $id }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-pointer-source-not-enabled = The pointer source “{ $source }” must be listed under sources and enabled.
config-filter-param-wrong-kind = { $param } does not apply to the { $kind } filter.
config-grid-only = { $key } can only be used in a grid layer.
config-zone-outside-free = Zones can only be used in a free layer.
config-outside-grid = { $key } lies outside the grid.
config-cell-overlap = This item overlaps “{ $other }”, which is pinned to the same cells.
config-unused-action-set = The action set “{ $id }” is never used.
config-repeat-never-runs = This repeat never runs, because the binding's pattern finishes at once.
config-set-without-exit = The action set “{ $id }” can be turned on, but nothing turns it off while it is active.
config-no-way-out = The board “{ $board }” has no item or binding that leads to another board.
config-unknown-group = There is no group called “{ $name }”. The item is treated as “others”.
config-unknown-font = The font “{ $family }” is not declared in the theme. The default font is used.
config-free-item-without-size = Items in a free layer need a size with both width and height.
config-position-ignored = The board's position is only used when its anchor is free.
config-area-ignored = The area setting only applies to a board that reserves screen space, so the full screen is used.
config-reserve-without-edge = Only a board docked to an edge (top, bottom, left or right) can reserve screen space, so none is reserved.
config-grid-unit-outside-grid = The grid unit can only be used inside a grid layer.
config-grid-overflow = Some items in the layer “{ $layer }” do not fit its rows and are not shown.
config-single-also-fires = “{ $pattern }” is a double press, but the same input is also bound on its own, which fires on the first press of every double.

## Messages from reading a file (02 "Message text")

config-parse-unknown-field = “{ $name }” is not a known setting here. Known settings: { $expected }.
config-parse-missing-field = “{ $name }” is required here.
config-parse-duplicate-field = “{ $name }” is written twice.
config-parse-invalid-type = Expected { $expected }, but found { $found }.
config-parse-invalid-value = Expected { $expected }, but found { $found }.
config-parse-unknown-variant = “{ $name }” is not one of the allowed values: { $expected }.
config-parse-unknown-action = There is no action called “{ $name }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-parse-unknown-key = There is no key called “{ $name }”.{ $suggestion ->
        [none] {""}
       *[other] { " " }Did you mean “{ $suggestion }”?
    }
config-parse-out-of-range = { $what }: { $value } is outside the allowed range { $min }–{ $max }.
config-parse-other = The file could not be read: { $message }

# Targeted saves (02): the reason after “Could not save the start board:”.

config-save-not-editable = start_board in profile.yaml is written in a form that cannot be changed safely. Change it in the file.
config-save-write-failed = The file could not be written: { $message }
