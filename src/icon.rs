//! Ikona aplikace kreslená přímo v kódu: televize se symbolem přehrávání.

pub const SIZE: u32 = 64;

const BODY: [f32; 3] = [0.11, 0.13, 0.20];
const SCREEN_TOP: [f32; 3] = [0.25, 0.65, 1.00];
const SCREEN_BOTTOM: [f32; 3] = [0.10, 0.36, 0.82];
const STAND: [f32; 3] = [0.42, 0.47, 0.56];
const WHITE: [f32; 3] = [1.0, 1.0, 1.0];

/// RGBA pixely bez premultiplikace, `SIZE` × `SIZE`.
pub fn rgba() -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Vyhlazení hran: průměr z 4×4 vzorků na pixel.
            let mut color = [0.0f32; 3];
            let mut coverage = 0.0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if let Some(sample) = shade(px, py) {
                        for (channel, value) in color.iter_mut().zip(sample) {
                            *channel += value;
                        }
                        coverage += 1.0;
                    }
                }
            }
            if coverage > 0.0 {
                pixels.extend(color.map(|channel| (channel / coverage * 255.0).round() as u8));
            } else {
                pixels.extend([0, 0, 0]);
            }
            pixels.push((coverage / (SAMPLES * SAMPLES) as f32 * 255.0).round() as u8);
        }
    }
    pixels
}

fn shade(x: f32, y: f32) -> Option<[f32; 3]> {
    if in_triangle(x, y, (27.0, 20.0), (27.0, 36.0), (40.0, 28.0)) {
        return Some(WHITE);
    }
    if in_rounded_rect(x, y, (7.0, 11.0, 57.0, 45.0), 3.0) {
        let t = (y - 11.0) / 34.0;
        return Some(std::array::from_fn(|i| {
            SCREEN_TOP[i] + (SCREEN_BOTTOM[i] - SCREEN_TOP[i]) * t
        }));
    }
    if in_rounded_rect(x, y, (3.0, 7.0, 61.0, 49.0), 7.0) {
        return Some(BODY);
    }
    if in_rounded_rect(x, y, (28.0, 48.0, 36.0, 56.0), 0.0)
        || in_rounded_rect(x, y, (17.0, 54.0, 47.0, 59.0), 2.5)
    {
        return Some(STAND);
    }
    None
}

fn in_rounded_rect(
    x: f32,
    y: f32,
    (left, top, right, bottom): (f32, f32, f32, f32),
    radius: f32,
) -> bool {
    if x < left || x > right || y < top || y > bottom {
        return false;
    }
    let cx = x.clamp(left + radius, right - radius);
    let cy = y.clamp(top + radius, bottom - radius);
    (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
}

fn in_triangle(x: f32, y: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let edge = |p: (f32, f32), q: (f32, f32)| (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0);
    let (d1, d2, d3) = (edge(a, b), edge(b, c), edge(c, a));
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_size_and_transparent_corner() {
        let pixels = rgba();
        assert_eq!(pixels.len(), (SIZE * SIZE * 4) as usize);
        assert_eq!(pixels[3], 0, "levý horní roh má být průhledný");
        let center = ((28 * SIZE + 32) * 4) as usize;
        assert_eq!(
            &pixels[center..center + 4],
            &[255, 255, 255, 255],
            "střed je bílý trojúhelník"
        );
    }
}
