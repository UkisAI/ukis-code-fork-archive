//! Render the production logo modules without building the entire agent.
#![allow(dead_code)]

#[path = "../codex-rs/tui/src/color.rs"]
mod color;
#[path = "../codex-rs/tui/src/empty_state_animation/geometry.rs"]
mod geometry;
#[path = "../codex-rs/tui/src/empty_state_animation/lighting.rs"]
mod lighting;
#[path = "../codex-rs/tui/src/empty_state_animation/paths.rs"]
mod paths;
#[path = "../codex-rs/tui/src/empty_state_animation/renderer.rs"]
mod renderer;

fn main() {
    let dark = lighting::Lighting::terminal((210, 221, 235), (15, 20, 37));
    let light = lighting::Lighting::terminal((32, 32, 32), (250, 250, 250));
    let mut renderer = renderer::Renderer::default();
    for (width, height, phase) in [(60, 21, 0.0), (60, 21, 0.5), (48, 17, 1.5)] {
        let cells = renderer.frame(width, height, phase, &dark).to_vec();
        assert!(cells.iter().any(|cell| cell.dots != 0));
        let light_cells = renderer.frame(width, height, phase, &light);
        assert_eq!(
            cells.iter().map(|cell| cell.dots).collect::<Vec<_>>(),
            light_cells.iter().map(|cell| cell.dots).collect::<Vec<_>>()
        );
        for cell in light_cells.iter().filter(|cell| cell.dots != 0) {
            let [_, r, g, b] = cell.rgb.to_be_bytes();
            assert!(r < 200 && g < 200 && b < 200);
        }
        println!("FRAME {width} {height} {phase}");
        for cell in cells {
            println!("{} {}", cell.dots, cell.rgb);
        }
    }
    let first = renderer.frame(60, 21, 0.0, &dark).to_vec();
    assert_eq!(renderer.frame(60, 21, 1.0, &dark), first);
}
