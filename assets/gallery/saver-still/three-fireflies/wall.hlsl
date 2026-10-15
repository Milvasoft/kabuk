// Three fireflies: a long exposure of a summer night. Each firefly's looping flight
// is written only where it flashed, in short green-gold strokes, ending at the firefly.

float2 flight(int i, float t)
{
    float2 S = i == 0 ? float2(-0.66, -0.16) : (i == 1 ? float2(-0.02, -0.33) : float2(0.36, 0.25));
    float2 E = i == 0 ? float2(-0.2, 0.04) : (i == 1 ? float2(0.36, -0.17) : float2(0.56, 0.2));
    float R = i == 0 ? 0.075 : (i == 1 ? 0.05 : 0.025);
    float w = i == 0 ? 15.0 : (i == 1 ? 12.0 : 10.0);
    float ph = i == 0 ? 0.6 : (i == 1 ? 2.4 : 4.0);
    float2 base = lerp(S, E, t) + float2(0.0, 0.06 * sin(t * 3.3 + ph));
    R *= 1.0 + 0.35 * sin(t * 4.0 + ph * 2.0);
    return base + R * float2(cos(w * t + ph) - cos(ph), sin(w * t + ph) - sin(ph));
}

// How bright the trail is at `t`: a few quick flashes, each rising fast and fading.
float flashes(int i, float t)
{
    float k = 0.0;
    [unroll]
    for (int f = 0; f < 9; f++)
    {
        float tf = 0.03 + 0.108 * float(f) + 0.03 * sin(float(f * 7 + i * 3));
        float x = t - tf;
        k += x > 0.0 ? 1.4 * exp(-x / 0.022) * smoothstep(0.0, 0.006, x) : 0.0;
    }
    // older flashes have faded more in the picture
    return k * (0.4 + 0.8 * t);
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 green = float3(0.72, 1.0, 0.28);
    float3 hot = float3(1.0, 1.0, 0.75);
    float3 col = 0.0;

    [unroll]
    for (int i = 0; i < 3; i++)
    {
        float scale = i == 0 ? 1.0 : (i == 1 ? 0.8 : 0.55);
        float best = 1.0;
        float bestT = 0.0;
        float2 a = flight(i, 0.0);
        [unroll]
        for (int j = 0; j < 48; j++)
        {
            float2 b = flight(i, float(j + 1) / 48.0);
            float2 ab = b - a;
            float h = saturate(dot(p - a, ab) / dot(ab, ab));
            float d = length(p - a - ab * h);
            if (d < best)
            {
                best = d;
                bestT = (float(j) + h) / 48.0;
            }
            a = b;
        }
        // refine against the true curve near the closest segment: no visible facets
        float2 cp = flight(i, bestT);
        float2 tg = normalize(flight(i, bestT + 0.003) - flight(i, bestT - 0.003));
        float2 off = p - cp;
        float along = dot(off, tg);
        best = min(best, abs(off.x * tg.y - off.y * tg.x) + max(abs(along) - 0.01, 0.0));
        float glowK = flashes(i, bestT) * trails;
        float w = (0.0006 + 0.0012 * saturate(glowK)) * scale;
        float stroke = smoothstep(w + pxs, max(w - pxs, 0.0), best);
        col += lerp(green, hot, saturate(glowK - 0.6) * 0.6) * glowK * (stroke * 1.1 + exp(-best / (0.005 * scale)) * 0.16);
        col += green * 0.03 * trails * stroke * smoothstep(0.0, 0.05, bestT);

        // The firefly itself, at the end of its flight.
        float2 fpos = flight(i, 1.0);
        float fd = length(p - fpos);
        float r = 0.0035 * scale;
        col += hot * smoothstep(r + pxs, r - pxs, fd);
        col += green * (exp(-fd / (0.008 * scale)) * 0.45 + exp(-fd / (0.035 * scale)) * 0.08);
    }

    return max(col - 0.003, 0.0);
}
