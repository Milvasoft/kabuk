// Wheat wind: a wheat field at the golden hour. Slow gusts roll across it
// as pale waves, and the ears in front lean and settle as each gust passes,
// under a low sun and a lone tree on the far edge. The icon side is quiet.

float ww_hash(float n)
{
    return frac(sin(n) * 43758.5453);
}

// Smooth 2D value noise.
float ww_noise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    float n = i.x + i.y * 57.0;
    return lerp(lerp(ww_hash(n), ww_hash(n + 1.0), f.x),
                lerp(ww_hash(n + 57.0), ww_hash(n + 58.0), f.x), f.y);
}

// The wind over the field at ground point q: broad waves travelling
// across, slowly torn apart. 0 is still air, 1 a full gust.
float ww_gust(float2 q, float t)
{
    float g = sin(q.x * 0.9 + q.y * 0.5 - t * 2.4) * 0.5 + 0.5;
    g *= 0.45 + 0.55 * ww_noise(q * float2(0.5, 0.8) + float2(-t * 0.9, 0.0));
    return g * g * (3.0 - 2.0 * g);
}

// One row of wheat ears in front. Returns (cover, light across the ear,
// rim on the sun side). base is the height where the ears sit, sp the gap.
float3 ww_ears(float2 uv, float base, float sp, float seed, float t, float sunSide)
{
    float3 best = 0.0;
    // Ears lean to the right, so the two stalks that can reach this pixel
    // stand in the window just left of it.
    float c0 = floor(uv.x / sp - 1.1);
    [unroll] for (int j = 0; j <= 1; j++)
    {
        float c = c0 + j;
        float h1 = ww_hash(c * 3.17 + seed);
        float h2 = frac(h1 * 17.3);
        float x0 = (c + 0.5 + (h1 - 0.5) * 0.5) * sp;
        float len = sp * (2.4 + 0.8 * h2);
        float il = 1.0 / len;
        float2 at = float2(x0, base + (h2 - 0.5) * sp * 1.6);
        // The stalk leans with the wind where it stands.
        float w = 0.5 + 0.5 * sin(x0 * 6.3 + base * 9.0 - t * 3.8);
        float lean = 0.06 + 0.26 * w * w * gusts + (h1 - 0.5) * 0.08;
        float2 up = float2(lean, -1.0 + 0.5 * lean * lean);
        float2 r = uv - at;
        // Coordinates along (a, 0 at the stalk, 1 at the tip) and across (b).
        float a = dot(r, up) * il;
        float b = (r.x * up.y - r.y * up.x) * il * 6.67;
        float ax = abs(a * 2.0 - 1.0);
        float body = sqrt(saturate(1.0 - ax * ax * ax));
        // Grains: the outline swells a little along the ear.
        float grain = 0.93 + 0.07 * sin(a * 34.0 + h1 * 6.0);
        float ear = saturate((body * grain - abs(b)) * 4.0);
        // Below the ear the stalk carries on downward, leaning less.
        float sb = abs(r.x - r.y * up.x * 0.6);
        float stalk = saturate((sp * 0.06 - sb) / (sp * 0.03)) * step(0.0, r.y);
        float cover = max(ear, stalk * 0.85);
        if (cover > best.x)
        {
            best = float3(cover, saturate(0.5 + 0.5 * b) * ear, saturate(b * sunSide) * ear);
        }
    }
    return best;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.05;
    float aa = 1.5 / resolution.y;
    float sx = uv.x / aspect;
    float horizon = 0.46;

    float3 sunCol = lerp(float3(1.00, 0.88, 0.66), float3(1.00, 0.70, 0.40), warmth);
    float3 skyTop = lerp(float3(0.26, 0.40, 0.60), float3(0.28, 0.32, 0.52), warmth);
    float3 skyLow = lerp(float3(0.96, 0.86, 0.70), float3(1.00, 0.72, 0.48), warmth);
    float2 sun = float2(aspect * 0.74, horizon - 0.08);

    // Sky: clear blue fading into a warm haze over the field.
    float h = horizon - uv.y;
    float3 col = lerp(skyLow, skyTop, smoothstep(0.0, 0.45, h));
    // Only the sky's own pixels pay for its sun, clouds, hills and tree.
    if (h > -aa * 2.0)
    {
        float sd = length(uv - sun);
        col += sunCol * (0.28 * exp(-sd * 6.0) + 0.10 * exp(-sd * 1.5) + 0.40 * smoothstep(0.030, 0.026, sd));

        // A few long, thin clouds drifting slowly, lit from below by the sun.
        float streak = ww_noise(float2(uv.x * 1.6 - t * 0.25, uv.y * 16.0)) * (0.6 + 0.4 * sin(uv.x * 9.0 - t * 0.3 + uv.y * 40.0));
        float band = smoothstep(0.06, 0.16, h) * smoothstep(0.42, 0.24, h);
        col = lerp(col, lerp(float3(1.0, 0.80, 0.70), sunCol, 0.4) * (0.85 + 0.25 * exp(-sd * 2.0)), smoothstep(0.30, 0.65, streak) * band * 0.55);

        // Far hills and a lone tree, blued by the distance.
        float hill = horizon - 0.010 - 0.020 * (0.5 + 0.5 * sin(uv.x * 2.3 + 1.0)) * (0.6 + 0.4 * sin(uv.x * 5.1));
        float2 tc = uv - float2(aspect * 0.58, hill);
        float crown = min(min(length(tc - float2(0.000, -0.046)) - 0.024,
                              length(tc - float2(-0.020, -0.034)) - 0.017),
                          length(tc - float2(0.021, -0.036)) - 0.018);
        crown += 0.0025 * sin(tc.x * 400.0) * sin(tc.y * 380.0);
        float trunk = max(abs(tc.x + 0.001 * tc.y * 40.0) - 0.0022, abs(tc.y + 0.012) - 0.013);
        float tree = smoothstep(aa, -aa, min(crown, trunk));
        float3 far = lerp(skyLow, float3(0.34, 0.33, 0.40), 0.5);
        col = lerp(col, far, smoothstep(hill - aa, hill + aa, uv.y));
        col = lerp(col, lerp(skyLow, float3(0.22, 0.20, 0.26), 0.65), tree);
    }

    // The field below the horizon, with gusts rolling over it in perspective.
    if (h < 0.0)
    {
        float d = max(uv.y - horizon, 1e-4);
        float z = 0.10 / d;
        float2 q = float2((uv.x - aspect * 0.5) * z, z) * 7.0;
        float g = ww_gust(q, t) * gusts;
        float3 lit = lerp(float3(0.80, 0.56, 0.26), float3(0.88, 0.62, 0.30), warmth);
        float3 dark = float3(0.40, 0.25, 0.11);
        // Gentle swells in the crop, and tramlines running toward the horizon.
        float swell = 0.5 + 0.25 * sin(q.x * 0.31 + 1.0) + 0.25 * sin(q.y * 0.43 + q.x * 0.11);
        float lane = (uv.x - aspect * 0.62) / d * 0.32;
        float lfp = 0.32 / (d * resolution.y);
        float tram = smoothstep(0.03 + lfp * 1.5, 0.0, abs(abs(frac(lane) - 0.5) - 0.04)) * saturate(1.0 - lfp * 8.0);
        float3 field = lerp(dark, lit, 0.40 + 0.25 * swell - 0.10 * tram);
        // Gusts show the paler backs of the bent ears.
        field = lerp(field, float3(1.00, 0.88, 0.62), g * 0.55);
        // The low sun gilds the far field toward it.
        field += sunCol * 0.22 * exp(-abs(uv.x - sun.x) * 1.6) * exp(-d * 8.0);
        field = lerp(field, skyLow * 0.95, exp(-d * 22.0) * 0.8);
        float quiet = smoothstep(0.02, 0.30, sx);
        field = lerp(lerp(dark, lit, 0.55), field, 0.35 + 0.65 * quiet);
        col = lerp(col, field, smoothstep(horizon, horizon + aa * 2.0, uv.y));

        // Rows of ears in front, nearest last; backlit, with a bright rim.
        if (uv.y > 0.73)
        {
            float sunSide = sign(sun.x - uv.x);
            [unroll] for (int i = 0; i < 3; i++)
            {
                float k = i;
                float sp = 0.012 * (1.0 + k * 0.9);
                float base = 0.80 + 0.075 * k;
                float3 e = ww_ears(uv, base, sp, k * 13.0, t, sunSide);
                float3 earCol = lerp(float3(0.30, 0.17, 0.07), float3(0.80, 0.54, 0.22), e.y * 0.6 + 0.15);
                earCol += sunCol * 0.65 * e.z * e.z;
                earCol = lerp(field, earCol, 0.70 + 0.12 * k);
                col = lerp(col, earCol, e.x * quiet);
            }
        }
    }
    return col;
}
