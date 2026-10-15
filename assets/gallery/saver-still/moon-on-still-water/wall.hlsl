// Moon on still water: a thin crescent low over a calm sea, its light laid down the
// water as a path of broken silver streaks; a few faint stars, nothing else.

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float vnoise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash21(i);
    float b = hash21(i + float2(1.0, 0.0));
    float c = hash21(i + float2(0.0, 1.0));
    float d = hash21(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, f.x), lerp(c, d, f.x), f.y);
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 silver = float3(0.86, 0.91, 1.0);
    float h = -0.13;           // the horizon
    float2 m = float2(0.27, 0.12); // the moon
    float r = 0.05;
    float3 col = 0.0;

    if (p.y > h)
    {
        // The crescent: the disc less a shadow disc pushed away from the sun, which has
        // set below the horizon to the lower right.
        float2 sun = normalize(float2(0.55, -1.0));
        float2 q = p - m;
        float d1 = length(q) - r;
        float d2 = length(q + sun * r * phase) - r * 1.005;
        float lit = smoothstep(pxs, -pxs, d1) * smoothstep(-pxs, pxs * 2.0, d2);
        // brighter towards the limb, with a hint of surface
        float limb = saturate(dot(normalize(q + 1e-5), sun));
        float surf = 0.85 + 0.15 * vnoise(q * 140.0);
        col += silver * lit * (0.62 + 0.38 * limb) * surf;
        // the dark side catches a little earthshine
        float disc = smoothstep(pxs, -pxs, d1);
        col += silver * disc * (1.0 - lit) * 0.035 * (0.7 + 0.6 * vnoise(q * 60.0 + 3.0));
        // a soft halo around the bright edge only
        float dc = max(max(d1, -d2), 0.0);
        col += silver * 0.09 * exp(-dc / 0.012) * (1.0 - lit) * smoothstep(-0.02, 0.01, d2 + 0.015);
        col += silver * 0.012 * exp(-max(d1, 0.0) / 0.04) * smoothstep(-pxs, pxs, d1);

        // Faint stars, fewer near the horizon and around the moon.
        float2 g = p * 34.0;
        float2 cell = floor(g);
        float2 off = float2(hash21(cell), hash21(cell + 17.0)) * 0.7 + 0.15;
        float pick = hash21(cell + 41.0);
        float sd = length(g - cell - off) / 34.0;
        float sz = (0.0007 + 0.0011 * hash21(cell + 5.0));
        float keep = step(pick, 0.2 * stars) * smoothstep(h + 0.03, h + 0.12, p.y) * smoothstep(0.12, 0.2, length(p - m));
        col += lerp(float3(1.0, 0.93, 0.85), float3(0.8, 0.87, 1.0), hash21(cell + 9.0))
               * keep * smoothstep(sz + pxs, sz - pxs, sd) * (0.25 + 0.45 * hash21(cell + 3.0));

        // A breath of haze on the horizon under the moon.
        float haze = exp(-abs(p.y - h) / 0.006) * exp(-abs(p.x - m.x) / 0.18);
        col += silver * haze * 0.05;
    }
    else
    {
        // The water: perspective rows close up towards the horizon.
        float dy = h - p.y;
        float z = 1.0 / (dy + 0.012);
        float dx = p.x - m.x;
        // The path widens as it comes nearer.
        float wcol = 0.02 + 0.3 * dy;
        float path = exp(-dx * dx / (wcol * wcol));
        float body = (0.45 + 0.55 * exp(-dy / 0.12)) * smoothstep(-0.47, -0.33, p.y);
        // Long thin horizontal glints: low frequency across, high frequency down.
        float n1 = vnoise(float2(dx * z * 5.0 + 3.0, z * 12.0));
        float n2 = vnoise(float2(dx * z * 11.0 - 7.0, z * 23.0 + 11.0));
        float glint = smoothstep(0.6, 0.76, n1 * 0.6 + n2 * 0.4);
        // Rows near the horizon are finer than a pixel; soften them there.
        float fine = smoothstep(0.0, 0.035, dy);
        float k = path * body * lerp(0.2, glint, fine);
        // a thin bright line where water meets sky
        k += exp(-dy / 0.002) * exp(-dx * dx / 0.003) * 0.5;
        col += silver * k * 0.95;
    }

    return max(col - 0.003, 0.0);
}
