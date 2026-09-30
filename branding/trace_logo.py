"""Regenerate the terminal silhouette from the checked-in UkisAI logo."""
from pathlib import Path
import base64
import io
import re

import numpy as np
from PIL import Image
from scipy import ndimage
from skimage.measure import approximate_polygon, find_contours

root = Path(__file__).resolve().parents[1]
source = (root / "branding/ukisai.svg").read_text(encoding="utf-8")
png = re.search(r'data:image/png;base64,([^"]+)', source).group(1)
image = np.array(Image.open(io.BytesIO(base64.b64decode(png))))
mask = (image[:, :, 3] > 160) & (image[:, :, :3].max(axis=2) > 60)
labels, _ = ndimage.label(mask)
counts = np.bincount(labels.ravel())
counts[0] = 0
mask = ndimage.binary_closing(counts[labels] > 600, iterations=2)
paths = []
for contour in find_contours(np.pad(mask, 2), 0.5):
    if len(contour) < 40:
        continue
    polygon = approximate_polygon(contour, tolerance=1.2) - 2
    paths.append("M" + "L".join(f"{x:.1f} {y:.1f}" for y, x in polygon[:-1]) + "Z")
y, x = np.where(mask)
bounds = [float(x.min()), float(y.min()), float(x.max()), float(y.max())]
output = root / "codex-rs/tui/src/empty_state_animation/paths.rs"
output.write_text(
    "//! UkisAI interlocking chain silhouette, traced from branding/ukisai.svg.\n"
    "//! See branding/README.md for provenance and regeneration.\n\n"
    "pub(super) const UKIS: (&str, [f64; 4]) = (\n"
    f'    "{"".join(paths)}",\n    {bounds},\n);\n',
    encoding="utf-8",
)
