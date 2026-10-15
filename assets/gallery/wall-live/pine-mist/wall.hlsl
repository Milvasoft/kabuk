// Pine mist: ridge after ridge of pine forest fading into the dawn haze,
// while fog drifts slowly through the valleys between them. A pale sun
// sits behind the haze right of centre; the left stays soft.

float pm_hash(float n)
{
    return frac(sin(n) * 43758.5453);
}

// Smooth 2D value noise.
float pm_noise2(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    float n = i.x + i.y * 57.0;
    return lerp(lerp(pm_hash(n), pm_hash(n + 1.0), f.x),
                lerp(pm_hash(n + 57.0), pm_hash(n + 58.0), f.x), f.y);
}

// The skyline of ridge k (0 = farthest) at x, as a height from the top:
// broad hills from a few incommensurate waves, with smaller bumps on them.
float pm_ridge(float x, float k)
{
    float base = 0.49 + 0.115 * k;
    float f = 1.0 - 0.12 * k;
    float m = 0.5 + 0.5 * sin(x * 1.9 * f + k * 2.31);
    float h = 0.17 * m * m * (0.65 + 0.35 * sin(x * 0.73 * f + k * 4.1))
            + 0.030 * sin(x * 4.3 * f + k * 1.3) * (0.6 + 0.4 * sin(x * 1.1 + k))
            + 0.013 * sin(x * 9.1 + k * 3.7);
    return base - h * (1.0 - 0.10 * k);
}

// How much of (x, y) a row of pines on ridge k covers; r is the ground.
float pm_pines(float x, float y, float r, float k, float aa)
{
    float sp = 0.0060 * (1.0 + 0.7 * k);
    float isp = 1.0 / sp;
    // Only the two nearest trees can reach this pixel.
    float c = floor(x * isp - 0.5);
    float cover = 0.0;
    [unroll] for (int j = 0; j <= 1; j++)
    {
        float cc = c + j;
        float h = pm_hash(cc * 12.989 + k * 78.233);
        float cx = cc + 0.5 + (h - 0.5) * 0.4;
        float g = frac(h * 13.1);
        float tall = (2.2 + 3.6 * g * g) * step(0.12, frac(h * 7.31));
        // Work in tree spacings: d is the depth below the tree's tip.
        float d = (y - r) * isp + tall;
        // Tiers of branches: each tier widens downward, then steps in.
        float w = d * (0.11 + 0.08 * frac(d * 0.85 + h));
        cover = max(cover, saturate((w - abs(x * isp - cx)) * sp / aa + 0.5) * step(0.0, d));
    }
    return cover;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.02;
    float aa = 1.2 / resolution.y;
    float sx = uv.x / aspect;

    // Dawn palette; warmth turns a cool blue morning into a rose dawn.
    float3 skyTop = lerp(float3(0.10, 0.19, 0.36), float3(0.15, 0.16, 0.38), warmth);
    float3 skyLow = lerp(float3(0.62, 0.74, 0.82), float3(0.96, 0.70, 0.56), warmth);
    float3 fogCol = lerp(float3(0.58, 0.68, 0.76), float3(0.80, 0.64, 0.62), warmth);
    float3 farCol = lerp(float3(0.32, 0.45, 0.62), float3(0.44, 0.40, 0.62), warmth);
    float3 nearCol = lerp(float3(0.015, 0.050, 0.055), float3(0.030, 0.045, 0.060), warmth);
    float3 sunCol = lerp(float3(1.0, 0.96, 0.88), float3(1.0, 0.80, 0.58), warmth);

    // Sky with a soft sun low behind the haze.
    float3 col = lerp(skyTop, skyLow, smoothstep(0.0, 0.55, uv.y));
    float2 sun = float2(aspect * 0.66, 0.30);
    float sd = length(uv - sun);
    float glow = exp(-sd * 4.0);
    col += sunCol * (0.28 * glow + 0.10 * exp(-sd * 1.2) + 0.16 * smoothstep(0.034, 0.030, sd));

    // Two slow wisp fields, shared by the odd and the even valleys.
    float wispA = pm_noise2(float2(uv.x * 2.4 - t * 1.6, uv.y * 11.0));
    float wispB = pm_noise2(float2(uv.x * 2.0 - t * 2.6 + 9.1, uv.y * 9.0 + 3.0));

    // Ridges toward the sun are lit a little through the haze, and so is
    // the fog near it.
    float sunSide = exp(-abs(uv.x - sun.x) * 1.2);
    float3 fc = fogCol + sunCol * 0.18 * glow;

    // Five ridges from far to near; each fades toward the haze with
    // distance and sinks into valley fog at its foot.
    [unroll] for (int i = 0; i < 5; i++)
    {
        float k = i;
        float far = 1.0 - k / 4.0;
        float r = pm_ridge(uv.x, k);
        float land;
        if (i < 2)
        {
            // Far away the pines are only a toothed edge along the ridge.
            float sp = 0.0060 * (1.0 + 0.7 * k);
            float tooth = 1.0 - abs(frac(uv.x / sp) * 2.0 - 1.0);
            // Uneven heights: two waves that do not share the trees' rhythm.
            float vary = (0.5 + 0.5 * sin(uv.x * 251.0 + k * 5.0)) * (0.55 + 0.45 * sin(uv.x * 37.0 + k));
            float tall = sp * (1.2 + 3.0 * vary);
            float edge = r - tall * tooth * sqrt(tooth);
            land = smoothstep(edge - aa, edge + aa, uv.y);
        }
        else
        {
            land = smoothstep(r - aa, r + aa, uv.y);
            if (uv.y > r - 0.0060 * (1.0 + 0.7 * k) * 5.8 && uv.y < r + aa)
                land = max(land, pm_pines(uv.x, uv.y, r, k, aa * 2.0));
        }
        float3 tone = lerp(nearCol, farCol, pow(far, 1.4));
        tone = lerp(tone, skyLow, far * far * 0.35);
        tone += sunCol * 0.10 * far * sunSide;
        col = lerp(col, tone, land);

        // Valley fog: thick low down, torn into slow drifting wisps.
        float below = uv.y - r;
        float wisp = (i & 1) ? wispB : wispA;
        float fog = smoothstep(-0.02, 0.09 + 0.02 * k, below) * (0.35 + 0.65 * wisp) * mist * (0.75 - 0.15 * k / 4.0);
        col = lerp(col, fc, saturate(fog));
    }

    // Soft haze over the icon side.
    col = lerp(col, fogCol * 0.9, 0.22 * (1.0 - smoothstep(0.0, 0.25, sx)));
    return col;
}
