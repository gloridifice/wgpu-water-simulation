# 第三方模型版权与授权声明

## SWE 雾实验场模型

以下信息来自原始 GLB 的 `asset.extras`。原始 GLB 保持不变，版权归原作者／权利人；烘焙资源沿用对应素材许可，不适用项目代码许可，也不代表原作者认可本项目。

| 模型 | 作者与来源 | 许可 | 文件 |
| --- | --- | --- | --- |
| DAE Diorama - Grandma's House | [Yarina Usenko / Crimoonza](https://sketchfab.com/Crimoonza)，[原作品](https://sketchfab.com/3d-models/dae-diorama-grandmas-house-fe347d42b3374511ab169c0c0f9f4ed9) | [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)，[完整条款](https://creativecommons.org/licenses/by/4.0/legalcode) | `dae_diorama_-_grandmas_house.glb`；`crates/swe-fog/assets/grandma.fogscene`、`grandma.fogterrain` |

派生处理：应用节点层级变换、最长水平边归一化为 64 米、XZ 居中及最低 Y 平移到零；仅导入不透明三角形，将基色贴图烘焙成线性 RGB 顶点颜色；静态碰撞排除人工展台外壳，独立焊接、保留开放边界并以 0.025 米误差预算简化；小型碰撞部件另外校验覆盖距离以保留细枝；高度场选取地面、桥面、石阶、岩石顶面与凉亭底板的向上表面，并用高度带排除栏杆、桥墩底座与凉亭屋顶，不将树干和其他道具顶部当作地面，另烘焙阻挡面。透明／裁剪材质、完整 PBR、真实测绘尺度和多层室内空间不在此转换范围。详见 [烘焙记录](../../crates/swe-fog/assets/SCENES.md)。

再分发上述原件或派生资源时，保留作者、来源、许可和修改说明。素材按许可以现状提供，不作保证。

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
