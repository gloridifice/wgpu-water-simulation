# 来源记录

## Crest 4 公共仓库

- 原作者／版权：Wave Harmonic and contributors。
- 仓库：https://github.com/wave-harmonic/crest
- 固定提交：`db0658ff0b2e93e4a9e28cc2867509658b0ecc00`。
- 引入日期：2026-10-26。
- 核心资源原目录：`crest/Assets/Crest/Crest/Textures/`。
- 核心许可原文件：`crest/Assets/Crest/Crest/LICENSE`；原文保留在 [crest/LICENSE.txt](crest/LICENSE.txt)。本记录仅针对该公共版本，不能推广为其他版本或商业素材的许可。
- 本地原图未修改；同目录 `.meta` 是原作者的 Unity 导入设置，保留以记录源图语义，不作为 Rust 运行时配置。两张原图均设置 `sRGBTexture: 1`。原着色器采样首通道；我们的烘焙同样只取首通道，并按 sRGB 转线性。

可直接检查的固定版本链接：

- [foam.png](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/Textures/foam.png)
- [Foam2.png](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/Textures/Foam2.png)
- [原泡沫材质](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/Shaders/OceanFoam.hlsl)
- [核心 MIT 许可](https://github.com/wave-harmonic/crest/blob/db0658ff0b2e93e4a9e28cc2867509658b0ecc00/crest/Assets/Crest/Crest/LICENSE)

| 本地原始文件 | SHA-256 |
| --- | --- |
| `crest/foam.png` | `B8803EFC9384722EE2BA2C72020E4E7A3B0C4BD8C1425E532528A6FB0FADBAD7` |
| `crest/foam.png.meta` | `CE24E08DDA857E528B0F8E43EEFB0038B4A47DBEBB1C8F83FCB93A9257336424` |
| `crest/Foam2.png` | `0F11E52A0BBA9EAEFD6EB26684DE18784058DE208D602496DF2C0F82AC950AD6` |
| `crest/Foam2.png.meta` | `F1BDA1EECBABF29D1E3C074FEE7CE3CE7361F051ACE4544FCCFE315E2134A59B` |
| `crest/LICENSE.txt` | `C19990F81E87A54E85AEB3C14325ABE855A3678572E19AA206FB0DCF308B25ED` |

`baked/crest_foam_nsh.png` 和 `baked/crest_foam2_nsh.png` 是本项目离线加工得到的派生资源，不是 Crest 原版提供的 NSH 贴图。Soft 取自其掩码；Height 与法线由本项目设计的外观模型生成。

## 已有 Shorewaves 资源

- 原输入：[../shorewaves/voronoi_lace.png](../shorewaves/voronoi_lace.png)。
- 原作者：Reboot16 / Enrico K.。
- 已有许可：[../shorewaves/LICENSE.txt](../shorewaves/LICENSE.txt)。
- 当前项目原加载方式将灰度首通道直接作为线性数据；默认烘焙沿用这一约定。
- 派生输出：`baked/shorewaves_nsh.png`。保留原输入，不覆盖原贴图。
