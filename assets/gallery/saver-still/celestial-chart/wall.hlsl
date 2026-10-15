// Celestial chart: an old star map drawn in light. A graduated ring, the faint grid of
// the sky, a dashed ecliptic, four invented constellations and a scatter of stars.

static const float TAU = 6.2831853;

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float stroke(float d, float w, float aa)
{
    return smoothstep(w + aa, max(w - aa, 0.0), d) * saturate(w / aa + 0.35);
}

static const float2 STARS[26] = {
    float2(-0.15, 0.25), float2(-0.15, 0.165), float2(-0.152, 0.06), float2(-0.235, 0.15), float2(-0.065, 0.175),
    float2(-0.29, 0.105), float2(-0.012, 0.215),
    float2(0.03, 0.27), float2(0.075, 0.205), float2(0.125, 0.245), float2(0.17, 0.185), float2(0.235, 0.235),
    float2(0.06, -0.01), float2(0.2, -0.025), float2(0.105, -0.115), float2(0.125, -0.125), float2(0.145, -0.135),
    float2(0.07, -0.235), float2(0.215, -0.225),
    float2(-0.31, -0.06), float2(-0.255, -0.085), float2(-0.2, -0.095), float2(-0.155, -0.13), float2(-0.16, -0.205),
    float2(-0.075, -0.215), float2(-0.07, -0.14)
};

// How bright each star is (1 = brightest).
static const float MAG[26] = {
    0.7, 1.0, 0.5, 0.6, 0.55, 0.4, 0.45,
    0.6, 0.75, 0.9, 0.6, 0.55,
    0.8, 1.0, 0.7, 0.75, 0.7, 0.6, 0.95,
    0.5, 0.55, 0.6, 0.65, 0.6, 0.8, 0.7
};

static const int2 SEGS[23] = {
    int2(0, 1), int2(1, 2), int2(3, 1), int2(1, 4), int2(3, 5), int2(4, 6),
    int2(7, 8), int2(8, 9), int2(9, 10), int2(10, 11),
    int2(12, 14), int2(13, 16), int2(14, 15), int2(15, 16), int2(14, 17), int2(16, 18),
    int2(19, 20), int2(20, 21), int2(21, 22), int2(22, 23), int2(23, 24), int2(24, 25), int2(25, 22)
};

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 lineCol = float3(0.42, 0.58, 0.85);
    float3 gold = float3(0.95, 0.75, 0.42);
    float3 starCol = float3(1.0, 0.97, 0.9);
    float3 col = 0.0;

    float r = length(p);
    float ang = atan2(p.y, p.x);
    float lw = 0.75 * pxs;

    // The graduated ring: two circles with degree ticks between them, and an outer hairline.
    float rA = 0.385;
    float rB = 0.405;
    float rC = 0.425;
    col += lineCol * 0.55 * (stroke(abs(r - rA), lw, pxs) + stroke(abs(r - rB), lw, pxs));
    col += lineCol * 0.35 * stroke(abs(r - rC), lw, pxs);
    float deg = ang / TAU * 360.0;
    float td = abs(deg - round(deg)) * TAU * r / 360.0;
    float t5 = abs(deg / 5.0 - round(deg / 5.0)) * 5.0 * TAU * r / 360.0;
    float t10 = abs(deg / 10.0 - round(deg / 10.0)) * 10.0 * TAU * r / 360.0;
    float band = step(rA, r) * step(r, rB);
    float tick = stroke(td, lw * 0.8, pxs) * step(r, rA + 0.006);
    tick = max(tick, stroke(t5, lw, pxs) * step(r, rA + 0.011));
    tick = max(tick, stroke(t10, lw, pxs));
    col += lineCol * 0.5 * tick * band;
    // hour marks on the outer band: small dots every 15 degrees
    float hrs = ang / TAU * 24.0;
    float2 hp = float2((hrs - round(hrs)) * TAU * r / 24.0, r - (rB + rC) * 0.5);
    col += lineCol * 0.6 * stroke(length(hp), 0.0018, pxs);

    // The grid of the sky: circles of declination and hour lines, dashed and faint.
    float inner = step(r, rA);
    float dash = step(0.5, frac(ang / TAU * 180.0));
    [unroll]
    for (int k = 1; k <= 3; k++)
    {
        col += lineCol * 0.22 * grid * stroke(abs(r - rA * float(k) / 4.0), lw * 0.8, pxs) * dash;
    }
    float hourA = ang / TAU * 12.0;
    float dl = abs(hourA - round(hourA)) * TAU * r / 12.0;
    float rdash = step(0.45, frac(r * 90.0));
    col += lineCol * 0.18 * grid * stroke(dl, lw * 0.8, pxs) * inner * rdash * step(0.03, r);

    // The ecliptic: an off-centre circle, dashed in gold.
    float2 ec = float2(0.06, -0.045);
    float re = 0.3;
    float2 eq = p - ec;
    float ea = atan2(eq.y, eq.x);
    float edash = step(0.4, frac(ea / TAU * 120.0));
    col += gold * 0.42 * stroke(abs(length(eq) - re), lw, pxs) * edash * inner;

    // Constellation lines, stopping short of the stars.
    float cl = 0.0;
    [unroll]
    for (int s = 0; s < 23; s++)
    {
        float2 a = STARS[SEGS[s].x];
        float2 b = STARS[SEGS[s].y];
        float2 ab = b - a;
        float len = length(ab);
        float h = dot(p - a, ab) / (len * len);
        float gap = 0.011 / len;
        float hc = clamp(h, gap, 1.0 - gap);
        float d = length(p - a - ab * hc);
        cl = max(cl, stroke(d, lw * 0.9, pxs));
    }
    col += starCol * 0.42 * cl;

    // The named stars of the figures.
    [unroll]
    for (int i = 0; i < 26; i++)
    {
        float m = MAG[i];
        float d = length(p - STARS[i]);
        float sz = 0.0012 + 0.0032 * m * m;
        float3 tint = i == 13 ? float3(1.0, 0.55, 0.38) : (i == 18 || i == 9 ? float3(0.75, 0.85, 1.0) : starCol);
        col += tint * (stroke(d, sz, pxs) * (0.55 + 0.45 * m) + 0.12 * m * exp(-d / 0.004));
    }

    // Field stars, inside the chart only, crowding along a faint band of the galaxy.
    float milky = exp(-pow(length(p - float2(-0.62, 0.7)) - 0.86, 2.0) / 0.004);
    float2 g = p * 42.0;
    float2 cell = floor(g);
    float2 off = float2(hash21(cell), hash21(cell + 17.0)) * 0.7 + 0.15;
    float sd = length(g - cell - off) / 42.0;
    float mag = hash21(cell + 5.0);
    float sz = 0.0006 + 0.0012 * mag * mag;
    float chart = step(r, rA - 0.01);
    col += starCol * step(hash21(cell + 41.0), (0.3 + 0.3 * milky) * stars) * chart * stroke(sd, sz, pxs) * (0.25 + 0.5 * mag);
    float2 g2 = p * 110.0;
    float2 cell2 = floor(g2);
    float2 off2 = float2(hash21(cell2 + 3.0), hash21(cell2 + 29.0)) * 0.7 + 0.15;
    float sd2 = length(g2 - cell2 - off2) / 110.0;
    col += float3(0.8, 0.85, 1.0) * step(hash21(cell2 + 13.0), 0.5 * milky * stars) * chart * stroke(sd2, 0.0005, pxs) * (0.2 + 0.3 * hash21(cell2 + 7.0));

    return max(col - 0.002, 0.0);
}
