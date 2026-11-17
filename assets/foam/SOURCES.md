# Source records

## Crest 4 public repository

- Original authors / copyright: Wave Harmonic and contributors.
- Repository: https://github.com/wave-harmonic/crest
- Pinned commit: `db0658ff0b2e93e4a9e28cc2867509658b0ecc00`.
- Date introduced: 2026-10-26.
- Original core assets directory: `crest/Assets/Crest/Crest/Textures/`.
- Original core license file: `crest/Assets/Crest/Crest/LICENSE`; its text is retained in [crest/LICENSE.txt](crest/LICENSE.txt). This record applies only to this public version and does not establish a license for other versions or commercial assets.
- Local original images are unchanged; the adjacent `.meta` files are the original authors' Unity import settings, retained to document source semantics, not as Rust runtime configuration. Both original images specify `sRGBTexture: 1`. The original shader samples the first channel; our bake likewise uses only the first channel and converts sRGB to linear.

Links to the pinned version:

- [foam.png](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/Textures/foam.png)
- [Foam2.png](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/Textures/Foam2.png)
- [Original foam material](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/Shaders/OceanFoam.hlsl)
- [Core MIT license](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/LICENSE)

| Local original | SHA-256 |
| --- | --- |
| `crest/foam.png` | `B8803EFC9384722EE2BA2C72020E4E7A3B0C4BD8C1425E532528A6FB0FADBAD7` |
| `crest/foam.png.meta` | `CE24E08DDA857E528B0F8E43EEFB0038B4A47DBEBB1C8F83FCB93A9257336424` |
| `crest/Foam2.png` | `0F11E52A0BBA9EAEFD6EB26684DE18784058DE208D602496DF2C0F82AC950AD6` |
| `crest/Foam2.png.meta` | `F1BDA1EECBABF29D1E3C074FEE7CE3CE7361F051ACE4544FCCFE315E2134A59B` |
| `crest/LICENSE.txt` | `C19990F81E87A54E85AEB3C14325ABE855A3678572E19AA206FB0DCF308B25ED` |

`baked/crest_foam_nsh.png` and `baked/crest_foam2_nsh.png` are derivative assets processed offline by this project, not NSH textures supplied by Crest. Soft comes from their masks; Height and normals are generated using this project's appearance model.

## Existing Shorewaves asset

- Original input: [../shorewaves/voronoi_lace.png](../shorewaves/voronoi_lace.png).
- Original author: Reboot16 / Enrico K.
- Existing license: [../shorewaves/LICENSE.txt](../shorewaves/LICENSE.txt).
- The project's existing loader treats the grayscale first channel directly as linear data; the default bake follows that convention.
- Derived output: `baked/shorewaves_nsh.png`. The original input is retained and not overwritten.
