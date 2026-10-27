"""Bake Pierre's CC0 splash atlas into four padded R8 layers and mip levels.

Offline only; requires Pillow. Runtime embeds the baked file, not Python/Pillow.
Layout: mip-major, then layer-major (TL, TR, BL, BR), then row-major pixels.
"""
from hashlib import sha256
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parent
source = ROOT / "pierre" / "Splash.png"
expected = "7ab4960575a41594a8c933a5bc6763b04a6420e322b33ea038404fb7a822c06e"
if sha256(source.read_bytes()).hexdigest() != expected:
    raise ValueError("Splash.png differs from the documented upstream source")
image = Image.open(source)
if image.mode != "RGBA" or image.size != (512, 512):
    raise ValueError("Expected the original 512x512 RGBA 2x2 atlas")
alpha = image.getchannel("A")
layers = []
for y in range(2):
    for x in range(2):
        tile = alpha.crop((x * 256, y * 256, (x + 1) * 256, (y + 1) * 256))
        tile = tile.resize((224, 224), Image.Resampling.LANCZOS)
        padded = Image.new("L", (256, 256), 0)
        padded.paste(tile, (16, 16))
        layers.append(padded)
output = bytearray()
size = 256
while True:
    for layer in layers:
        output.extend(layer.tobytes())
    if size == 1:
        break
    size //= 2
    layers = [layer.resize((size, size), Image.Resampling.BOX) for layer in layers]
path = ROOT / "splash-mips.r8"
path.write_bytes(output)
print(f"{path.name}: {len(output)} bytes, sha256={sha256(output).hexdigest()}")
