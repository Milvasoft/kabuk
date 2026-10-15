// Ember enso: one brushed circle of glowing vermilion, pressed hard where the brush
// lands and running dry as it closes, with a small seal beside it.

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float vnoise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash21(i);
    float b = hash21(i + float2(1.0, 0.0));
    float c = hash21(i + float2(0.0, 1.0));
    float d = hash21(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, f.x), lerp(c, d, f.x), f.y);
}

static const float TAU = 6.2831853;

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float R = 0.265 * size;
    float2 c = float2(-0.04, 0.015);
    float2 q = p - c;

    // The stroke runs clockwise from the lower left over most of the circle.
    float a0 = 3.75;
    float span = 0.9;
    float ang = atan2(q.y, q.x);
    float s = frac((a0 - ang) / TAU);
    float t = s / span;

    // Before the start (t < 0) the stroke ends in the rounded head where the brush landed.
    float tc = t > 1.0 + (1.0 / span - 1.0) * 0.5 ? (s - 1.0) / span : t;
    float tw = max(tc, 0.0);

    // The brush's centre line wanders a little off the circle; the width swells after
    // the press and thins towards the dry end.
    float rr = R * (1.0 + 0.025 * sin(tw * 5.0 + 0.7) - 0.035 * tw * tw);
    float w = R * 0.15 * (0.9 + 0.22 * smoothstep(0.0, 0.12, tw)) * (1.0 - 0.72 * smoothstep(0.45, 1.0, tw));
    float dr = length(q) - rr;
    float a = dr / w; // across the stroke, -0.5 .. 0.5

    // Ragged edges and bristle streaks.
    float edge = 0.5 - 0.06 * vnoise(float2(tc * 60.0, 3.1)) - 0.04 * vnoise(float2(tc * 170.0, 7.7));
    float aa = 1.5 * pxs / w;
    float along = max(-tc, 0.0) * span * TAU * rr / w; // behind the start, in widths
    float capd = length(float2(along * 1.6, a));
    float body = smoothstep(edge, edge - aa, capd) * step(tc, 1.0);

    float n = 0.6 * vnoise(float2(a * 24.0, tc * 2.2 + a * 2.0)) + 0.4 * vnoise(float2(a * 75.0, tc * 7.0));
    float dry = dryness * (lerp(0.0, 0.95, smoothstep(0.25, 1.0, tc)) + 0.5 * abs(a) * smoothstep(0.15, 0.8, tc));
    float ink = smoothstep(dry - 0.04, dry + 0.04, n);
    // The very end breaks into a few bristles only.
    ink *= smoothstep(1.0, 0.94, tc) + 0.6 * step(0.62, n) * step(tc, 1.0) * smoothstep(1.04, 0.98, tc);
    float headInk = 0.0;
    float2 head = c + rr * float2(cos(a0), sin(a0));
    float2 tang = float2(sin(a0), -cos(a0));
    t = tc;

    float load = 0.62 + 0.38 * vnoise(float2(t * 3.0, a * 4.0));
    load *= lerp(1.0, 0.7, smoothstep(0.4, 1.0, t)) * (1.0 + 0.25 * smoothstep(0.08, -0.05, t));
    float k = body * ink * load;

    float3 hot = float3(1.0, 0.5, 0.22);
    float3 deep = float3(0.82, 0.16, 0.07);
    float3 col = lerp(deep, hot, saturate(load * 1.2 - 0.25)) * k;

    // A faint warmth around the inked parts only.
    float halo = exp(-max(capd - 0.5, 0.0) * w / (0.012 * size)) * step(t, 1.0) * (1.0 - 0.7 * smoothstep(0.3, 1.0, t));
    col += deep * halo * 0.05;

    // A few splashes thrown where the brush landed.
    [unroll]
    for (int i = 0; i < 7; i++)
    {
        float fi = float(i);
        float2 o = float2(hash21(float2(fi, 3.0)), hash21(float2(fi, 9.0)));
        float2 nrm = float2(cos(a0), sin(a0));
        float2 sp = head - tang * (0.03 + 0.07 * o.x) * size + nrm * (o.y - 0.5) * 0.05 * size;
        float rad = (0.0012 + 0.0022 * hash21(float2(fi, 17.0)) * (1.0 - o.x)) * size;
        col += deep * 0.8 * smoothstep(rad + pxs, rad - pxs, length(p - sp));
    }

    // The seal: a small vermilion square with carved lines, lower right.
    float2 sc = c + float2(R * 1.32, -R * 1.02);
    float sh = 0.024 * size;
    float2 sq = p - sc;
    float2 sb = abs(sq) - sh;
    float sd = max(sb.x, sb.y) + 0.0015 * vnoise(sq * 900.0);
    float seal = smoothstep(pxs, -pxs, sd);
    float2 g = sq / sh;
    float carve = 0.0;
    carve = max(carve, step(abs(g.x + 0.32), 0.07) * step(abs(g.y), 0.62));
    carve = max(carve, step(abs(g.x - 0.3), 0.07) * step(abs(g.y + 0.05), 0.55));
    carve = max(carve, step(abs(g.y - 0.08), 0.07) * step(abs(g.x - 0.3), 0.32));
    carve = max(carve, step(abs(g.y + 0.58), 0.07) * step(abs(g.x), 0.62));
    carve *= step(0.18, vnoise(sq * 500.0));
    col += float3(0.78, 0.1, 0.06) * seal * (1.0 - carve) * (0.75 + 0.25 * vnoise(sq * 300.0));

    return max(col - 0.002, 0.0);
}
