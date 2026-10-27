# 浪花粒子纹理

## 来源与许可

当前使用 **Pierre's Effects vol.1** 中的 `Pierre01/Texture/Splash.png`，作者 **日陰者ピエール（Pierre / HikagePierreACV）**，由 Effekseer 官方样例页面发布。原图是 512×512 RGBA 的 2×2 图集，包含四种水片／飞溅形状；它不是四帧动画，本项目按粒子随机选取一种。

- 作者与下载页：https://effekseer.github.io/en/contributes/Pierre01/index.html
- 官方 1.30 压缩包：https://effekseer.github.io/contributes/Pierre01_130.zip
- 官方许可说明：https://effekseer.github.io/en/contribute.html （Pierre 样例位于 CC0 部分，不是旧许可部分）
- 许可：**CC0 1.0 Universal**，允许修改和再分发；保留作者署名作为来源记录。
- 完整许可：[pierre/LICENSE-CC0.txt](pierre/LICENSE-CC0.txt)，原文来自 https://creativecommons.org/publicdomain/zero/1.0/legalcode.txt 。
- 仓库原图：[pierre/Splash.png](pierre/Splash.png)，与上述官方压缩包中的文件逐字节一致。
- 原图 SHA-256：`7ab4960575a41594a8c933a5bc6763b04a6420e322b33ea038404fb7a822c06e`。

检索时也查看了 OpenGameArt 的 Water Drop Particle 和小尺寸 Splash 动画；前者偏独立水球、后者偏像素风，未用于本项目。

## 本项目的处理

`bake.py` 从原图提取 **Alpha 通道**，不是取白色 RGB 轮廓。按左上、右上、左下、右下拆分，缩为 224×224 后居中放入 256×256 图层，四边留 16 像素透明边。各层独立生成 9 级 mip，避免图集相邻形状渗色和远距离闪烁。没有修改原图。

产物 `splash-mips.r8` 为线性 R8 数据，按 mip → layer → row 排列，包含四层，共 349,524 字节。SHA-256：`29d279b448b1dc2e1ebab2c8df9784048f1e23059610bdfab632400a1a37dbbe`。

离线重建：

```powershell
python -m pip install Pillow
python assets/spray/bake.py
```

正常 Rust 构建和运行不需要 Python、Pillow 或联网。渲染使用 `texture_2d_array`，每个粒子出生时固定一个随机图层；四边形自身旋转，不旋转 UV 后裁切图案。颜色由着色器提供，继续使用预乘透明度和深度软交界，不采用加法发光。发射强度调试使用独立圆形标记，不受美术纹理形状影响。
