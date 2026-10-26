# Challaborough Pillbox

- 作者：[artfletch](https://sketchfab.com/artfletch)
- 原始模型：[Challaborough Pillbox](https://sketchfab.com/3d-models/challaborough-pillbox-374f28fe248646aab57223afaebbcae9)
- 许可：[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/)
- 作者、来源和许可也保存在原始 GLB 的 `asset.extras` 中。
- 完整的项目素材署名与修改说明见 [`NOTICE.md`](NOTICE.md)。

`challaborough_pillbox.glb` 是原始扫描资源，未改写。它包含 1,077,042 个三角形、一个双面 unlit 材质和一张 4096×4096 JPEG。贴图中的摄影光照保留，不再叠加沙地的方向光照。模型比例没有实测校准；当前场景的比例、水位和摆放只是可调整的预览参数。

## 获取资源（Git LFS）

仓库通过根目录 `.gitattributes` 将 `*.glb` 和 `*.sweh` 交给 Git LFS 管理。Git 提交保存指针文件，模型及高度场的二进制内容保存在 LFS 对象中；其他已提交素材及历史不做迁移。

安装 [Git LFS](https://git-lfs.com/) 后，在克隆的仓库根目录执行：

```powershell
git lfs install --local
git lfs pull
git lfs ls-files
```

不要把 LFS 指针文本当作模型文件使用。缺少 LFS 对象时先执行 `git lfs pull`，无需重新烘焙已提供的高度场。发布时仍需附带实际二进制资源和本目录版权声明。

维护者正常执行 `git push` 时，Git LFS 的 pre-push hook 会先上传对应对象；仅创建本地提交不会上传模型。使用归档下载时，是否包含真实 LFS 对象取决于托管平台设置，推荐通过 Git 克隆并拉取 LFS 资源。

## 派生高度场

`challaborough_pillbox.sweh` 是本项目从该模型生成的单层高度场，沿用上述来源和许可。默认烘焙命令：

```powershell
cargo run --release --bin terrain-bake
```

默认源路径是本目录的 GLB，目标是本目录的 `.sweh`。每格不大于 0.05 个模型单位，得到 984×890 格；不假设模型单位已经准确对应米。工具应用 glTF 节点变换，删除完全落在包围盒底面和四个竖直裁切平面上的三角形（容差 0.002 个模型单位），再对剩余三角形进行俯视光栅化，取每格中心最高交点。此次默认处理删除 4,264 个裁切面三角形；运行时显示使用同样的清理规则。上方裁切面不自动删除。

这是一项明确的扫描处理规则，不是通用语义分割：其他模型上的天然平面可能需要不同参数。`--cap-epsilon 0` 可禁用清理。未命中的格子保留无效标记；本工具不猜测洞穴、悬挑下方或缺失扫描区域的形状。

```powershell
cargo run --release --bin terrain-bake -- --input assets/models/challaborough_pillbox.glb --output target/coast-preview.sweh --cell-size 0.1 --cap-epsilon 0.002
```

工具只创建不存在的输出，不覆盖已有文件。重新烘焙正式资产前，先将旧文件移走。模型修改后源 SHA-256 不匹配会拒绝加载，必须重新烘焙。烘焙不读写个人配置，也不需要 GPU。

### 文件格式 v1

- 8 字节 magic：`SWEHMAP1`。
- 4 字节小端 JSON 元数据长度。
- UTF-8 JSON：版本、源 GLB SHA-256、应用节点变换后的源包围盒、二维分辨率、裁切面容差。
- X 最快、Z 次之的单元中心数组。每格为两个小端 `f32`：高度、覆盖标记（0 或 1）。

高度使用模型场景坐标，不包含用户运行时平移、缩放、Y 轴旋转或海平面偏移。加载时检查版本、尺寸、数据长度、有限值、覆盖标记和源文件身份。它不是 PNG 灰度高度图，不做 sRGB 或 gamma 转换。

`.sweh` 与 GLB 都是运行时资产。发布程序时在可执行文件旁保留 `assets/models/` 及这两份文件；开发时也可从仓库根目录读取，另有编译时仓库位置的回退查找。不依赖原素材下载目录。
