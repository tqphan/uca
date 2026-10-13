// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tray icon image (05, decided): a rounded blue square with a white "U",
//! drawn in code until a designed logo exists.

pub const SIZE: u32 = 32;

const BLUE: [u8; 4] = [0x3b, 0x82, 0xf6, 0xff];
const WHITE: [u8; 4] = [0xff, 0xff, 0xff, 0xff];

/// RGBA pixels, SIZE × SIZE.
pub fn rgba() -> Vec<u8> {
    let n = SIZE as i32;
    let radius = 6;
    let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            // Rounded corners: outside the corner circles is transparent.
            let cx = if x < radius {
                radius
            } else if x >= n - radius {
                n - radius - 1
            } else {
                x
            };
            let cy = if y < radius {
                radius
            } else if y >= n - radius {
                n - radius - 1
            } else {
                y
            };
            let inside = (x - cx).pow(2) + (y - cy).pow(2) <= radius * radius;
            // The "U": two bars and a bottom bowl.
            let bar = (9..=12).contains(&x) || (19..=22).contains(&x);
            let in_u = (bar && (7..=20).contains(&y)) || {
                let (ux, uy) = (x as f32 - 15.5, y as f32 - 19.5);
                let d = (ux * ux + uy * uy).sqrt();
                y > 19 && (2.5..=6.5).contains(&d)
            };
            let px = if !inside {
                [0, 0, 0, 0]
            } else if in_u {
                WHITE
            } else {
                BLUE
            };
            out.extend_from_slice(&px);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_size_and_colours() {
        let px = rgba();
        assert_eq!(px.len(), (SIZE * SIZE * 4) as usize);
        let at =
            |x: u32, y: u32| &px[((y * SIZE + x) * 4) as usize..((y * SIZE + x) * 4 + 4) as usize];
        assert_eq!(at(0, 0)[3], 0, "rounded corner is transparent");
        assert_eq!(at(16, 2), BLUE);
        assert_eq!(at(10, 10), WHITE, "left bar of the U");
    }
}
