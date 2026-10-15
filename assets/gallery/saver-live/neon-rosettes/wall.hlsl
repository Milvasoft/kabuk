// Neon rosettes: a single neon line draws a rose curve, petal after petal, holds it a
// moment and lets it fade while the next one begins somewhere else, in other colours.
// A rose is r = (c + cos(k theta)) / (1 + c) with k = n / d; a pixel finds the curve
// by trying each turn of theta that passes through its own direction, so the line
// needs no stored points and the pen's progress is simply a limit on theta.

static const float TAU = 6.2831853;
static const float PI = 3.14159265;
static const float CYCLE = 36.0;      // seconds between two rosettes
static const float DRAW = 24.0;       // seconds the pen takes to draw one
static const float FADE_FROM = 30.0;
static const float FADE_TO = 47.0;
static const int MAX_TURNS = 5;     // the largest d

// n, d and the offset c of each rosette in turn.
static const float3 CURVES[7] = {
    float3(5.0, 4.0, 0.0),
    float3(7.0, 3.0, 0.3),
    float3(8.0, 5.0, 0.0),
    float3(6.0, 5.0, 0.4),
    float3(7.0, 4.0, 0.18),
    float3(4.0, 3.0, 0.55),
    float3(7.0, 5.0, 0.0),
};

static const float3 NEON[5] = {
    float3(1.0, 0.22, 0.58),    // pink
    float3(0.25, 0.8, 1.0),     // cyan
    float3(0.62, 0.32, 1.0),    // violet
    float3(1.0, 0.62, 0.2),     // amber
    float3(0.3, 1.0, 0.6),      // mint
};

float hash11(float n)
{
    return frac(sin(n * 91.345) * 47453.5453);
}

float3 rosette(float2 uv, float cycle, float tau, float aspect, float pix)
{
    float3 curve = CURVES[(int)fmod(cycle, 7.0)];
    float n = curve.x, d = curve.y, c = curve.z;
    float k = n / d;
    float odd = fmod(n * d, 2.0) * step(c, 0.0);
    float span = (odd > 0.5 ? PI : TAU) * d;            // theta until the curve closes

    float R = 0.34;
    float side = fmod(abs(cycle), 2.0) * 2.0 - 1.0;            // left and right in turn
    float2 centre = float2(side * (0.15 + 0.35 * hash11(cycle + 0.3)) * max(aspect - 0.85, 0.0),
                           (hash11(cycle + 5.1) - 0.5) * 0.1);
    float spin = 0.012 * tau + TAU * hash11(cycle + 2.2);
    float2 q = (uv - centre) / R;
    float cs = cos(spin), sn = sin(spin);
    q = float2(cs * q.x + sn * q.y, -sn * q.x + cs * q.y);
    float rho = length(q);
    float thp = atan2(q.y, q.x);
    thp = thp < 0.0 ? thp + TAU : thp;

    float pen = span * saturate(tau / DRAW);
    float zero = acos(-c);
    float w = 0.0011 * neon + 0.5 * pix;
    float bestCore = 0.0;
    float bestGlow = 0.0;
    float bestTh = 0.0;
    [unroll]
    for (int m = -1; m < MAX_TURNS; m++)
    {
        [unroll]
        for (int b = 0; b < 2; b++)
        {
            // Through this pixel's direction (b = 0) or the opposite one, with r < 0.
            float th = frac((thp + PI * b) / TAU) * TAU + TAU * m;
            float r = (c + cos(k * th)) / (1.0 + c);
            float dr = -k * sin(k * th) / (1.0 + c);
            float e = abs(rho - (b == 0 ? r : -r)) * rsqrt(1.0 + dr * dr / max(rho * rho, 0.0025));
            // Near the centre the curve is nearly straight lines through it, where
            // cos(k theta) = -c; the distance to the nearest one is better there.
            float u = k * th;
            float u1 = zero + TAU * round((u - zero) / TAU);
            float u2 = -zero + TAU * round((u + zero) / TAU);
            float th0 = (abs(u - u1) < abs(u - u2) ? u1 : u2) / k;
            float e0 = rho * abs(sin(th - th0));
            float near = (1.0 - smoothstep(0.015, 0.045, rho)) * step(e0, e);
            e = lerp(e, e0, near);
            th = near > 0.0 ? th0 : th;
            // Drawn up to the pen, with soft ends so the glow has no seams.
            float arc = max(rho, 0.05) * R / 0.012;
            float drawn = saturate((pen - th) * arc + 0.5) * saturate((span - th) * arc + 0.5) * saturate(th * arc + 0.5);
            float age = max(pen - th, 0.0) / span * DRAW;
            float v = drawn * (0.5 + 0.5 * exp(-age / 3.0));
            float dist = e * R;                              // in screen heights
            float core = saturate((w - dist) / pix + 0.5) * v;
            float glow = exp(-dist / (0.0065 * neon)) * 0.4 * v;
            bestTh = glow > bestGlow ? th : bestTh;
            bestGlow = max(bestGlow, glow);
            bestCore = max(bestCore, core);
        }
    }

    float life = 1.0 - smoothstep(FADE_FROM, FADE_TO, tau);
    float3 a = NEON[(int)fmod(cycle * 3.0, 5.0)];
    float3 z = NEON[(int)fmod(cycle * 3.0 + 1.0 + fmod(cycle, 2.0), 5.0)];
    float3 tint = lerp(a, z, 0.5 - 0.5 * cos(bestTh / span * TAU));
    float3 col = lerp(tint, float3(1.0, 1.0, 1.0), 0.45) * bestCore + tint * bestGlow;

    // The knot at the centre, where every petal passes, once the pen has reached it.
    float knot = rho * R;
    float reached = saturate((pen - zero / k) * 4.0);
    col += lerp(tint, float3(1.0, 1.0, 1.0), 0.5) * reached * (0.8 * exp(-knot / 0.0018) + 0.25 * exp(-knot / 0.006));

    // The pen's tip, while it draws.
    float rt = (c + cos(k * pen)) / (1.0 + c);
    float2 tip = rt * float2(cos(pen), sin(pen));
    float td = length(q - tip) * R;
    float drawing = 1.0 - smoothstep(DRAW - 1.0, DRAW + 1.5, tau);
    col += float3(1.0, 0.95, 0.9) * drawing * (exp(-td / 0.003) + 0.3 * exp(-td / 0.018));
    return col * life;
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float t = time * tempo + 8.0;
    float cycle = floor(t / CYCLE);
    float tau = t - cycle * CYCLE;
    float3 now = rosette(uv, cycle, tau, aspect, pix);
    float3 before = rosette(uv, cycle - 1.0, tau + CYCLE, aspect, pix);
    return max(now, before) + min(now, before) * 0.5;
}
