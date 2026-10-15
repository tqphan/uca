// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The renderer's view of a board: resolved styles and labels per item, plus
//! the `active` flash (05). Rebuilt when the config or the shown board changes.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use uca_config::board::BoardFile;
use uca_config::cascade::{Cascade, ResolvedItem};
use uca_config::store::LoadedConfig;
use uca_config::values::State;
use uca_engine::{BoardSnapshot, ItemSnapshot, Latch};
use uca_types::Key;

use crate::paint::ItemLook;
use crate::surface::Surface;

pub struct BoardModel {
    pub board_id: String,
    resolved: Vec<Vec<ResolvedItem>>,
    board_surface: Surface,
    layer_surfaces: Vec<Surface>,
    /// When each item last became active, for the flash (05).
    active_since: BTreeMap<(usize, usize), Instant>,
    /// Items that were active at the last frame.
    was_active: BTreeMap<(usize, usize), bool>,
}

impl BoardModel {
    /// For a board of the config, or the built-in Safe Board (`config` unused then).
    pub fn new(config: &LoadedConfig, board_id: &str) -> Option<Self> {
        let file: BoardFile = if board_id == uca_engine::safe_board::SAFE_BOARD_ID {
            uca_engine::safe_board::safe_board()
        } else {
            config
                .boards
                .values()
                .find(|b| b.board.id.0 == board_id)?
                .clone()
        };
        let theme = file
            .board
            .theme
            .as_ref()
            .and_then(|t| config.themes.get(t))
            .map(|(_, t)| t);
        let cascade = Cascade::new(&file, theme);
        let b = &file.board;
        let board_surface = Surface::new(
            &cascade,
            b.background.as_ref(),
            b.border.as_ref(),
            b.shadow.as_ref(),
            b.opacity,
        );
        let layer_surfaces = file
            .layers
            .iter()
            .map(|l| {
                Surface::new(
                    &cascade,
                    l.background.as_ref(),
                    l.border.as_ref(),
                    l.shadow.as_ref(),
                    l.opacity,
                )
            })
            .collect();
        let resolved = file
            .layers
            .iter()
            .map(|l| l.items.iter().map(|i| cascade.item(l, i)).collect())
            .collect();
        Some(Self {
            board_id: board_id.to_owned(),
            resolved,
            board_surface,
            layer_surfaces,
            active_since: BTreeMap::new(),
            was_active: BTreeMap::new(),
        })
    }

    fn label(&self, item: &ItemSnapshot, res: &ResolvedItem, shift: bool) -> String {
        if let Some(safe) = item.safe_item {
            return uca_i18n::translate(safe.label_id(), &[]);
        }
        if res.item.prediction.is_some() {
            // A prediction slot shows its suggestion; an empty one no label (16).
            return item
                .slot_word
                .as_ref()
                .map(|w| w.0.clone())
                .unwrap_or_default();
        }
        let entry = item.action_set.as_ref().and_then(|set| {
            res.item
                .actions
                .as_ref()?
                .0
                .iter()
                .find(|(id, _)| id.0 == *set)
                .map(|(_, e)| e)
        });
        let label = entry
            .and_then(|e| e.label.clone())
            .or_else(|| res.item.label.clone());
        let shifted = entry
            .and_then(|e| e.label_shift.clone())
            .or_else(|| res.item.label_shift.clone());
        match (shift, shifted) {
            (true, Some(s)) => s,
            _ => label.unwrap_or_default(),
        }
    }

    /// Looks for the current frame; `now` drives the active flash.
    pub fn looks(
        &mut self,
        board: &BoardSnapshot,
        latched: &[(Key, Latch)],
        now: Instant,
    ) -> Vec<Vec<ItemLook<'_>>> {
        let shift = latched
            .iter()
            .any(|(k, _)| matches!(k, Key::ShiftLeft | Key::ShiftRight));
        // Track flash starts first (needs &mut self).
        let mut flashing = BTreeMap::new();
        for (li, layer) in board.layers.iter().enumerate() {
            for item in &layer.items {
                let key = (li, item.index);
                let active = item.states.contains(&State::Active);
                let was = self.was_active.insert(key, active).unwrap_or(false);
                if active && !was {
                    self.active_since.insert(key, now);
                }
                let flash_ms = self
                    .resolved
                    .get(li)
                    .and_then(|l| l.get(item.index))
                    .map_or(0.0, |r| r.behavior.active_flash_ms);
                let flash = self.active_since.get(&key).is_some_and(|since| {
                    now.saturating_duration_since(*since)
                        < Duration::from_secs_f64(flash_ms / 1000.0)
                });
                flashing.insert(key, active || flash);
            }
        }
        let this = &*self;
        board
            .layers
            .iter()
            .enumerate()
            .map(|(li, layer)| {
                layer
                    .items
                    .iter()
                    .filter_map(|item| {
                        let res = this.resolved.get(li)?.get(item.index)?;
                        Some(ItemLook {
                            resolved: res,
                            label: this.label(item, res, shift),
                            active: flashing.get(&(li, item.index)).copied().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .collect()
    }

    /// Paints the board: its surface, each visible layer's surface, then the items.
    pub fn paint(
        &mut self,
        painter: &egui::Painter,
        board: &BoardSnapshot,
        latched: &[(Key, Latch)],
        now: Instant,
    ) {
        let rect = |r: uca_types::RectLog| {
            egui::Rect::from_min_size(
                egui::pos2(r.x as f32, r.y as f32),
                egui::vec2(r.width as f32, r.height as f32),
            )
        };
        self.board_surface.paint(painter, rect(board.rect));
        for (layer, surface) in board.layers.iter().zip(&self.layer_surfaces) {
            if layer.visible {
                surface.paint(painter, rect(layer.rect));
            }
        }
        let looks = self.looks(board, latched, now);
        crate::paint::paint_board(painter, board, &looks);
    }

    /// Whether a flash is still running (the window keeps repainting, 05).
    pub fn animating(&self, now: Instant) -> bool {
        self.active_since.iter().any(|((li, ii), since)| {
            let ms = self
                .resolved
                .get(*li)
                .and_then(|l| l.get(*ii))
                .map_or(0.0, |r| r.behavior.active_flash_ms);
            now.saturating_duration_since(*since) < Duration::from_secs_f64(ms / 1000.0)
        })
    }
}
