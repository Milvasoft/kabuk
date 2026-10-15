// Paper dunes: layers of cut paper shaped like dunes, each casting a soft
// shadow on the one behind, while a low sun drifts slowly across them.

float pd_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// The top edge of layer i (0 is the farthest), in screen units from the top.
float pd_edge(float x, float fi, float t)
{
    float base = 0.36 + fi * 0.105;
    float k = 1.0 + fi * 0.35;
    float drift = t * (0.15 + fi * 0.06);
    // A sine with its second harmonic leans the crests like wind-blown sand.
    float z = x * 1.5 * k + fi * 1.9 + drift;
    float z2 = x * 3.7 * k - fi * 0.7 + drift * 1.3;
    return base
        - 0.050 * (sin(z) + 0.38 * sin(2.0 * z + 1.2))
        - 0.020 * (sin(z2) + 0.30 * sin(2.0 * z2 + 1.0))
        - 0.006 * sin(x * 9.1 * k + fi * 2.3 - drift * 0.7);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.03;

    // The sun drifts along the far ridge and back over many minutes.
    float2 sun = float2(aspect * (0.62 + 0.18 * sin(t * 0.7)), 0.30 + 0.03 * sin(t * 1.1));
    float sd = length(uv - sun);

    float3 skyHi = lerp(float3(0.98, 0.86, 0.78), float3(0.72, 0.66, 0.86), dusk);
    float3 skyLo = lerp(float3(1.00, 0.80, 0.66), float3(0.98, 0.66, 0.60), dusk);
    float3 col = lerp(skyHi, skyLo, smoothstep(0.0, 0.5, uv.y));
    col += float3(0.10, 0.07, 0.03) * exp(-sd * 5.0);
    col = lerp(col, float3(1.0, 0.95, 0.86), smoothstep(0.075, 0.068, sd) * 0.85);

    float3 nearC = lerp(float3(0.50, 0.24, 0.30), float3(0.26, 0.17, 0.32), dusk);
    float3 farC = lerp(float3(1.00, 0.80, 0.66), float3(0.90, 0.68, 0.70), dusk);

    [unroll] for (int i = 0; i < 6; i++)
    {
        float fi = i;
        float e = pd_edge(uv.x, fi, t);
        float d = uv.y - e;
        float a = fi / 5.0;
        float3 layer = lerp(farC, nearC, a * a * 0.5 + a * 0.5);
        // Paper is lit from the sun: brighter near the sun and near the edge.
        float lit = exp(-abs(uv.x - sun.x) * 1.6) * (1.0 - a * 0.5);
        layer *= 0.92 + 0.16 * lit;
        // Each sheet is lighter along its cut edge and deepens below it.
        layer *= 1.04 - 0.16 * smoothstep(0.0, 0.18, d);
        float rim = exp(-max(d, 0.0) * resolution.y / 3.0) * lit * 0.12;
        layer += rim;

        float inside = smoothstep(-0.6, 0.6, d * resolution.y);
        // The layer's shadow falls on what lies behind, just above its edge.
        float shadow = 1.0 - 0.24 * exp(min(d, 0.0) * 110.0) * (1.0 - inside);
        col = lerp(col * shadow, layer, inside);
    }

    // Fibres of the paper, fixed in place.
    float g = pd_hash(floor(px / max(resolution.y / 1080.0, 1.0)) + 0.5) - 0.5;
    col *= 1.0 + g * 0.035 * grain;
    return col;
}
