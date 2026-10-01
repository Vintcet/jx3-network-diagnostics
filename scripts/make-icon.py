from pathlib import Path
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parents[1]
out = root / 'src-tauri' / 'icons'
out.mkdir(exist_ok=True)
im = Image.new('RGBA', (256, 256), (0, 0, 0, 0))
d = ImageDraw.Draw(im)
d.rounded_rectangle((8, 8, 248, 248), radius=52, fill='#2458a6')
d.line([(44, 139), (83, 139), (106, 80), (135, 181), (160, 116), (181, 139), (212, 139)], fill='white', width=14, joint='curve')
d.ellipse((199, 126, 224, 151), fill='#83d5dc')
im.save(out / 'icon.ico', sizes=[(16, 16), (32, 32), (48, 48), (128, 128), (256, 256)])
im.save(out / 'icon.png')
