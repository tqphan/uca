// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Board, layer and item rectangles in monitor logical pixels
//! (02 "Board placement", "Layer layouts", "Layout details").

use uca_config::action::MonitorRef;
use uca_config::board::{Anchor, Board, BoardFile, BoardMonitor, Item, Layer, LayoutKind};
use uca_config::values::{Four, Length, Position, Size};
use uca_types::{MonitorId, MonitorInfo, RectLog};

#[derive(Debug, Clone, PartialEq)]
pub struct ItemLayout {
    /// Index of the item in its layer's `items`.
    pub index: usize,
    /// The item's id, or `item-<n>` when it has none (02).
    pub id: String,
    pub rect: RectLog,
    /// False for grid items that do not fit a grid with fixed `rows`.
    pub placed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayerLayout {
    pub id: String,
    pub rect: RectLog,
    pub items: Vec<ItemLayout>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardLayout {
    pub monitor: MonitorId,
    pub rect: RectLog,
    pub layers: Vec<LayerLayout>,
}

/// The monitor a board goes on; falls back to the primary (05).
pub fn choose_monitor<'m>(
    board: &Board,
    monitors: &'m [MonitorInfo],
    pointer_monitor: Option<MonitorId>,
) -> Option<&'m MonitorInfo> {
    let primary = || {
        monitors
            .iter()
            .find(|m| m.primary)
            .or_else(|| monitors.first())
    };
    let chosen = match &board.monitor {
        None | Some(BoardMonitor::Monitor(MonitorRef::Primary)) => primary(),
        Some(BoardMonitor::Monitor(MonitorRef::Index(i))) => {
            usize::try_from(*i).ok().and_then(|i| monitors.get(i))
        }
        Some(BoardMonitor::Monitor(MonitorRef::Name(n))) => monitors.iter().find(|m| m.name == *n),
        Some(BoardMonitor::FollowPointer) => {
            pointer_monitor.and_then(|id| monitors.iter().find(|m| m.id == id))
        }
    };
    chosen.or_else(primary)
}

/// Resolves a length against its parent size; `cell` is the grid cell size on that axis.
fn len(l: Length, parent: f64, cell: Option<f64>) -> f64 {
    match l {
        Length::Px(v) => v,
        Length::Percent(p) => parent * p / 100.0,
        Length::Grid(g) => cell.map_or(0.0, |c| c * g),
    }
}

/// Shrinks `r` by `padding` (top, right, bottom, left), lengths relative to `parent`.
fn inset(r: RectLog, padding: Option<Four<Length>>, parent: RectLog) -> RectLog {
    let [t, right, b, l] = match padding {
        None => return r,
        Some(Four::All(v)) => [v; 4],
        Some(Four::Each(e)) => e,
    };
    let (t, b) = (len(t, parent.height, None), len(b, parent.height, None));
    let (l, right) = (len(l, parent.width, None), len(right, parent.width, None));
    RectLog::new(
        r.x + l,
        r.y + t,
        (r.width - l - right).max(0.0),
        (r.height - t - b).max(0.0),
    )
}

fn size_of(
    size: Option<Size>,
    parent: RectLog,
    cell: Option<(f64, f64)>,
) -> (Option<f64>, Option<f64>) {
    let w = size
        .and_then(|s| s.width)
        .map(|w| len(w, parent.width, cell.map(|c| c.0)));
    let h = size
        .and_then(|s| s.height)
        .map(|h| len(h, parent.height, cell.map(|c| c.1)));
    (w, h)
}

fn pos_of(position: Option<Position>, parent: RectLog, cell: Option<(f64, f64)>) -> (f64, f64) {
    let x = position
        .and_then(|p| p.x)
        .map_or(0.0, |x| len(x, parent.width, cell.map(|c| c.0)));
    let y = position
        .and_then(|p| p.y)
        .map_or(0.0, |y| len(y, parent.height, cell.map(|c| c.1)));
    (parent.x + x, parent.y + y)
}

