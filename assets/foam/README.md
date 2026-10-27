# 泡沫 NSH 烘焙资源

本目录提供三套用于对照的同布局泡沫细节数据。模拟密度、动量和双相位位移仍由现有近岸模拟产生；这些图片只用于外观。

| 运行时图片 | 原始输入 | 输入解释 |
| --- | --- | --- |
| `baked/shorewaves_nsh.png` | `../shorewaves/voronoi_lace.png` | 线性灰度，与原项目读取方式一致 |
| `baked/crest_foam_nsh.png` | `crest/foam.png` | sRGB 解码后的第一通道；原图 512×512 RGBA |
| `baked/crest_foam2_nsh.png` | `crest/Foam2.png` | sRGB 解码后的第一通道；原图 450×450 灰度 |

三张默认输出均为 1024×1024、8 位 RGBA、线性数据，没有 sRGB/gAMA 标签，不对 Alpha 预乘。增大较小原图时采用周期双线性插值，只统一采样尺寸，不增加原始图案细节；缩小时使用面积过滤，避免较细的输入图案因点采样而丢失覆盖。

## 通道约定

- R、G：切线法线 X/Y，分别从 -1～1 编码到 0～1。局部 Y 对应 UV 的 V 增大方向；局部 Z 指向表面外侧。渲染时应根据实际流动坐标变换到世界 XZ 水面基底。
- B：Soft，输入掩码经可调曲线得到的柔和分布，不是最后的不透明度。
- A：Height，从同一形态设计的相对起伏，不是测量的几何高度或以米表示的水深。

高度先由 Soft 的幂曲线得到，再作周期边界的可分离盒式模糊。法线按该高度的周期中心差分产生：

```text
法线 = normalize(-高度沿 U 的导数 × 相对高度,
                 -高度沿 V 的导数 × 相对高度,
                 1)
```

“相对高度”是高度与整块纹理边长的比值，默认 0.004，用于定义微法线坡度，不直接抬高渲染顶点。这个过程是一种美术近似，不能从原始灰度还原真实泡沫厚度。Normal、Soft、Height 来自同一输入，但当前不是新的泡径／网膜形态生成器。

## 重复烘焙

在仓库根目录执行：

```powershell
cargo run --features ocean --bin foam-bake
cargo test --features ocean --bin foam-bake
```

工具默认更新上述三张运行时资源及各自的 `.recipe.json`。recipe 记录输入路径、原图尺寸、颜色空间、算法版本、FNV-1a 内容指纹和全部参数；第三方原图的 SHA-256 另见 [SOURCES.md](SOURCES.md)。默认不下载任何网络内容，不改模拟或个人设置。

已烘焙 PNG 是版本化的运行时资源，供程序 `include_bytes!` 内嵌；重新烘焙时直接覆盖它们即可。不要先删除这些资源再构建工具，否则主库编译会因缺少内嵌文件失败。新克隆包含它们，可以直接运行命令。

自定义输入示例：

```powershell
cargo run --features ocean --bin foam-bake -- --input assets/shorewaves/voronoi_lace.png --output target/foam-test/nsh.png --input-space linear --height-gamma 1.35 --blur-radius 2 --normal-height 0.004
cargo run --features ocean --bin foam-bake -- --help
```

`--input-space` 必须与来源约定一致。工具按明确参数解释首通道，不自动把图片文件的显示亮度当成物理高度，也不使用原图 Alpha 代替掩码。对照 Crest 原效果时，必须保留其 sRGB 解码；如果刻意改成线性解释，应另存为新的制作方案并重新调覆盖曲线。

`--size` 支持 32～4096 的二次幂；`--soft-gamma`、`--height-gamma` 支持 0.1～8；`--blur-radius` 是输出像素中的模糊半径，范围 0～32；`--normal-height` 支持 0～0.1。参数均验证有限数。默认批处理保留输入强度范围，不逐图自动归一化，避免对照时隐式改变泡沫覆盖。

## 预览与比较

预览默认写入 `target/foam-bake-preview/`，不进入版本管理；可用 `--preview-dir` 更换目录，或 `--no-preview` 禁用。每张 `_preview.png` 是两行四列，顺序为：

1. 上排：Soft、Height、解码后的法线 RGB、打包 RGB（预览不使用 Alpha 透明）。
2. 下排：显示密度 0.2／0.5／0.8 时的示意覆盖、Soft 的 3×3 平铺。

预览覆盖使用一个明确的平滑阈值曲线，只用于比较输入形态，不声称与全部运行时光照完全相同。模拟中的泡沫密度不会被这条曲线修改。周期重采样、模糊和求导不会夹住图像边缘；输入自身若有明显内容接缝，仍需重新制作形态，工具不承诺自动消除所有接缝。

运行时生成 mip 时，应把 RG 解码成三维法线再平均、归一化和重新编码；B/A 单独作为线性标量过滤。不要把整张 RGB 作为普通法线，也不要把 A 覆盖为 255。

来源和许可见 [SOURCES.md](SOURCES.md)；Crest 派生 PNG 继续附带 `crest/LICENSE.txt`，Shorewaves 派生 PNG 继续附带 `../shorewaves/LICENSE.txt`。
