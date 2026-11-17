# Workspace icons

SVG assets for GPU Ocean's six-category navigation, integrated into the egui sidebar icon segmented control.

| File | Workspace | Graphic |
| --- | --- | --- |
| `scene.svg` | Scene | Isometric cube |
| `shallow.svg` | Shallow Water | Water surface and nearshore slope cross-section |
| `offshore.svg` | Offshore | Single curling wave |
| `animated-sdf.svg` | Animated SDF Ripples | Curved contours with propagation direction |
| `shading.svg` | Shading | Half-lit sphere and light source |
| `configurations.svg` | Configurations | Parameter card with controls |

All use `viewBox="0 0 24 24"`, 1.75-unit round-capped strokes and `currentColor`, displayed at 24 px by default on a transparent background. The SVGs have accessible names and no scripts, fonts, external resources, or embedded bitmaps.

Source: the original five icons used image-generation tools for style references and were reconstructed with simple vector paths; the Animated SDF Ripples icon was drawn directly as vector paths. None was copied from an external icon library or contains generated bitmaps. Reference images and browser previews are kept only in the system Temp directory and are not runtime dependencies.

Inline SVGs on web pages inherit text color; standalone decoding requires explicitly setting `currentColor`. egui's image loader caches display resources generated at the relevant scale; selection and color follow the theme. Browser previews do not replace checking the actual egui window.
