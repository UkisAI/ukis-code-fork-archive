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
        let rows = cells
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        if cell.dots == 0 {
                            ' '
                        } else {
                            char::from_u32(0x2800 + u32::from(cell.dots)).unwrap()
                        }
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        if width == 48 {
            verify_snapshot(
                "snapshots/codex_tui__empty_state_animation__tests__onboarding_settled_logo.snap",
                &rows.join("\n"),
            );
        } else if phase == 0.0 {
            verify_snapshot(
                "onboarding/snapshots/codex_tui__onboarding__welcome__tests__welcome_logo_160x48.snap",
                &format!(
                    "{}\n\n  Welcome to UkisAI Code, UkisAI's coding agent, powered by Codex",
                    rows.join("\n")
                ),
            );
        } else {
            let first = cells.iter().find(|cell| cell.dots != 0).unwrap();
            let [_, r, g, b] = first.rgb.to_be_bytes();
            let fade = [(0, 1.0), (200, 0.59), (400, 0.18)].map(|(ms, alpha)| {
                let (r, g, b) = color::blend((r, g, b), (15, 20, 37), alpha);
                format!(
                    "{ms}ms: {} Rgb({r}, {g}, {b})",
                    char::from_u32(0x2800 + u32::from(first.dots)).unwrap()
                )
            });
            verify_snapshot(
                "snapshots/codex_tui__empty_state_animation__tests__first_screen_replay_fade.snap",
                &fade.join("\n"),
            );
        }
        for cell in cells {
            println!("{} {}", cell.dots, cell.rgb);
        }
    }
    let first = renderer.frame(60, 21, 0.0, &dark).to_vec();
    assert_eq!(renderer.frame(60, 21, 1.0, &dark), first);
}

fn verify_snapshot(path: &str, expected: &str) {
    let snapshot = std::fs::read_to_string(format!("codex-rs/tui/src/{path}")).unwrap();
    let snapshot = snapshot.replace("\r\n", "\n");
    let body = snapshot.splitn(3, "---\n").nth(2).unwrap();
    assert_eq!(body.trim_end(), expected.trim_end(), "{path}");
}
