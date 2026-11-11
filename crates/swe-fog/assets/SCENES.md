# 内置场景与烘焙资源

默认模型为 **DAE Diorama - Grandma’s House**，原网格实验场仍可切换。自然公园场景及资源已移除；旧会话 `scene: "datun"` 兼容读取为 `grandma`，其他参数保留，保存时写入新名称。

GLB 和 `.fogscene` 通过 Git LFS 保存，克隆后先运行 `git lfs pull` 再构建；`.fogterrain` 直接保存在 Git 中。原始模型位于 `assets/models/dae_diorama_-_grandmas_house.glb`，作者 Yarina Usenko / Crimoonza，许可 CC BY 4.0，详见 [NOTICE](../../../assets/models/NOTICE.md)。原 GLB 不改写。

## Grandma’s House 烘焙

| 项目 | 当前资源 |
| --- | --- |
| 原始 GLB 三角形 | 163,066 |
| 不透明显示三角形 | 77,554 |
| 参与碰撞简化的三角形 | 77,369（排除展台外壳） |
| 简化静态碰撞三角形 | 51,894 |
| 简化误差预算 / meshopt 报告误差 | 0.025 / 0.02478 m |
| 明确选取的高度场承载三角形 | 2,148 |
| 256² 有效高度格 | 43,963 / 65,536 |
| 承载面重叠命中计数 | 10,841 |
| 有效高度范围 | 9.576969–43.893604 m |
| 世界包围盒 X/Y/Z | 59.820602 / 80.82076 / 64 m |
| 节点变换后平移前减去的 XYZ | −0.906301 / −1.6740599 / 1.6787891 |
| 统一缩放系数 | 2.7051432 |

米制尺度是实验场归一化选择，不是实测尺度。累计节点变换后，将最长 XZ 边归一化为 64 米，XZ 居中，最低 Y 放到零。本次保持显示模型的坐标／尺度不变。

### 高度场不再从所有可见物体取最高点

`grandma.bake.json` 是显式烘焙配置：绑定原 GLB SHA-256，并逐项列出地面、桥和石阶网格。只从所选网格的向上表面取样（单位法线 Y≥0.5），不将房顶、树干、道具、展台外壳等当成承载面。选取结果单独存入 `.fogscene` 的 `support` 数组；改变分辨率／区域后仍从相同承载面重烘焙，不能回退成所有显示网格。

旧资源曾把全部不透明几何当作地面，最高有效格达到 78.903 m。新旧共同有效格中，11,205 格的高度下调超过 0.5 m，最大修正约 43.56 m；显示模型未随之删除。高度场仍是一层：桥下和桥上不能同时承载独立雾层；同一选取集合的重叠仍取最高面。缺少承载面的格保持无效，不猜测地面。

### 简化碰撞与地面独立

碰撞保留房屋、树干和实心道具，不因其未进入 `support` 而删除；仅按配置排除人工展台外壳。碰撞在同一世界变换下焊接（容差 0.00001 m），使用 meshopt、8,000 面目标、0.025 m 绝对误差预算，锁定开放边界。实际 51,894 面比旧资源的 20,188 面更多，这是收紧原 0.10 m 误差并保护开放边界的代价；目标不是硬上限，也不为凑数量放宽误差。meshopt 误差不是严格 Hausdorff 上界。

运行时相机只对简化网格构建一次 BVH，叶最多 8 三角形，每帧先筛选相机臂扫掠包围盒。角色仍使用高度场与局部阻挡面，不是完整三角网格刚体角色控制器，未引入物理引擎。陡立碰撞面与局部地面至 1.8 m 高范围重叠时生成共享面阻挡。

显示仍是原不透明几何和基色贴图采样的线性 RGB 顶点色，不是完整 PBR；透明／alpha-mask 树叶、草片、水面、透射和发光扩展仍不导入。

## 资源格式与重烘焙

- `grandma.fogscene`：`FOGSCN03`，8 字节 magic；五个 little-endian u32（显示 floor 数、obstacle 数、颜色标记、碰撞三角数、承载三角数）；随后为 f32 显示三角形、可选逐角 RGB、碰撞三角形、承载三角形。加载检查长度、数量及有限数。兼容读取 `FOGSCN02`，旧格式／手写 JSON 没有 `support` 时沿用全部 floor。
- `grandma.fogterrain`：高度、有效格、通行面与包含承载面在内的完整 BakeSource SHA-256。旧场景配新地形或反之会因来源不符拒绝加载。
- 烘焙配置要求来源 SHA 一致、网格名称存在、至少选出一个向上承载三角形；错误时拒绝输出，不静默回退所有网格。

输出必须不存在。先生成新文件，再显式替换发布资产：

```powershell
cargo run --release -p swe-fog -- --import-glb assets/models/dae_diorama_-_grandmas_house.glb --bake-profile crates/swe-fog/assets/grandma.bake.json --bake-scene grandma-new.fogscene --bake grandma-new.fogterrain --model-span 64 --resolution 256
cargo run --release -p swe-fog -- --scene grandma-new.fogscene --terrain grandma-new.fogterrain --no-settings
```

不指定 `--bake-profile` 的通用 GLB 导入仍保留旧的全部不透明面取高语义，不应据此覆盖 Grandma 正式资源。可用 `--scene grandma.fogscene --bake new.fogterrain` 重烘焙，已打包的承载面会保留。

原始 GLB SHA-256：`865179a1fd98aa861813753b13dbb16a182dfcda9299b6e2decf7ec1ddfc8a2f`。
