# Spray particle texture

## Source and license

The current texture is `Pierre01/Texture/Splash.png` from **Pierre's Effects vol.1**, by **日陰者ピエール (Pierre / HikagePierreACV)**, published on the official Effekseer sample page. The original is a 512×512 RGBA 2×2 atlas of four water-sheet/splash shapes; it is not a four-frame animation. This project randomly selects a shape per particle.

- Author and download: https://effekseer.github.io/en/contributes/Pierre01/index.html
- Official 1.30 archive: https://effekseer.github.io/contributes/Pierre01_130.zip
- Official license information: https://effekseer.github.io/en/contribute.html (the Pierre sample is in the CC0 section, not the old license section)
- License: **CC0 1.0 Universal**, permitting modification and redistribution; author attribution is retained as a source record.
- Full license: [pierre/LICENSE-CC0.txt](pierre/LICENSE-CC0.txt), originally from https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt.
- Repository original: [pierre/Splash.png](pierre/Splash.png), byte-for-byte identical to the file in the official archive above.
- Original SHA-256: `7ab4960575a41594a8c933a5bc6763b04a6420e322b33ea038404fb7a822c06e`.

OpenGameArt's Water Drop Particle and a small Splash animation were also considered; the former resembles individual water droplets and the latter has a pixel-art style. Neither is used here.

## Project processing

`bake.py` extracts the original's **alpha channel**, not its white RGB outline. It splits the atlas in top-left, top-right, bottom-left, bottom-right order, resizes each to 224×224, and centers it in a 256×256 layer with a 16-pixel transparent border. Nine mip levels are generated independently for each layer to avoid bleeding between atlas shapes and distant flicker. The original is unchanged.

The output `splash-mips.r8` is linear R8 data ordered mip → layer → row, with four layers and 349,524 bytes total. SHA-256: `29d279b448b1dc2e1ebab2c8df9784048f1e23059610bdfab632400a1a37dbbe`.

Offline rebuild:

```powershell
python -m pip install Pillow
python assets/spray/bake.py
```

Normal Rust builds and execution need neither Python, Pillow, nor network access. Rendering uses `texture_2d_array`, assigning a fixed random layer at particle birth; the quad rotates without rotating UVs and clipping the shape. Color comes from the shader, with premultiplied transparency and soft depth intersections rather than additive glow. Emission intensity debugging uses a separate circular marker unaffected by the artwork's shape.
