// Ginkgo in gold: a ginkgo twig drawn in fine gold lines on deep green, as a botanical
// plate is. Each fan-shaped leaf has a wavy rim, the notch in its middle and the fine
// veins that fan out from its stalk; three leaves have already let go and drift down.

uint pcg(uint v)
{
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

float grain01(float2 px)
{
    return float(pcg(uint(px.x) + pcg(uint(px.y)))) * (1.0 / 4294967295.0);
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

static const float DEG = 0.0174533;

// One leaf. Returns (ink of its lines, how much it is inside the blade).
float2 leaf(float2 p, float2 base, float dir_deg, float size, float w, float pix)
{
    float a0 = dir_deg * DEG;
    float2 u = float2(cos(a0), sin(a0));
    float2 v = float2(u.y, -u.x);
    float2 d = p - base;
    float2 q = float2(dot(d, v), dot(d, u)) / size;
    float lw = w / size;
    float lp = pix / size;

    // The stalk, gently bowed.
    float2 sq = q - float2(0.06 * q.y * q.y, 0.0);
    float stalk = length(float2(sq.x, sq.y - clamp(sq.y, 0.0, 0.52)));

    // The fan: polar coordinates around the top of the stalk.
    float2 b = q - float2(0.0, 0.50);
    float r = length(b);
    float a = atan2(b.x, b.y);
    float th = 1.02;
    float notch = 0.30 * exp(-a * a / 0.0035);
    float rim = 1.0 * (0.955 + 0.045 * cos(a * 11.0)) * (1.0 - 0.06 * abs(a)) - notch;
    // The fan opens from a narrow wedge at the stalk, its sides gently hollow.
    float side = th * (0.45 + 0.55 * smoothstep(0.0, 0.75, r));
    // The rim's distance, corrected for how fast the rim turns (the notch is steep).
    float drim = 2.0 * a / 0.0035 * notch - 0.045 * 11.0 * sin(a * 11.0);
    float g = drim / max(r, 0.05);
    float sd = max((r - rim) / sqrt(1.0 + g * g), (abs(a) - side) * r);
    float outline = 1.0 - smoothstep(lw - lp, lw + lp, abs(sd));
    float inside = 1.0 - smoothstep(-lp, lp, sd);

    // Veins fan out from the stalk; they thin where they crowd near its top.
    float m = 26.0;
    float va = (a + side) / (2.0 * side) * m;
    float vd = abs(frac(va + 0.5) - 0.5) * (2.0 * side / m) * r;
    float vw = lw * 0.45;
    float vein = (1.0 - smoothstep(vw - lp, vw + lp, vd)) * smoothstep(0.12, 0.30, r) * inside;
    // The cleft runs down from the notch.
    float cleft = (1.0 - smoothstep(lw * 0.8 - lp, lw * 0.8 + lp, abs(b.x))) * step(0.38, b.y) * step(b.y, rim + 0.3);

    float ink = max(outline, max(vein * 0.55, cleft * 0.8 * inside));
    ink = max(ink, 1.0 - smoothstep(lw * 1.1 - lp, lw * 1.1 + lp, stalk));
    return float2(ink, inside);
}

float3 wall(float2 px)
{
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float2 p = float2(px.x, resolution.y - px.y) / resolution.y;
    float w = 0.00085 * stroke;

    // Deep green ground with a soft light from the upper right.
    float2 lq = p - float2(aspect - 0.25, 0.85);
    float3 c = lerp(float3(0.030, 0.085, 0.072), float3(0.070, 0.165, 0.135), exp(-dot(lq, lq) * 1.6));

    // The twig: an arc from the top edge sweeping down to the left.
    float2 cen = float2(aspect - 0.20, 0.10);
    float R = 0.90;
    float ap = atan2(p.y - cen.y, p.x - cen.x);
    float ac = clamp(ap, 93.0 * DEG, 148.0 * DEG);
    float2 on = cen + R * float2(cos(ac), sin(ac));
    float twig = 1.0 - smoothstep(w * 1.8 - pix, w * 1.8 + pix, length(p - on));
    float ink = 0.0;
    float fill = 0.0;

    // Leaves on the twig: (angle on the arc, direction, size).
    static const float3 LEAVES[7] = {
        float3(100.0, 150.0, 0.125), float3(106.0, -95.0, 0.150), float3(114.0, 172.0, 0.118),
        float3(121.0, -122.0, 0.158), float3(130.0, 138.0, 0.108), float3(137.0, -150.0, 0.132),
        float3(147.0, 205.0, 0.140)
    };
    [unroll] for (int i = 0; i < 7; i++)
    {
        float2 at = cen + R * float2(cos(LEAVES[i].x * DEG), sin(LEAVES[i].x * DEG));
        float2 l = leaf(p, at, LEAVES[i].y, LEAVES[i].z, w, pix);
        ink = max(ink, l.x);
        fill = max(fill, l.y);
    }
    // Leaves that already fell.
    float2 f1 = leaf(p, float2(aspect - 0.70, 0.34), 232.0, 0.105, w, pix);
    float2 f2 = leaf(p, float2(aspect - 0.36, 0.20), -48.0, 0.090, w, pix);
    float2 f3 = leaf(p, float2(aspect - 1.02, 0.13), 118.0, 0.072, w, pix);
    ink = max(ink, max(f1.x, max(f2.x, f3.x)));
    fill = max(fill, max(f1.y, max(f2.y, f3.y)));

    // The twig passes behind the leaves.
    ink = max(ink, twig * (1.0 - fill));
    float3 gold = lerp(float3(0.93, 0.80, 0.55), float3(0.78, 0.62, 0.36), saturate(1.0 - p.y));
    c = lerp(c, gold, fill * 0.07);
    c = lerp(c, gold, ink * 0.92);

    float2 vg = px / resolution - 0.5;
    c *= 1.0 - 0.25 * dot(vg, vg);
    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
