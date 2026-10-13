// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The built-in Safe Board (09): a 2 × 2 grid over the whole primary monitor.

use uca_config::board::{Board, BoardFile, Item, Layer, Layout, LayoutKind};
use uca_config::values::{Id, Length, Size};

use crate::types::SafeItem;

pub const SAFE_BOARD_ID: &str = "safe-board";
pub const SAFE_LAYER_ID: &str = "safe-items";

/// Cell order; Switch method keeps its cell while hidden until M3 (09).
pub const SAFE_ITEMS: [SafeItem; 4] = [
    SafeItem::Resume,
    SafeItem::Recalibrate,
    SafeItem::SwitchMethod,
    SafeItem::Restart,
];

pub fn safe_item_id(item: SafeItem) -> &'static str {
    match item {
        SafeItem::Resume => "resume",
        SafeItem::Recalibrate => "recalibrate",
        SafeItem::SwitchMethod => "switch-method",
        SafeItem::Restart => "restart",
    }
}

/// Shown in M1; Switch method arrives with the assessment (M3).
pub fn safe_item_shown(item: SafeItem) -> bool {
    item != SafeItem::SwitchMethod
}

fn id(s: &str) -> Id {
    Id(s.to_owned())
}

pub fn safe_board() -> BoardFile {
    let full = Size {
        width: Some(Length::Percent(100.0)),
        height: Some(Length::Percent(100.0)),
    };
    let items = SAFE_ITEMS
        .iter()
        .map(|&s| Item {
            id: Some(id(safe_item_id(s))),
            visible: Some(safe_item_shown(s)),
            ..Item::default()
        })
        .collect();
    BoardFile {
        schema: "uca/1".into(),
        board: Board {
            id: id(SAFE_BOARD_ID),
            size: Some(full),
            position: None,
            opacity: None,
            visible: None,
            background: None,
            border: None,
            padding: None,
            font: None,
            shadow: None,
            behavior: None,
            dwell_indicator: None,
            states: Default::default(),
            group: None,
            extends: None,
            title: None,
            monitor: None,
            anchor: None,
            reserve_space: None,
            click_through: None,
            area: None,
            theme: None,
            show_action_sets: None,
        },
        groups: Default::default(),
        templates: Default::default(),
        bindings: Vec::new(),
        layers: vec![Layer {
            id: id(SAFE_LAYER_ID),
            layout: Layout {
                kind: LayoutKind::Grid,
                columns: Some(2),
                rows: Some(2),
                gap: None,
            },
            bindings: Vec::new(),
            items,
            position: None,
            size: None,
            opacity: None,
            visible: None,
            background: None,
            border: None,
            padding: None,
            font: None,
            shadow: None,
            behavior: None,
            dwell_indicator: None,
            states: Default::default(),
            group: None,
            extends: None,
        }],
    }
}