pub fn layout_board(file: &BoardFile, monitor: &MonitorInfo) -> BoardLayout {
    let b = &file.board;
    let scale = if monitor.scale_factor > 0.0 {
        monitor.scale_factor
    } else {
        1.0
    };
    let screen = RectLog::new(
        0.0,
        0.0,
        monitor.bounds_phys.width / scale,
        monitor.bounds_phys.height / scale,
    );

    let (w, h) = size_of(b.size, screen, None);
    let (w, h) = (w.unwrap_or(screen.width), h.unwrap_or(screen.height));
    let (x, y) = match b.anchor.unwrap_or(Anchor::Center) {
        Anchor::Top => ((screen.width - w) / 2.0, 0.0),
        Anchor::Bottom => ((screen.width - w) / 2.0, screen.height - h),
        Anchor::Left => (0.0, (screen.height - h) / 2.0),
        Anchor::Right => (screen.width - w, (screen.height - h) / 2.0),
        Anchor::Center => ((screen.width - w) / 2.0, (screen.height - h) / 2.0),
        Anchor::Free => pos_of(b.position, screen, None),
    };
    let rect = RectLog::new(x, y, w, h);
    let content = inset(rect, b.padding, screen);
    BoardLayout {
        monitor: monitor.id,
        rect,
        layers: file
            .layers
            .iter()
            .map(|l| layout_layer(l, content))
            .collect(),
    }
}

fn layout_layer(layer: &Layer, parent: RectLog) -> LayerLayout {
    let (x, y) = pos_of(layer.position, parent, None);
    let (w, h) = size_of(layer.size, parent, None);
    let rect = RectLog::new(x, y, w.unwrap_or(parent.width), h.unwrap_or(parent.height));
    let content = inset(rect, layer.padding, parent);
    let gap = |axis: f64| layer.layout.gap.map_or(0.0, |g| len(g, axis, None));
    let items = match layer.layout.kind {
        LayoutKind::Free => layer
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let (x, y) = pos_of(item.position, content, None);
                let (w, h) = size_of(item.size, content, None);
                placed(
                    i,
                    item,
                    RectLog::new(x, y, w.unwrap_or(0.0), h.unwrap_or(0.0)),
                )
            })
            .collect(),
        LayoutKind::Grid => grid(layer, content, gap(content.width), gap(content.height)),
        LayoutKind::Row => line(&layer.items, content, gap(content.width), true),
        LayoutKind::Column => line(&layer.items, content, gap(content.height), false),
    };
    LayerLayout {
        id: layer.id.0.clone(),
        rect,
        items,
    }
}

fn item_id(i: usize, item: &Item) -> String {
    item.id
        .as_ref()
        .map_or_else(|| format!("item-{i}"), |id| id.0.clone())
}

fn placed(i: usize, item: &Item, rect: RectLog) -> ItemLayout {
    ItemLayout {
        index: i,
        id: item_id(i, item),
        rect,
        placed: true,
    }
}

/// Grid placement (02): pinned items first, then the rest flow into free cells.
fn grid(layer: &Layer, content: RectLog, gap_x: f64, gap_y: f64) -> Vec<ItemLayout> {
    let n = layer.items.len();
    let columns = layer.layout.columns.map_or_else(
        || (n as f64).sqrt().ceil().max(1.0) as usize,
        |c| c.max(1) as usize,
    );
    let fixed_rows = layer.layout.rows.map(|r| r as usize);
    let span = |item: &Item| {
        item.span
            .map_or((1, 1), |s| (s.cols.max(1) as usize, s.rows.max(1) as usize))
    };

    let mut taken: Vec<Vec<bool>> = Vec::new();
    let fits = |taken: &Vec<Vec<bool>>, c: usize, r: usize, (sc, sr): (usize, usize)| {
        c + sc <= columns
            && fixed_rows.is_none_or(|rows| r + sr <= rows)
            && (r..r + sr).all(|rr| (c..c + sc).all(|cc| !taken.get(rr).is_some_and(|row| row[cc])))
    };
    let take = |taken: &mut Vec<Vec<bool>>, c: usize, r: usize, (sc, sr): (usize, usize)| {
        while taken.len() < r + sr {
            taken.push(vec![false; columns]);
        }
        for row in taken.iter_mut().skip(r).take(sr) {
            for cell in row.iter_mut().skip(c).take(sc) {
                *cell = true;
            }
        }
    };

    let mut cells: Vec<Option<(usize, usize)>> = vec![None; n];
    for (i, item) in layer.items.iter().enumerate() {
        if let Some(cell) = item.cell {
            let (c, r) = (cell.col as usize, cell.row as usize);
            if fits(&taken, c, r, span(item)) {
                take(&mut taken, c, r, span(item));
                cells[i] = Some((c, r));
            }
        }
    }
    let max_rows = fixed_rows.unwrap_or(usize::MAX);
    for (i, item) in layer.items.iter().enumerate() {
        if item.cell.is_some() {
            continue;
        }
        let s = span(item);
        let mut r = 0;
        'search: while r < max_rows && r <= taken.len() + s.1 {
            for c in 0..columns {
                if fits(&taken, c, r, s) {
                    take(&mut taken, c, r, s);
                    cells[i] = Some((c, r));
                    break 'search;
                }
            }
            r += 1;
        }
    }

    let rows = fixed_rows.unwrap_or(taken.len()).max(1);
    let cell_w = ((content.width - gap_x * (columns as f64 - 1.0)) / columns as f64).max(0.0);
    let cell_h = ((content.height - gap_y * (rows as f64 - 1.0)) / rows as f64).max(0.0);
    layer
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| match cells[i] {
            Some((c, r)) => {
                let (sc, sr) = span(item);
                let rect = RectLog::new(
                    content.x + c as f64 * (cell_w + gap_x),
                    content.y + r as f64 * (cell_h + gap_y),
                    sc as f64 * cell_w + (sc as f64 - 1.0) * gap_x,
                    sr as f64 * cell_h + (sr as f64 - 1.0) * gap_y,
                );
                placed(i, item, rect)
            }
            None => ItemLayout {
                index: i,
                id: item_id(i, item),
                rect: RectLog::default(),
                placed: false,
            },
        })
        .collect()
}

