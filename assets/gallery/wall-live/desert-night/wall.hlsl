// Desert night: sharp-crested dunes under the Milky Way. The moon, out of
// sight, slowly climbs and sinks, so the line between the lit and the
// shaded faces of each dune creeps along it; the stars wheel very slowly.
// The left, where the icons sit, holds only a few faint stars.

float3 dn_hash3(float2 p)
{
    float3 p3 = frac(p.xyx * float3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yxz + 33.33);
    return frac((p3.xxy + p3.yzz) * p3.zyx);
}

float dn_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// Smooth 2D value noise.
float dn_noise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    return lerp(lerp(dn_hash(i), dn_hash(i + float2(1.0, 0.0)), f.x),
                lerp(dn_hash(i + float2(0.0, 1.0)), dn_hash(i + float2(1.0, 1.0)), f.x), f.y);
}

// The crest line of dune row k at x, as a height from the top: sharp
// crests over soft hollows, the shapes leaning a little downwind.
float dn_ridge(float x, float k)
{
    float f = 2.4 + 1.1 * k;
    float s = sin(x * f + k * 2.7 + 0.8 * sin(x * f * 0.41 + k));
    float c = 1.0 - abs(s);
    return (0.60 + 0.085 * k) - (0.045 + 0.02 * k) * c * c * (0.7 + 0.3 * sin(x * 0.9 + k * 1.3));
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;
    float aa = 1.5 / resolution.y;
    float sx = uv.x / aspect;

    // Night sky, a little lighter low down.
    float3 col = lerp(float3(0.030, 0.040, 0.080), float3(0.006, 0.008, 0.022), smoothstep(0.62, 0.0, uv.y));

    // The sky; the nearest dune row hides everything below its foot.
    if (uv.y < 0.61)
    {
        // The sky wheels slowly around a pole above the top left.
        float a = t * 0.0004;
        float2 pole = float2(aspect * 0.25, -0.35);
        float2 sp = uv - pole;
        float ca = cos(a);
        float sa = sin(a);
        sp = float2(sp.x * ca - sp.y * sa, sp.x * sa + sp.y * ca);

        // The Milky Way: a soft band torn by dark dust lanes, rising on the right.
        float2 dir = normalize(float2(0.55, -1.0));
        float2 rel = sp - (float2(aspect * 0.72, 0.62) - pole);
        float across = dot(rel, float2(-dir.y, dir.x));
        float along = dot(rel, dir);
        float2 mp = float2(along * 3.0, across * 6.0);
        float cloud = dn_noise(mp * 2.0) * 0.65 + dn_noise(mp * 5.3 + 3.1) * 0.35;
        float dust = smoothstep(0.35, 0.75, dn_noise(float2(along * 5.0, across * 16.0) + 11.0));
        float core = exp(-across * across * 90.0);
        float wide = exp(-across * across * 16.0);
        float mw = (core * (0.55 + 0.6 * cloud) * (1.0 - 0.75 * dust * exp(-across * across * 300.0)) + wide * 0.25 * cloud) * glow;
        mw *= smoothstep(0.30, 0.55, sx);
        float3 mwCol = lerp(float3(0.36, 0.42, 0.62), float3(0.80, 0.66, 0.50), saturate(exp(-across * across * 220.0) * cloud));
        col += mwCol * mw * 0.45;

        // Stars: one at most per cell, more of them inside the Milky Way.
        float cells = 170.0;
        float2 g = sp * cells;
        float2 gc = floor(g);
        float3 sh = dn_hash3(gc);
        float2 so = sh.xy * 0.7 + 0.15;
        float sd = length(g - gc - so) / cells * resolution.y;
        float keep = step(1.0 - (0.10 + 0.25 * wide) * stars, sh.z);
        float b3 = frac(sh.z * 17.3);
        b3 *= b3 * b3;
        float bright = b3 * b3 * 0.9 + 0.12;
        float twinkle = 0.85 + 0.15 * sin(t * (0.6 + sh.x) + sh.y * 40.0);
        float star = exp(-sd * sd * (0.9 - 0.5 * bright)) * keep * bright * twinkle;
        star *= 0.3 + 0.7 * smoothstep(0.05, 0.35, sx);
        col += lerp(float3(0.70, 0.80, 1.00), float3(1.00, 0.88, 0.72), sh.x) * star;
    }

    // A faint glow along the horizon, warmer toward the far right.
    col += lerp(float3(0.03, 0.04, 0.07), float3(0.09, 0.06, 0.05), sx) * exp(-max(0.60 - uv.y, 0.0) * 10.0);

    // The moon, low on the right, slowly climbs and sinks again, so the
    // line between the lit and the shaded faces creeps over the dunes.
    float swing = sin(t * 0.010 + 1.2);
    float2 light = normalize(float2(0.80 + 0.15 * swing, 0.07 + 0.06 * (1.0 - swing)));

    // Four rows of dunes, far to near.
    [unroll] for (int i = 0; i < 4; i++)
    {
        float k = i;
        float r = dn_ridge(uv.x, k);
        float land = smoothstep(r - aa, r + aa, uv.y);
        if (land > 0.0)
        {
            // The face is lit by the slope just under the crest; the line
            // between light and shade bends downhill.
            float depth = max(uv.y - r, 0.0);
            float xs = uv.x - depth * 0.5 + 0.006 * sin(depth * 60.0 + k);
            float slope = (dn_ridge(xs + 0.004, k) - dn_ridge(xs, k)) / 0.004;
            // Height falls as y grows, so a positive slope faces right.
            float facing = dot(normalize(float2(slope, 1.0)), light);
            float lit = smoothstep(0.0, 0.18, facing);
            // Far below the crest the faces flatten into the hollow between dunes.
            lit = lerp(lit, 0.45, smoothstep(0.02, 0.12, depth));
            float far = 1.0 - k / 3.0;
            float3 sunny = lerp(float3(0.30, 0.31, 0.41), float3(0.18, 0.20, 0.31), far);
            float3 shade = lerp(float3(0.018, 0.020, 0.040), float3(0.050, 0.058, 0.100), far);
            float3 sand = lerp(shade, sunny, lit * (0.55 + 0.45 * moon));
            col = lerp(col, sand, land);
        }
    }
    return col;
}
