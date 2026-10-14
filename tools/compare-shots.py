"""Side-by-side comparison of two screenshots (design page vs Kabuk).

    python tools/compare-shots.py design.png kabuk.png out.png [--zoom 3] [--crop x,y,w,h]...

Each crop region is taken from both images and stacked: design on top, Kabuk below,
enlarged without smoothing so pixel differences stay visible. Development tool only.
"""
import argparse

from PIL import Image


def main():
    p = argparse.ArgumentParser()
    p.add_argument("design")
    p.add_argument("kabuk")
    p.add_argument("out")
    p.add_argument("--zoom", type=int, default=3)
    p.add_argument("--crop", action="append", default=[])
    # Kabuk's crop is moved by this many pixels (when a region sits elsewhere, e.g. because
    # the design shows modules that arrive in later milestones).
    p.add_argument("--kshift", type=int, default=0)
    a = p.parse_args()
    d = Image.open(a.design).convert("RGBA")
    k = Image.open(a.kabuk).convert("RGBA")
    crops = [tuple(int(v) for v in c.split(",")) for c in a.crop] or [(0, 0, d.width, d.height)]
    rows = []
    for x, y, w, h in crops:
        for img, dx in ((d, 0), (k, a.kshift)):
            part = img.crop((x + dx, y, x + dx + w, y + h)).resize((w * a.zoom, h * a.zoom), Image.NEAREST)
            rows.append(part)
    width = max(r.width for r in rows)
    gap = 6
    out = Image.new("RGBA", (width, sum(r.height + gap for r in rows)), (255, 0, 255, 255))
    y = 0
    for r in rows:
        out.paste(r, (0, y))
        y += r.height + gap
    out.save(a.out)
    print(a.out, out.size)


if __name__ == "__main__":
    main()