/// `row` (horizontal) or `column` layout (02).
fn line(items: &[Item], content: RectLog, gap: f64, horizontal: bool) -> Vec<ItemLayout> {
    let (main, cross) = if horizontal {
        (content.width, content.height)
    } else {
        (content.height, content.width)
    };
    let sizes: Vec<(Option<f64>, Option<f64>)> = items
        .iter()
        .map(|item| {
            let (w, h) = size_of(item.size, content, None);
            if horizontal {
                (w, h)
            } else {
                (h, w)
            }
        })
        .collect();
    let fixed: f64 = sizes.iter().filter_map(|s| s.0).sum();
    let flexible = sizes.iter().filter(|s| s.0.is_none()).count();
    let gaps = gap * (items.len().saturating_sub(1)) as f64;
    let share = if flexible > 0 {
        ((main - fixed - gaps) / flexible as f64).max(0.0)
    } else {
        0.0
    };
    let mut offset = 0.0;
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let (m, c) = sizes[i];
            let m = m.unwrap_or(share);
            let c = c.unwrap_or(cross);
            let cross_offset = (cross - c) / 2.0;
            let rect = if horizontal {
                RectLog::new(content.x + offset, content.y + cross_offset, m, c)
            } else {
                RectLog::new(content.x + cross_offset, content.y + offset, c, m)
            };
            offset += m + gap;
            placed(i, item, rect)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use uca_types::RectPhys;

    use super::*;

    fn monitor(scale: f64) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(0),
            name: "m".into(),
            primary: true,
            bounds_phys: RectPhys::new(0.0, 0.0, 1920.0 * scale, 1080.0 * scale),
            work_area_phys: RectPhys::new(0.0, 0.0, 1920.0 * scale, 1040.0 * scale),
            scale_factor: scale,
            refresh_hz: 60.0,
        }
    }

    fn board(yaml: &str) -> BoardFile {
        uca_config::parse_board(&format!("schema: uca/1\n{yaml}"), Path::new("b.yaml"))
            .expect("board")
    }

    fn rects(l: &BoardLayout, layer: usize) -> Vec<RectLog> {
        l.layers[layer].items.iter().map(|i| i.rect).collect()
    }

    #[test]
    fn board_anchor_and_size() {
        let b = board("board: { id: b, anchor: bottom, size: { width: 100%, height: 40% } }\n");
        let l = layout_board(&b, &monitor(1.0));
        assert_eq!(l.rect, RectLog::new(0.0, 648.0, 1920.0, 432.0));
        let b = board("board: { id: b, size: { width: 960, height: 540 } }\n");
        assert_eq!(
            layout_board(&b, &monitor(1.5)).rect,
            RectLog::new(480.0, 270.0, 960.0, 540.0),
            "center, logical px"
        );
        let b = board("board: { id: b, anchor: free, position: { x: 10, y: 50% }, size: { width: 100, height: 100 } }\n");
        assert_eq!(
            layout_board(&b, &monitor(1.0)).rect,
            RectLog::new(10.0, 540.0, 100.0, 100.0)
        );
        let b = board("board: { id: b }\n");
        assert_eq!(
            layout_board(&b, &monitor(2.0)).rect,
            RectLog::new(0.0, 0.0, 1920.0, 1080.0),
            "default fills"
        );
    }

    #[test]
    fn grid_with_padding_gap_span_and_pins() {
        let b = board(
            "board: { id: b, size: { width: 420, height: 220 }, anchor: free, padding: 10 }\n\
             layers:\n  - id: g\n    layout: { type: grid, columns: 4, gap: 10 }\n    items:\n\
             \x20     - { id: a }\n      - { id: pin, cell: { col: 1, row: 0 } }\n\
             \x20     - { id: wide, span: { cols: 2, rows: 1 } }\n      - empty\n      - { id: c }\n",
        );
        let l = layout_board(&b, &monitor(1.0));
        // content 400 × 200; 4 columns → cell 92.5; rows: a,pin,wide | empty,c → 2 rows → cell 95.
        assert_eq!(
            rects(&l, 0),
            vec![
                RectLog::new(10.0, 10.0, 92.5, 95.0),
                RectLog::new(112.5, 10.0, 92.5, 95.0),
                RectLog::new(215.0, 10.0, 195.0, 95.0),
                RectLog::new(10.0, 115.0, 92.5, 95.0),
                RectLog::new(112.5, 115.0, 92.5, 95.0),
            ]
        );
        assert_eq!(l.layers[0].items[3].id, "item-3");
    }

    #[test]
    fn grid_auto_columns_and_overflow() {
        let b = board(
            "board: { id: b, anchor: free, size: { width: 300, height: 300 } }\n\
             layers: [ { id: g, layout: { type: grid }, items: [ {id: a}, {id: b}, {id: c}, {id: d}, {id: e} ] } ]\n",
        );
        let l = layout_board(&b, &monitor(1.0));
        assert_eq!(
            l.layers[0].items[4].rect,
            RectLog::new(100.0, 150.0, 100.0, 150.0),
            "3 columns, 2 rows"
        );
        let b = board(
            "board: { id: b, anchor: free, size: { width: 200, height: 100 } }\n\
             layers: [ { id: g, layout: { type: grid, columns: 2, rows: 1 }, items: [ {id: a}, {id: b}, {id: c} ] } ]\n",
        );
        let l = layout_board(&b, &monitor(1.0));
        assert!(!l.layers[0].items[2].placed, "does not fit");
    }

    #[test]
    fn row_and_column() {
        let b = board(
            "board: { id: b, anchor: free, size: { width: 500, height: 100 } }\n\
             layers:\n  - { id: r, layout: { type: row, gap: 10 }, items: [ { id: a, size: { width: 100, height: 50 } }, { id: b }, { id: c } ] }\n\
             \x20 - { id: k, layout: { type: column }, items: [ { id: d }, { id: e, size: { width: 20% } } ] }\n",
        );
        let l = layout_board(&b, &monitor(1.0));
        assert_eq!(
            rects(&l, 0),
            vec![
                RectLog::new(0.0, 25.0, 100.0, 50.0),
                RectLog::new(110.0, 0.0, 190.0, 100.0),
                RectLog::new(310.0, 0.0, 190.0, 100.0),
            ]
        );
        assert_eq!(
            rects(&l, 1),
            vec![
                RectLog::new(0.0, 0.0, 500.0, 50.0),
                RectLog::new(200.0, 50.0, 100.0, 50.0)
            ]
        );
    }

    #[test]
    fn free_layer_and_grid_units() {
        let b = board(
            "board: { id: b, anchor: free, size: { width: 1000, height: 500 } }\n\
             layers: [ { id: f, layout: { type: free }, padding: [0, 0, 0, 100], items: [ { id: z, position: { x: 50%, y: 10 }, size: { width: 25%, height: 100 } } ] } ]\n",
        );
        let l = layout_board(&b, &monitor(1.0));
        assert_eq!(
            l.layers[0].items[0].rect,
            RectLog::new(550.0, 10.0, 225.0, 100.0)
        );
    }

    #[test]
    fn monitor_choice_falls_back_to_primary() {
        let mut second = monitor(1.0);
        second.id = MonitorId(1);
        second.primary = false;
        second.name = "side".into();
        let monitors = vec![second, monitor(1.0)];
        let pick = |yaml: &str| {
            let b = board(yaml);
            choose_monitor(&b.board, &monitors, Some(MonitorId(1))).map(|m| m.id)
        };
        assert_eq!(pick("board: { id: b }\n"), Some(MonitorId(0)));
        assert_eq!(
            pick("board: { id: b, monitor: side }\n"),
            Some(MonitorId(1))
        );
        assert_eq!(
            pick("board: { id: b, monitor: 7 }\n"),
            Some(MonitorId(0)),
            "gone → primary"
        );
        assert_eq!(
            pick("board: { id: b, monitor: follow-pointer }\n"),
            Some(MonitorId(1))
        );
    }
}
