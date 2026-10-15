// Abyssal jellies: a few bioluminescent jellyfish rise slowly through the deep, their
// bells breathing, their frilled arms and tentacles trailing in waves with sparks of
// light running down them; marine snow sinks past. Each jelly crosses the screen and
// comes round again.

static const int JELLIES = 5;
static const int TENTACLES = 8;

float hash11(float n)
{
    return frac(sin(n * 78.233) * 43758.5453);
}

float3 jellyColour(float i)
{
    float3 c = float3(0.35, 0.85, 1.0);                  // cyan
    c = i > 0.5 ? float3(0.68, 0.48, 1.0) : c;           // violet
    c = i > 1.5 ? float3(1.0, 0.45, 0.72) : c;           // rose
    c = i > 2.5 ? float3(0.3, 1.0, 0.78) : c;            // sea green
    c = i > 3.5 ? float3(0.5, 0.6, 1.0) : c;             // deep blue
    return c;
}

// One jelly in its own frame: q in bell widths, the rim's centre at the origin, +y down.
// Returns (light of the edges and threads, light of the soft body).
float2 jelly(float2 q, float t, float seed, float lw)
{
    float beat = 0.5 + 0.5 * sin(t * 1.3 + seed * 6.28);    // 1 = open, 0 = squeezed
    // The bell: a dome whose lower part draws in as it squeezes.
    float pinch = 1.0 - 0.2 * (1.0 - beat) * smoothstep(-0.35, 0.0, q.y);
    float w = 0.5 * (0.9 + 0.1 * beat) * pinch;
    float h = 0.7 * (1.06 - 0.06 * beat);
    float2 e = q / float2(w, h);
    float r = length(e);
    float edge = abs(r - 1.0) * min(w, h);
    float top = smoothstep(0.03, 0.0, q.y);
    float inside = top * step(r, 1.0);
    float rim = top * saturate(1.0 - edge / lw);
    float bloom = smoothstep(0.12, -0.04, q.y) * exp(-edge / 0.09) * 0.3;
    float ang = atan2(e.x, -e.y);
    // Four glowing horseshoes inside the bell, and faint ribs out to the rim.
    float2 g = float2(e.x, e.y + 0.45) ;
    float ga = atan2(g.x, g.y);
    float gr = length(g / float2(1.0, 0.7)) - 0.27;
    float gonads = inside * saturate(1.0 - abs(gr) / (2.0 * lw)) * 0.6 * (0.5 + 0.5 * cos(ga * 4.0 + 3.14));
    float ribs = inside * pow(abs(cos(ang * 8.0)), 30.0) * 0.14 * r;
    float glowIn = inside * (0.05 + 0.16 * (1.0 - r) * (1.0 - r));
    float margin = saturate(1.0 - abs(q.y - 0.012 * sin(q.x * 45.0 + t)) / lw) * step(abs(q.x), w) * 0.6;

    // Two frilled arms from the middle, and fine tentacles from the rim, swinging in a
    // wave that runs down them, with sparks of light travelling to their ends.
    float s = q.y;
    float below = step(0.0, s);
    float threads = 0;
    [unroll]
    for (int m = 0; m < 2; m++)
    {
        float side = m * 2.0 - 1.0;
        float len = 1.1;
        float along = saturate(s / len);
        float x = side * 0.06 * (1.0 + s) + 0.08 * s * sin(2.5 * s - 1.1 * t + side);
        float frill = (0.035 + 0.03 * sin(s * 30.0 - t * 2.0)) * (1.0 - along);
        float d = abs(abs(q.x - x) - frill * 0.5);
        threads += below * (1.0 - along) * saturate(1.0 - d / lw) * 0.8;
    }
    [unroll]
    for (int k = 0; k < TENTACLES; k++)
    {
        float fk = k;
        float x0 = (fk / (TENTACLES - 1.0) * 1.7 - 0.85) * w;
        float len = 1.8 + 1.6 * hash11(seed * 13.0 + fk);
        float sway = 0.12 * s * sin(3.0 * s - 1.6 * t + fk * 1.7 + seed * 5.0);
        float x = x0 * (1.0 - 0.3 * saturate(s / len)) + sway;
        float d = abs(q.x - x);
        float along = saturate(s / len);
        float spark = pow(0.5 + 0.5 * cos(s * 7.0 - t * 2.2 + fk * 2.3), 20.0);
        threads += below * (1.0 - along) * (1.0 - along) * saturate(1.0 - d / (0.6 * lw)) * (0.4 + 1.6 * spark);
    }
    return float2(rim + margin + gonads + ribs + threads, glowIn + bloom);
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float t = time * current;
    float3 col = 0;

    [unroll]
    for (int i = 0; i < JELLIES; i++)
    {
        float fi = i;
        float a = hash11(fi + 1.7);
        float b = hash11(fi + 4.1);
        float c = hash11(fi + 8.3);
        float size = 0.07 + 0.12 * a;                  // bell width in screen heights
        float rise = 0.008 + 0.014 * a;                     // the near ones pass faster
        // Up the screen, then round again from below; the trip covers the tentacles too.
        float trip = 1.0 + 4.6 * size;
        float y = 0.5 + 3.8 * size - frac(hash11(fi * 3.3 + 0.4) + t * rise / trip) * trip;
        float lane = ((fi + 0.5 + 0.6 * (c - 0.5)) / JELLIES - 0.5) * (aspect - 0.3);
        float x = lane + 0.07 * sin(t * 0.06 + 6.28 * b);
        float tilt = 0.2 * sin(t * 0.09 + 6.28 * a);
        float2 d = uv - float2(x, y);
        float cs = cos(tilt), sn = sin(tilt);
        float2 q = float2(cs * d.x + sn * d.y, -sn * d.x + cs * d.y) / size;
        float lw = 1.3 * pix / size;
        float depth = 0.5 + 0.5 * a;                        // big ones are near and brighter
        float2 l = jelly(q, t + 9.0 * b, fi + b, lw);
        float3 tint = jellyColour(fi);
        col += (tint * l.x + tint * tint * l.y) * depth;
    }

    // Marine snow: a speck in some cells, sinking.
    float2 sp = uv * 26.0 + float2(0.0, -t * 0.35);
    float2 id = floor(sp);
    float2 f = frac(sp) - 0.5;
    float h = hash11(id.x * 17.0 + id.y * 131.0);
    float2 at = (float2(hash11(h * 31.0), hash11(h * 57.0)) - 0.5) * 0.6;
    float speck = step(0.7, h) * saturate(1.5 - length(f - at) / (1.6 * pix * 26.0));
    col += float3(0.5, 0.65, 0.8) * speck * 0.16;

    return col * glow;
}
