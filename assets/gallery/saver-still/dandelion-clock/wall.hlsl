// Dandelion clock: a seed head drawn in fine white light on a slender stem, a gap in
// it where the wind has been, and a few seeds already sailing away.

static const float PI = 3.14159265;
static const float TAU = 6.2831853;

float hash11(float n)
{
    return frac(sin(n * 127.1) * 43758.5453);
}

float stroke(float d, float w, float aa)
{
    return smoothstep(w + aa, max(w - aa, 0.0), d) * saturate(w / aa + 0.35);
}

float segDist(float2 p, float2 a, float2 b)
{
    float2 ab = b - a;
    float h = saturate(dot(p - a, ab) / dot(ab, ab));
    return length(p - a - ab * h);
}

// One seed: a grain at `base`, a hair-thin beak along `u`, and the parachute of
// filaments at its tip.
float seed(float2 p, float2 base, float2 u, float len, float rp, float pxs)
{
    float2 tip = base + u * len;
    float k = 0.0;
    k += 0.5 * stroke(segDist(p, base, base + u * len * 0.14), 0.0009, pxs);
    k += 0.5 * stroke(segDist(p, base + u * len * 0.16, tip), 0.0005, pxs);
    float2 v = p - tip;
    float rho = length(v);
    float a = atan2(u.x * v.y - u.y * v.x, dot(u, v));
    float g = a / (PI * 0.8);
    float fil = abs(frac(g * 8.0 + 0.5) - 0.5) / 8.0 * PI * 0.8 * rho;
    float reach = rp * (0.7 + 0.3 * cos(g * 1.3));
    float inFan = step(abs(g), 1.0) * smoothstep(reach, reach * 0.7, rho);
    k += 0.34 * stroke(fil, 0.0004, pxs) * inFan * smoothstep(0.0, rp * 0.12, rho);
    // a soft sheen where the filaments crowd together near the tip
    k += 0.05 * step(abs(g), 1.0) * smoothstep(reach, 0.0, rho);
    k += 0.45 * stroke(rho, 0.001, pxs);
    return k;
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 white = float3(0.95, 0.96, 0.92);
    float3 col = 0.0;

    float2 H = float2(-0.2, 0.09);
    float Lmax = 0.17;

    // The head, in two layers (behind, then in front), each a ring of angular
    // sectors with one seed per sector; only the nearest few sectors are drawn.
    float2 v = p - H;
    float ang = atan2(v.y, v.x);
    float k = 0.0;
    [unroll]
    for (int layer = 0; layer < 2; layer++)
    {
        float fl = float(layer);
        float n = layer == 0 ? 34.0 : 46.0;
        float sector = ang / TAU * n;
        float c0 = floor(sector);
        [unroll]
        for (int j = -2; j <= 2; j++)
        {
            float c = c0 + float(j);
            float idx = c - n * floor(c / n);
            float h1 = hash11(idx + fl * 91.0);
            float h2 = hash11(idx * 3.1 + fl * 17.0);
            float a = (idx + 0.2 + 0.6 * h1) / n * TAU;
            float2 u = float2(cos(a), sin(a));
            // seeds near the line of sight look shorter
            float depth = layer == 0 ? 0.5 + 0.25 * h2 : 0.78 + 0.22 * h2;
            float len = Lmax * depth;
            // the wind has taken seeds from the upper right
            float gone = step(0.0, cos(a - 0.5)) * step(hash11(idx * 7.7 + fl), wind * 0.9 * cos(a - 0.5));
            float bright = layer == 0 ? 0.45 : 1.0;
            k += seed(p, H + u * 0.012, u, len, 0.035 * depth, pxs) * bright * (1.0 - gone);
        }
    }
    col += white * k;

    // The receptacle: a small bulb, finely dotted.
    float rr = length(v);
    float dots = stroke(length(frac(v * 520.0) - 0.5) / 520.0, 0.0006, pxs);
    col += white * smoothstep(0.0135, 0.011, rr) * (0.12 + 0.35 * dots);

    // The stem: a slender hollow stalk, curving down, two outlines fading into the dark.
    float sy = H.y - 0.012 - p.y;
    float st = sy / 0.5;
    float sx = H.x + 0.08 * st * st - 0.015 * st;
    float sdx = (0.16 * st - 0.015) / 0.5;
    float sd = (p.x - sx) / sqrt(1.0 + sdx * sdx);
    float sw = 0.0026 + 0.0012 * st;
    float alive = step(0.0, sy) * smoothstep(0.92, 0.6, st);
    col += white * 0.5 * alive * (stroke(abs(sd - sw), 0.0006, pxs) + stroke(abs(sd + sw), 0.0006, pxs));
    col += white * 0.05 * alive * smoothstep(sw, 0.0, abs(sd));
    // bracts curling down from below the head
    [unroll]
    for (int b = 0; b < 3; b++)
    {
        float fb = float(b) - 1.0;
        float2 o = H + float2(fb * 0.006, -0.012);
        float2 q = p - o;
        float t = saturate(-q.y / 0.05);
        float bx = fb * (0.006 + 0.03 * t * t) + fb * 0.004;
        float bd = abs(q.x - bx) / sqrt(1.0 + pow(fb * 0.06 * t / 0.05 * 20.0, 2.0) * 0.0);
        col += white * 0.35 * step(0.0, -q.y) * step(t, 0.99) * stroke(bd, 0.0006 + 0.0012 * (1.0 - t), pxs) * (1.0 - t);
    }

    // Seeds already sailing off to the upper right.
    [unroll]
    for (int f = 0; f < 5; f++)
    {
        float ff = float(f);
        float show = step(ff + 0.5, wind * 6.0);
        float2 pos = float2(0.0 + ff * 0.16 + 0.05 * hash11(ff + 3.0), 0.16 + 0.12 * sin(ff * 1.9) + ff * 0.025);
        float tilt = -0.5 + 0.9 * hash11(ff + 11.0);
        float2 u = float2(sin(tilt), cos(tilt));
        float sc = 1.0 - 0.1 * ff;
        float len = 0.1 * sc;
        col += white * seed(p, pos - u * len, u, len, 0.033 * sc, pxs) * show * (0.95 - 0.1 * ff);
    }

    return max(col - 0.002, 0.0);
}
