// Ebru flow: Turkish paper marbling. Bands of ink lie on the size, a few
// drops of colour open into rings, a comb is drawn through them one way and
// back, and a slow wave sways the whole bath. The comb and the wave never
// stop moving, very slowly, so the pattern flows like ink on water. Each
// step is undone exactly per pixel (the marbling maps of Jaffer and Lu).
// With "follow" the inks take the look's accent colours.

float eb_hash(float n)
{
    return frac(sin(n * 91.3458) * 47453.5453);
}

float eb_hash2(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// Distance from x to the nearest tine of a comb.
float eb_tine(float x, float spacing, float off)
{
    float m = frac((x - off) / spacing);
    return spacing * min(m, 1.0 - m);
}

// One of the five inks.
float3 eb_ink(float k, float3 i0, float3 i1, float3 i2, float3 i3, float3 i4)
{
    float3 c = i0;
    c = lerp(c, i1, step(0.18, k));
    c = lerp(c, i2, step(0.34, k));
    c = lerp(c, i3, step(0.48, k));
    c = lerp(c, i4, step(0.62, k));
    return c;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;
    float calm = smoothstep(0.04, 0.34, uv.x / aspect);

    // The inks: classic (indigo, ochre, rose, sage, paper) or the look's.
    float3 paper = float3(0.86, 0.82, 0.72);
    float3 a1 = tok_accent;
    float3 a2 = tok_accent2;
    float3 i0 = lerp(float3(0.13, 0.19, 0.38), a1 * 0.42, follow);
    float3 i1 = lerp(float3(0.76, 0.60, 0.33), a1, follow);
    float3 i2 = lerp(float3(0.66, 0.34, 0.34), lerp(a2, a1 * 0.7, 0.3), follow);
    float3 i3 = lerp(float3(0.42, 0.52, 0.45), lerp(a2, float3(0.5, 0.5, 0.5), 0.5) * 0.7, follow);
    float3 i4 = lerp(paper, lerp(paper, a1, 0.12), follow);
    float3 vein = lerp(float3(0.93, 0.90, 0.82), lerp(float3(0.93, 0.92, 0.90), a1, 0.15), follow);

    float2 q = uv;

    // Undo the wave that sways the bath.
    float wa = 0.035 * calm;
    q.x -= wa * sin(q.y * 4.1 + t * 0.031) + wa * 0.5 * sin(q.y * 9.7 - t * 0.023 + 1.3);
    q.y -= 0.02 * calm * sin(q.x * 3.3 - t * 0.027);

    // Undo four slow swirls drawn with a stylus: each turns the ink around
    // its centre, more near it; they twist back and forth and never tear.
    [unroll] for (int j = 3; j >= 0; j--)
    {
        float fj = j;
        float2 sc = float2(aspect * (0.42 + 0.17 * fj), 0.22 + 0.56 * frac(fj * 0.381 + 0.15));
        float2 sv = q - sc;
        float sa = 2.4 * calm * sin(t * (0.0022 + 0.0006 * fj) + fj * 2.4) * exp(-length(sv) / 0.16);
        float cs = cos(sa);
        float sn = sin(sa);
        q = sc + float2(cs * sv.x - sn * sv.y, sn * sv.x + cs * sv.y);
    }

    // Undo the comb: a fine comb drawn across and back in gentle waves, the
    // gel-git of the masters, leaving soft arches in every band.
    float drift = t * 0.0012;
    float zc = 0.045 * comb * calm;
    q.y -= zc * sin(q.x * 31.0 + drift * 31.0);
    q.x -= 0.6 * zc * sin(q.y * 23.0 - drift * 17.0);

    // Undo the drops, the last one first: each is three drops of ink let
    // fall on the same spot, so it opens into rings and pushes the bands
    // aside.
    float got = 0.0;
    float3 acc = 0.0;
    [unroll] for (int i = 3; i >= 0; i--)
    {
        float fi = i;
        float2 c = float2(aspect * (0.40 + 0.17 * fi + 0.03 * sin(t * 0.013 + fi * 2.1)),
                          0.15 + 0.7 * frac(fi * 0.618 + 0.3) + 0.03 * sin(t * 0.011 + fi));
        float rr = 0.05 + 0.03 * eb_hash(fi + 11.0);
        float r2 = rr * rr;
        float2 v = q - c;
        float dd = max(dot(v, v), 1e-6);
        // Inside: which ring.
        float rc = dd / r2;
        float w = fwidth(rc);
        float inside = 1.0 - smoothstep(3.0 - w, 3.0 + w, rc);
        float ring = floor(rc);
        float k0 = eb_hash(fi * 7.0 + ring);
        float3 rcol = eb_ink(k0, i0, i1, i2, i3, i4);
        float edge = abs(frac(rc + 0.5) - 0.5);
        rcol = lerp(rcol, vein, (1.0 - smoothstep(0.0, w * 1.6, edge)) * 0.75 * step(0.5, rc));
        float wt = (1.0 - got) * inside;
        acc += rcol * wt;
        got += wt;
        // Outside: undo the push.
        q = c + v * sqrt(max(1.0 - 3.0 * r2 / dd, 0.0));
    }

    // The first inks: bands laid across the bath, of uneven widths, each
    // colour a few strokes wide with pale veins between the strokes.
    float b = q.y / 0.032 + 0.6 * sin(q.y * 4.3) + 0.4 * sin(q.x * 1.9 + q.y * 2.6);
    float idx = floor(b);
    float grp = floor(idx / 3.0 + 0.6 * eb_hash(idx));
    float3 band = eb_ink(eb_hash(grp + 17.0), i0, i1, i2, i3, i4);
    band *= 0.9 + 0.1 * eb_hash(idx + 3.0);
    float wb = fwidth(b);
    float e = abs(frac(b + 0.5) - 0.5);
    band = lerp(band, vein, (1.0 - smoothstep(0.0, wb * 1.4, e)) * 0.55);
    float3 avg = (i0 + i1 + i2 + i3 + i4) * 0.2;
    // Where the strokes crowd too tightly, let them blend.
    band = lerp(band, avg, saturate(wb * 2.0 - 0.6));

    float3 col = acc + band * (1.0 - got);

    // The paper's grain and a soft quiet toward the icons.
    col = lerp(lerp(col, avg, 0.55), col, calm);
    col += (eb_hash2(px) - 0.5) / 255.0;
    float2 vv = px / resolution - 0.5;
    col *= 1.0 - 0.25 * dot(vv, vv);
    return col;
}
