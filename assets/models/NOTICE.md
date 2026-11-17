# 第三方模型版权与授权声明

## Challaborough Pillbox

本项目包含 artfletch 创作的 **Challaborough Pillbox** 摄影测量模型。原模型、其中的网格及贴图的著作权由原作者或相应权利人保留；将素材收录于本项目不表示本项目取得这些素材的著作权。

- **作者／署名**：artfletch
- **作者主页**：https://sketchfab.com/artfletch
- **原始作品**：https://sketchfab.com/3d-models/challaborough-pillbox-374f28fe248646aab57223afaebbcae9
- **素材许可**：Creative Commons Attribution 4.0 International（CC BY 4.0，知识共享署名 4.0 国际）
- **许可摘要**：https://creativecommons.org/licenses/by/4.0/
- **完整许可条款**：https://creativecommons.org/licenses/by/4.0/legalcode

上述作者、作品名称、来源链接和许可均记录在随附 GLB 的 `asset.extras` 元数据中。

## 本项目中的文件与修改说明

| 文件 | 来源与处理 |
| --- | --- |
| `challaborough_pillbox.glb` | 原始下载的模型文件，在本项目中未改写；保留原作者元数据和嵌入贴图 |
| `challaborough_pillbox.sweh` | 本项目从上述 GLB 生成的派生单层高度场，附带有效覆盖标记；本项目对该派生资源采用 CC BY 4.0 许可，并保留原作品署名 |

派生处理应用了 glTF 节点变换，排除了完全位于模型包围盒底面及四个竖直裁切平面上的三角形，再通过俯视采样取最高表面。默认裁切容差为 0.002 个模型单位，删除 4,264 个三角形；高度场尺寸为 984×890。原始 GLB 保持不变。运行时显示可以使用同样的裁切面清理，并调整模型比例、摆放和水位。这些处理由本项目完成，不是原作者提供或认可的水动力学模型。

原始 GLB 的 SHA-256：

```text
e6830ab3aa69e204815be613687a27e3ee4bc5a637f8569e3ed6e0e9def25033
```

## 使用与再分发

CC BY 4.0 允许在遵守许可条件的前提下复制、再分发和改编素材，包括商业用途。再分发原模型或改编版本时，应保留适当署名、作品来源和许可链接，注明所作修改，并保留适用的已有声明。不得暗示原作者为本项目或任何使用方式背书，也不得通过额外法律条款或技术措施限制许可授予的权利。

本声明仅适用于上述第三方模型及所列派生资源，不将项目代码或其他素材统一改为 CC BY 4.0。素材按许可条款以现状提供，不作保证；其他权利可能仍需另行取得许可。此处为说明性摘要，授权范围、条件及免责事项以完整许可条款为准。

简短署名示例：

> “Challaborough Pillbox” by artfletch, via Sketchfab, licensed under CC BY 4.0. Modified for this project: removal of artificial scan caps and derivation of a single-layer heightfield.
>
> Source: https://sketchfab.com/3d-models/challaborough-pillbox-374f28fe248646aab57223afaebbcae9
>
> License: https://creativecommons.org/licenses/by/4.0/
