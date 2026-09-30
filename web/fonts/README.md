# Title font

`lifeping-title.woff2` is [Dancing Script](https://github.com/googlefonts/DancingScript)
Bold, subset to the glyphs of "LifePing" (~2 KB). It is licensed under the SIL Open
Font License 1.1 (`OFL.txt`). Because "Dancing Script" is a Reserved Font Name and
subsetting counts as modification, the font is renamed "LifePing Title".

To regenerate (e.g. after changing the title text), with `fonttools` and `brotli`:

```sh
curl -Lo ds.ttf 'https://github.com/google/fonts/raw/main/ofl/dancingscript/DancingScript%5Bwght%5D.ttf'
fonttools varLib.instancer ds.ttf wght=700 -o ds700.ttf
pyftsubset ds700.ttf --text='LifePing' --layout-features='*' --flavor=woff2 \
  --name-IDs='' --output-file=sub.woff2
python3 - <<'EOF'
from fontTools.ttLib import TTFont, newTable
f = TTFont("sub.woff2")
name = newTable("name"); name.names = []
for nid, s in [
    (0, "Copyright 2016 The Dancing Script Project Authors (https://github.com/googlefonts/DancingScript). Subset for lifeping."),
    (1, "LifePing Title"), (2, "Bold"), (3, "LifePingTitle-Bold"),
    (4, "LifePing Title Bold"), (6, "LifePingTitle-Bold"),
    (13, "This Font Software is licensed under the SIL Open Font License, Version 1.1."),
    (14, "https://openfontlicense.org"),
]:
    name.setName(s, nid, 3, 1, 0x409)
f["name"] = name
f.save("lifeping-title.woff2")
EOF
```
