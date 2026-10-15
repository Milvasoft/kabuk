// Cloud sea: dawn seen from a summit above the clouds. Rows of soft cloud
// tops roll slowly sideways, their crowns lit gold by the low sun, while a
// few far peaks rise through them on the right. The left stays softer.

float cs_hash(float n)
{
    return frac(sin(n) * 43758.5453);
}

// Smooth 1D value noise.
float cs_noise(float x)
{
    float i = floor(x);
    float f = frac(x);
    f = f * f * (3.0 - 2.0 * f);
    return lerp(cs_hash(i), cs_hash(i + 1.0), f);
}

// One row of round cloud puffs of random size along u.
// Returns (height of the puff over u, place across it from -1 to 1).
float2 cs_puff(float u, float seed)
{
    float c = floor(u);
    float f = frac(u) * 2.0 - 1.0;
    float r = 0.35 + 0.65 * cs_hash(c * 7.31 + seed);
    return float2(sqrt(saturate(1.0 - f * f)) * r, f);
}

// Height of the far peaks above the horizon at x (below 0 where there is
// none): three peaks with concave slopes; detail adds broken edges.
float cs_peaks(float x, float aspect, float detail)
{
    float3 v = saturate(1.0 - abs(x - aspect * float3(0.80, 0.90, 0.585)) / float3(0.20, 0.11, 0.085));
    v = v * sqrt(v) * float3(0.130, 0.085, 0.050);
    float h = max(max(v.x, v.y), v.z);
    h += detail * (0.010 * sin(x * 37.0) * sin(x * 13.0 + 1.0) + 0.004 * sin(x * 83.0 + 2.0)) * saturate(h * 30.0);
    return h - 0.004;
}

// The centre of the peak that stands highest over x.
float cs_summit(float x, float aspect)
{
    float3 c = aspect * float3(0.80, 0.90, 0.585);
    float3 v = saturate(1.0 - abs(x - c) / float3(0.20, 0.11, 0.085));
    v = v * sqrt(v) * float3(0.130, 0.085, 0.050);
    return v.x >= max(v.y, v.z) ? c.x : (v.y >= v.z ? c.y : c.z);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.01;
    float aa = 1.5 / resolution.y;
    float horizon = 0.47;
    float sx = uv.x / aspect;

    float3 sunCol = lerp(float3(1.00, 0.86, 0.68), float3(1.00, 0.64, 0.38), warmth);
    float3 sunSky = lerp(float3(0.96, 0.80, 0.68), float3(1.00, 0.60, 0.40), warmth);
    float3 midSky = lerp(float3(0.42, 0.48, 0.68), float3(0.52, 0.40, 0.60), warmth);
    float3 topSky = lerp(float3(0.06, 0.11, 0.26), float3(0.08, 0.08, 0.22), warmth);
    float3 shade = lerp(float3(0.24, 0.28, 0.46), float3(0.30, 0.27, 0.48), warmth);
    float3 crown = lerp(float3(0.96, 0.92, 0.92), float3(1.00, 0.80, 0.66), warmth);
    float2 sun = float2(aspect * 0.70, horizon - 0.010);
    float side = exp(-abs(uv.x - sun.x) * 1.1);

    // Sky: deep blue overhead, warming toward the sun's side of the horizon.
    float h = horizon - uv.y;
    float3 col = lerp(midSky, topSky, smoothstep(0.02, 0.45, h));
    col = lerp(col, sunSky, exp(-max(h, 0.0) * 9.0) * (0.45 + 0.55 * side));
    float sd = length(uv - sun);
    col += sunCol * (0.35 * exp(-sd * 9.0) + 0.55 * smoothstep(0.032, 0.028, sd));

    // Far peaks: blue on the shadow side, alpenglow on the sun side.
    float ph = cs_peaks(uv.x, aspect, 1.0);
    float peak = (1.0 - smoothstep(ph - aa, ph + aa, h)) * smoothstep(-aa, aa, h + 0.02);
    // The line between lit and shaded faces leans and wanders downhill.
    float xs = uv.x + (ph - h) * 0.45 + 0.004 * sin(h * 140.0);
    // A face is lit when it lies on the summit's sun side.
    float top = cs_summit(xs, aspect);
    float face = smoothstep(0.0, 0.0012, (top - xs) * sign(top - sun.x));
    float3 rock = lerp(float3(0.17, 0.17, 0.32), float3(0.86, 0.60, 0.62) * sunCol, face * (0.45 + 0.25 * saturate(1.0 - (ph - h) * 12.0)));
    rock = lerp(rock, sunSky * 0.85 + midSky * 0.15, exp(-max(h, 0.0) * 45.0) * 0.9);
    col = lerp(col, rock, peak);

    float3 haze = sunSky * 0.85 + midSky * 0.2;
    float toRight = sign(uv.x - sun.x);
    // Softer toward the icons on the left.
    float soft = 0.6 + 0.4 * smoothstep(0.0, 0.35, sx);
    float halfInvAa = 0.5 / aa;

    // Rows of cloud crowns from the horizon to the bottom edge, far first.
    [unroll] for (int i = 0; i < 5; i++)
    {
        float s = (i + 1.0) / 5.0;
        float base = horizon + 0.006 + 0.58 * s * s;
        float wide = 0.014 + 0.32 * s * s;
        float tall = wide * (0.58 + 0.06 * sin(t * 3.0 + i * 1.7)) * billow;
        float drift = t * (0.6 + 1.2 * s) * ((i & 1) ? 1.0 : -0.7);
        float u = uv.x * (1.0 / wide) + drift + i * 3.1;
        float swell = 0.35 + 0.85 * cs_noise(u * 0.19 + i * 5.0);
        // Heights inside the row are measured in puff heights from its base.
        float yt = (uv.y - base) / tall;
        float sharp = tall * halfInvAa;
        // The body darkens from a smooth line below the crowns.
        float fall = 1.0 - saturate((yt + swell * 0.55) * 0.45) * 0.6;
        float bodyLit = fall * fall * 0.85;
        float lit = bodyLit;
        float gold = 0.0;
        float rowCover = 0.0;
        // Two staggered layers of puffs, the front one a little lower, so
        // each front puff shows its round outline against the one behind.
        // The farthest row is too small to need the back layer.
        [unroll] for (int j = (i < 1 ? 1 : 0); j < 2; j++)
        {
            float2 pf = cs_puff(u + j * 0.5, i * 11.0 + j * 3.7);
            float over = yt - 0.10 * j + pf.x * swell;
            float cover = saturate(over * sharp + 0.5);
            float rim = saturate(1.0 - over * 2.5);
            rim *= rim;
            // Each crown is brighter on the side facing the sun.
            float crownLit = saturate(1.0 - 0.35 * pf.y * pf.y - 0.15 * pf.y * toRight);
            lit = lerp(lit, lerp(bodyLit, crownLit, rim * 0.8), cover);
            gold = lerp(gold, rim, cover);
            rowCover = max(rowCover, cover);
        }
        lit = lerp(0.35, lit, soft);
        float3 c = lerp(shade, crown, lit) + sunCol * (0.35 * side * (1.0 - s * 0.6) * soft) * gold;
        // Far rows melt into the bright haze along the horizon.
        c = lerp(c, haze, (1.0 - s) * (1.0 - s) * 0.75);
        col = lerp(col, c, rowCover);
    }
    return col;
}
