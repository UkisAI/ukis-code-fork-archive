# UkisAI terminal branding

Source: `UkisAI/ukisai`, `public/ukisavenir.svg` and `DESIGN.md`. The original logo is preserved as `ukisai.svg`.

- Pink: `#EDAEF9`
- Violet: `#A855F7`
- Purple: `#AC2EE7`
- Sky: `#84CBFF`
- Blue: `#81B1FA`
- Website canvas: `#000000`; body: `#EDEDED`

The CLI respects the user's terminal background. Ukis violet drives selected controls and text accents; measured contrast adjustment keeps them readable on light backgrounds and limited terminal palettes. The logo uses pink lighting, violet fill, blue rim light, and sky highlights on dark backgrounds. Light terminals retain the upstream high-contrast ink treatment.

`codex-rs/tui/src/empty_state_animation/paths.rs` contains a clean silhouette traced from the PNG embedded in the source SVG. Both animation keyframes use the chain mark, preserving the upstream rotation, fade, placement, and replay code. Raster speckles are excluded from the silhouette. The original raster's metallic surface is replaced with the terminal renderer's existing lighting.

To regenerate the silhouette, install Pillow, NumPy, SciPy, and scikit-image, then run `python branding/trace_logo.py` from the repository root. Review the resulting snapshots with `just test -p codex-tui` and cargo-insta before accepting changes.

Do not rename wire-protocol fields, authentication providers, or `.codex` paths as part of visual branding. Keep upstream license and notice files intact.

## Validation

The Ukis branding workflow compiles the actual geometry, lighting, and renderer modules, checks the animation loop and light-terminal coverage/contrast, and compares rendered frames against the TUI logo snapshots. It also checks Rust formatting. The preview PNG uses those compiled renderer cells. The local Node launcher was checked for syntax, missing-build errors, and argument forwarding.

The full `just test -p codex-tui` suite has not run successfully on the initial Windows workstation: the repository runner requires PowerShell 7, and the machine lacks the full C++ build prerequisites. Ubuntu also has a pre-existing interrupted package-manager state. A packaged CLI binary is not included in this initial branding commit. Inherited OpenAI CI workflows remain present and enabled; some require upstream infrastructure.
