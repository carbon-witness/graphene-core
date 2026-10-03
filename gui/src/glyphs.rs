//! The player-style glyphs of the tray icon and menu: a white circle with a coloured play, pause, restart
//! or cross, drawn at runtime so there are no image files to keep in step with the code.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glyph {
    /// Green: the node runs
    Play,
    /// Yellow: the node is stopped
    Pause,
    /// Blue-cyan: the restart action (button and tray menu)
    Restart,
    /// A plain yellow disc, no white: starting, syncing or stopping (the tray icon)
    Busy,
    /// Red: crashed, stuck or someone else's node
    Cross,
}

impl Glyph {
    /// The glyph named by Status::glyph
    pub fn from_name(name: &str) -> Glyph {
        match name {
            "play" => Glyph::Play,
            "busy" => Glyph::Busy,
            "restart" => Glyph::Restart,
            "cross" => Glyph::Cross,
            _ => Glyph::Pause,
        }
    }

    pub fn for_status_color(color: &str) -> Glyph {
        match color {
            "green" => Glyph::Play,
            "yellow" => Glyph::Busy,
            "red" => Glyph::Cross,
            _ => Glyph::Pause,
        }
    }

    fn rgb(self) -> [f32; 3] {
        match self {
            Glyph::Play => [0x2e as f32, 0xa0 as f32, 0x43 as f32],
            Glyph::Pause | Glyph::Busy => [0xe0 as f32, 0xa1 as f32, 0x00 as f32],
            Glyph::Restart => [0x1c as f32, 0x9b as f32, 0xd6 as f32],
            Glyph::Cross => [0xd1 as f32, 0x24 as f32, 0x2f as f32],
        }
    }

    /// Whether the point (u, v) in [-1, 1]², v pointing down, is on the glyph.
    fn covers(self, u: f32, v: f32) -> bool {
        // The shapes below are drawn at 1/GLYPH_SCALE of their final size; the scale keeps them inside the rim
        let (u, v) = (u / GLYPH_SCALE, v / GLYPH_SCALE);
        match self {
            Glyph::Busy => true,
            Glyph::Play => in_triangle((u, v), (-0.26, -0.42), (-0.26, 0.42), (0.46, 0.0)),
            Glyph::Pause => v.abs() <= 0.38 && ((-0.32..=-0.09).contains(&u) || (0.09..=0.32).contains(&u)),
            Glyph::Restart => {
                let r = (u * u + v * v).sqrt();
                let a = v.atan2(u);
                // a ring open at the top right, with an arrowhead at the end of the opening
                let ring = (0.23..=0.40).contains(&r) && !(-1.45..=-0.40).contains(&a);
                let at = |r: f32, a: f32| (r * a.cos(), r * a.sin());
                ring || in_triangle((u, v), at(0.12, -0.40), at(0.51, -0.40), at(0.315, -0.98))
            }
            Glyph::Cross => {
                u.abs() <= 0.36 && v.abs() <= 0.36 && ((u - v).abs() <= 0.13 || (u + v).abs() <= 0.13)
            }
        }
    }
}

/// How much the glyphs are enlarged inside the circle: legible at the tray's 16 px, clear of the rim
const GLYPH_SCALE: f32 = 1.45;

fn in_triangle(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let side = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| (p.0 - r.0) * (q.1 - r.1) - (q.0 - r.0) * (p.1 - r.1);
    let (d1, d2, d3) = (side(p, a, b), side(p, b, c), side(p, c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

/// RGBA pixels of `glyph` in a white circle with a thin grey rim (so it shows on a light taskbar too),
/// size × size, anti-aliased by 4×4 supersampling.
pub fn rgba(glyph: Glyph, size: u32) -> Vec<u8> {
    const SUB: u32 = 4;
    const RADIUS: f32 = 0.96;
    const RIM: f32 = 0.09;
    let white = [255.0, 255.0, 255.0];
    let rim = [0x8c as f32, 0x95 as f32, 0x9f as f32];
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut sum, mut cover) = ([0.0f32; 3], 0.0f32);
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let u = ((x * SUB + sx) as f32 + 0.5) / (size * SUB) as f32 * 2.0 - 1.0;
                    let v = ((y * SUB + sy) as f32 + 0.5) / (size * SUB) as f32 * 2.0 - 1.0;
                    let r = (u * u + v * v).sqrt();
                    if r > RADIUS {
                        continue;
                    }
                    let col = if glyph.covers(u, v) { glyph.rgb() } else if r > RADIUS - RIM { rim } else { white };
                    for i in 0..3 {
                        sum[i] += col[i];
                    }
                    cover += 1.0;
                }
            }
            let n = (SUB * SUB) as f32;
            let c = |i: usize| if cover > 0.0 { (sum[i] / cover).round() as u8 } else { 0 };
            px.extend_from_slice(&[c(0), c(1), c(2), (cover / n * 255.0).round() as u8]);
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_colour_sits_inside_a_white_circle() {
        let px = rgba(Glyph::Play, 32);
        let at = |x: u32, y: u32| &px[((y * 32 + x) * 4) as usize..][..4];
        assert_eq!(at(0, 0)[3], 0, "corner is transparent");
        assert_eq!(at(17, 16), &[0x2e, 0xa0, 0x43, 255], "centre is the green triangle");
        assert_eq!(at(16, 4), &[255, 255, 255, 255], "above the triangle is white");
    }
}

#[cfg(test)]
mod dump {
    /// GLYPH_DUMP=<dir>: writes each glyph's raw RGBA there, for looking at them
    #[test]
    fn dump_glyphs() {
        let Ok(dir) = std::env::var("GLYPH_DUMP") else { return };
        for (name, g) in [("play", super::Glyph::Play), ("pause", super::Glyph::Pause), ("restart", super::Glyph::Restart), ("cross", super::Glyph::Cross), ("busy", super::Glyph::Busy)] {
            for size in [16u32, 32, 64] {
                std::fs::write(format!("{dir}/{name}-{size}.rgba"), super::rgba(g, size)).unwrap();
            }
        }
    }
}
