# 内置模型与烘焙资源

GLB 和 `.fogscene` 通过 Git LFS 保存，克隆后先运行 `git lfs pull` 再构建；`.fogterrain` 直接保存在 Git 中。

原始文件位于仓库 `assets/models/`；作者、来源及许可见 [NOTICE](../../../assets/models/NOTICE.md)。大屯自然公园及其派生资源为 **CC BY-NC 4.0，不可作为自由商用素材使用**；Grandma’s House 为 CC BY 4.0。

| 项目 | 大屯自然公园（默认） | Grandma’s House |
| --- | --- | --- |
| 原始三角形 | 358,598 | 163,066 |
| 导入的不透明三角形 | 358,598 | 77,554 |
| 简化静态碰撞三角形 | 30,365 | 20,188 |
| meshopt 报告误差 | 0.09997 m | 0.09996 m |
| 256² 有效高度格 | 41,741 / 65,536 | 50,675 / 65,536 |
| 重叠高度命中计数 | 4,278 | 127,843 |
| 世界包围盒尺寸 X/Y/Z | 59.585438 / 9.827565 / 64 m | 59.820602 / 80.82076 / 64 m |
| 节点变换后平移前减去的 XYZ | −0.22738695 / −0.19000079 / −0.08084202 | −0.906301 / −1.6740599 / 1.6787891 |
| 统一缩放系数 | 23.095121 | 2.7051432 |

以上米制尺度是实验场归一化选择，不是实测地理尺度。所有节点矩阵先累计到世界空间，再以最长 XZ 边归一化到 64 米、XZ 居中、最低 Y 放到零。高度场覆盖 `[-32,-32,64,64]`，格距 0.25 m；缺少几何覆盖的格保持无效，不填零地面。

## 资源职责

- `datun.fogscene` / `grandma.fogscene`：不透明显示三角形、基色贴图采样后的线性 RGB 顶点颜色、独立简化碰撞三角形。格式 `FOGSCN02`：8 字节 magic，四个 little-endian u32（floor 数、obstacle 数、颜色标记、碰撞三角数），随后是 f32 三角形位置、可选 RGB 顶点颜色、碰撞三角形。加载检查长度、数量及有限数。
- `.fogterrain`：从原始导入的不透明显示几何取最高表面，包含高度、有效格、通行面和整个 BakeSource 的 SHA-256；不从简化网格生成高度。简化碰撞中的陡立面仅在与局部地面到 1.8 m 高区间重叠时生成额外阻挡面，避免把高处屋檐直接投影成无限高障碍。
- 静态碰撞焊接容差 0.00001 m，meshopt 简化目标 8,000 三角形，绝对误差预算 0.10 m。目标不是硬上限：拓扑／误差限制使上述两模型提前停止，不为凑数量无限放大误差。meshopt 误差不是严格 Hausdorff 上界；薄细节可能丢失。
- 相机在运行时仅从简化碰撞构建一次中位分割 BVH，叶节点最多 8 三角形；每帧先用相机臂扫掠包围盒筛选，再做球／三角形及边缘胶囊相交。角色仍使用高度场及共享阻挡面，不是完整刚体／三角网格角色控制器；未引入物理引擎。
- 当前模型显示是顶点颜色近似，不是完整 glTF PBR。透明、alpha-mask 草叶／树叶／贴花／水面、透射和发光扩展不在转换范围。原始 GLB 完整保留，未改写。
- 高度场只支持一层最高表面；树冠、屋顶、桥梁、室内与上下层重叠不具备独立的层下雾／层下行走空间。重叠计数公开保留，不能把最高表面称为自动识别出的可行走地面。

## 显式重新烘焙

输出必须不存在。以下路径是示例新文件名，不覆盖已发布资产；确认结果后再显式替换仓库资产。

```powershell
cargo run --release -p swe-fog -- --import-glb assets/models/taiwan_yangmingshan_-_datun_nature_park.glb --bake-scene datun-new.fogscene --bake datun-new.fogterrain --model-span 64 --resolution 256
cargo run --release -p swe-fog -- --import-glb assets/models/dae_diorama_-_grandmas_house.glb --bake-scene grandma-new.fogscene --bake grandma-new.fogterrain --model-span 64 --resolution 256
cargo run --release -p swe-fog -- --scene datun-new.fogscene --terrain datun-new.fogterrain --no-settings
```

可单独用 `--scene xxx.fogscene --bake new.fogterrain` 重新烘焙高度场。运行时不读取原 GLB，三个内置场景均编译期嵌入，不依赖开发者下载目录。

原始 GLB SHA-256：

```text
datun   1fd74ae0228a0124e12dbcdfa7719552a2439ae1e9b9e1f3cb186ab69fe5452b
grandma 865179a1fd98aa861813753b13dbb16a182dfcda9299b6e2decf7ec1ddfc8a2f
```
