// Chrome drift: drops of liquid metal drifting on a graphite ground, slowly
// merging and parting like a quiet lava lamp. Each drop mirrors a soft
// studio: a bright sky with two long softboxes over a dark floor, so the
// horizon line bends across every surface. "tint" colours the studio light
// with the look's accent.

float cd_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// The studio seen in direction r (x right, y up, z toward the viewer).
float3 cd_studio(float3 r, float3 tintC)
{
    // A bright sky above, a dark floor below, a soft horizon between.
    float up = r.y;
    float sky = smoothstep(-0.06, 0.10, up);
    float3 skyC = lerp(float3(0.62, 0.66, 0.72), float3(0.86, 0.88, 0.92), saturate(up));
    float3 floorC = lerp(float3(0.07, 0.07, 0.08), float3(0.16, 0.15, 0.15), saturate(-up * 2.0));
    float3 c = lerp(floorC, skyC, sky);
    // Two long softboxes: one high and cool, one low on the right and warm.
    float b1 = smoothstep(0.18, 0.05, abs(r.y - 0.62)) * smoothstep(0.75, 0.45, abs(r.x + 0.15));
    float b2 = smoothstep(0.10, 0.02, abs(r.x - 0.78)) * smoothstep(0.55, 0.25, abs(r.y - 0.15));
    c += float3(0.95, 0.98, 1.0) * b1 * 0.9;
    c += float3(1.0, 0.86, 0.70) * b2 * 0.7;
    // A warm glow on the floor from the right, a cool one on the left.
    c += float3(0.30, 0.18, 0.10) * (1.0 - sky) * smoothstep(-0.4, 0.8, r.x) * 0.6;
    c += float3(0.06, 0.10, 0.18) * (1.0 - sky) * smoothstep(0.4, -0.8, r.x) * 0.8;
    return c * tintC;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.022;
    // y up from here on.
    float2 p = float2(uv.x, -uv.y);

    float f = 0.0;
    float sh = 0.0;
    float2 gf = 0.0;
    // The light is high on the left: shadows fall down and to the right.
    float2 so = float2(0.035, -0.045) * size;
    [unroll] for (int i = 0; i < 7; i++)
    {
        float fi = i;
        float2 c = float2(aspect * (0.62 + 0.26 * sin(t * (0.31 + 0.07 * fi) + fi * 2.3)),
                          -0.5 + 0.32 * sin(t * (0.23 + 0.05 * fi) + fi * 1.7));
        float r = size * (0.075 + 0.045 * frac(fi * 0.618 + 0.2)) * (0.9 + 0.1 * sin(t * 0.7 + fi));
        float2 d = p - c;
        float dd = dot(d, d) + 1e-5;
        float k = r * r / dd;
        f += k;
        float2 ds = d - so;
        sh += r * r / (dot(ds, ds) + 1e-5);
        gf += -2.0 * k * d / dd;
    }

    // The ground: graphite, a soft pool of shade under the metal.
    float2 vv = px / resolution - 0.5;
    float3 bg = lerp(float3(0.115, 0.118, 0.125), float3(0.055, 0.056, 0.062), saturate(dot(vv, vv) * 2.2));
    bg *= 1.0 - 0.55 * smoothstep(0.2, 1.1, sh);
    // A faint reflected glow of the warm softbox below the drops.
    bg += float3(0.05, 0.035, 0.02) * smoothstep(0.25, 0.9, f) * (1.0 - smoothstep(0.9, 1.0, f));

    // Each drop is a dome: height sqrt(1 - 1/f), a hemisphere for one drop.
    float fs = max(f, 1.0001);
    float h = sqrt(1.0 - 1.0 / fs);
    float2 gh = gf / (2.0 * h * fs * fs);
    float R0 = 0.11 * size;
    float3 n = normalize(float3(-gh * R0, 1.0));
    float3 rv = reflect(float3(0.0, 0.0, -1.0), n);

    float3 tintC = lerp(float3(1.0, 1.0, 1.0), 0.35 + 0.9 * tok_accent, tint);
    float3 metal = cd_studio(rv, tintC);
    // Chrome is a little darker and bluer looking straight in.
    metal *= lerp(float3(0.80, 0.82, 0.86), float3(1.0, 1.0, 1.0), saturate(1.0 - n.z + 0.3));

    float w = fwidth(f);
    float inside = smoothstep(1.0 - w, 1.0 + w, f);
    // A thin dark contact line at the edge, the floor seen at a grazing angle.
    metal *= lerp(0.55, 1.0, smoothstep(1.0, 1.0 + 6.0 * w, f));
    float3 col = lerp(bg, metal, inside);

    col += (cd_hash(px) - 0.5) / 255.0;
    return col;
}
