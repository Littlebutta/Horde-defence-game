#!/usr/bin/env python3
"""Bakes Natural Earth land polygons (public domain, naturalearthdata.com) into assets/land.bin for
the campaign globe: u32 ring count, then per ring u32 point count and i16 lon/lat pairs in 1/100
degrees, little endian. Points closer than STEP degrees to the last kept one are dropped.

    python3 tools/bake_land.py ne_50m_land.geojson assets/land.bin
"""
import json, struct, sys

STEP = 0.12
src, dst = sys.argv[1], sys.argv[2]
rings = []
for f in json.load(open(src))["features"]:
    g = f["geometry"]
    polys = g["coordinates"] if g["type"] == "MultiPolygon" else [g["coordinates"]]
    for poly in polys:
        ring = poly[0]  # outer ring only; lakes are drawn as land
        kept = [ring[0]]
        for p in ring[1:]:
            if abs(p[0] - kept[-1][0]) + abs(p[1] - kept[-1][1]) >= STEP:
                kept.append(p)
        if len(kept) >= 4:
            rings.append(kept)
out = bytearray(struct.pack("<I", len(rings)))
for r in rings:
    out += struct.pack("<I", len(r))
    for lon, lat in r:
        out += struct.pack("<hh", round(lon * 100), round(lat * 100))
open(dst, "wb").write(out)
print(len(rings), "rings,", sum(len(r) for r in rings), "points,", len(out), "bytes")
