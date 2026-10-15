// Neon arcade: a corridor of round arches drawn in neon tube, pink and violet in
// turn, running away to a lit doorway, mirrored in a wet floor.

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

// Distance (in the arch's own units) to a round arch: two pillars from the floor up
// to the spring line, and a half circle over them.
float archDist(float2 w, float floorY, float springY, float rad)
{
    float2 q = float2(abs(w.x), w.y);
    float pillar = length(q - float2(rad, clamp(q.y, floorY, springY)));
    float arc = q.y > springY ? abs(length(q - float2(0.0, springY)) - rad) : length(q - float2(rad, springY));
    return min(pillar, arc);
}

// Neon: a white-hot core, a coloured tube, and a short glow.
float3 neon(float d, float3 c, float w, float pxs)
{
    float core = smoothstep(w * 0.45 + pxs, w * 0.45 - pxs, d);
    float tube = smoothstep(w + pxs, w - pxs, d);
    float glow = exp(-d / (w * 4.0)) * 0.22 + exp(-d / (w * 14.0)) * 0.05;
    return lerp(c * tube, float3(1.0, 0.92, 0.97), core * 0.65) + c * glow;
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 pink = float3(1.0, 0.18, 0.55);
    float3 violet = float3(0.5, 0.3, 1.0);
    float3 col = 0.0;

    float2 V = float2(0.05, 0.03); // the vanishing point
    float floorY = -0.42;
    float springY = 0.04;
    float rad = 0.34;
    float camX = 0.13;

    [unroll]
    for (int k = 0; k < 7; k++)
    {
        float fk = float(k);
        float z = 1.15 + fk * 0.55 * depth;
        float3 c = (k % 2 == 0) ? pink : violet;
        float fade = exp(-fk * 0.26);
        // world point seen through this pixel at the arch's depth
        float2 w = (p - V) * z + float2(camX, 0.0) + float2(0.0, V.y);
        float d = archDist(w, floorY, springY, rad) / z;
        float tw = 0.0016 / sqrt(z);
        col += neon(d, c, tw, pxs) * fade;
        // its mirror image in the wet floor, broken by ripples
        float2 wr = float2(w.x, 2.0 * floorY - w.y);
        float dr = archDist(wr, floorY, springY, rad) / z;
        float below = step(w.y, floorY);
        float ripple = 0.7 + 0.3 * smoothstep(0.3, 0.7, vnoise(float2(p.x * 7.0, p.y * 300.0)));
        float depthFade = exp(-(floorY - w.y) / 0.3) * smoothstep(-0.47, -0.38, p.y);
        col += neon(dr, c, tw * 1.5, pxs) * fade * below * ripple * depthFade * reflection * 0.85;
    }

    // The doorway at the far end, softly lit.
    float zEnd = 1.15 + 7.2 * 0.55 * depth;
    float2 we = (p - V) * zEnd + float2(camX, V.y);
    float2 qd = float2(abs(we.x), we.y);
    float door = qd.y > springY ? length(qd - float2(0.0, springY)) - rad * 0.55 : qd.x - rad * 0.55;
    door = max(door, floorY - we.y);
    float inDoor = smoothstep(pxs * zEnd, -pxs * zEnd, door);
    float rise = saturate((we.y - floorY) / (springY + rad * 0.55 - floorY));
    float3 warm = float3(1.0, 0.74, 0.82);
    col += warm * inDoor * (0.78 - 0.5 * rise);
    col += pink * exp(-max(door, 0.0) / 0.06) * (1.0 - inDoor) * 0.08;
    // its reflection in the wet floor
    float2 wd = float2(abs(we.x), 2.0 * floorY - we.y);
    float doorR = wd.y > springY ? length(wd - float2(0.0, springY)) - rad * 0.55 : wd.x - rad * 0.55;
    doorR = max(doorR, floorY - wd.y);
    float riseR = saturate((wd.y - floorY) / (springY + rad * 0.55 - floorY));
    float ripple = 0.6 + 0.4 * smoothstep(0.3, 0.7, vnoise(float2(p.x * 7.0, p.y * 300.0)));
    col += warm * smoothstep(pxs * zEnd, -pxs * zEnd, doorR) * step(we.y, floorY) * (0.78 - 0.5 * riseR) * 0.3 * reflection * ripple
           * exp(-(floorY - we.y) / 0.6);

    // Floor seams running to the doorway, faint.
    float2 wf = (p - V);
    float dz = (floorY - V.y) / min(wf.y, -1e-4); // depth of the floor point
    float2 fw = float2(wf.x * dz + camX, floorY);
    float onFloor = step(wf.y, 0.0) * step(1.15, dz) * step(dz, zEnd);
    float seam = abs(frac(fw.x / 0.17 + 0.5) - 0.5) * 0.17 / dz;
    col += violet * 0.18 * onFloor * smoothstep(1.2 * pxs, 0.2 * pxs, seam) * exp(-(dz - 1.15) * 0.25) * step(abs(fw.x - camX), rad * 0.95);

    return max(col - 0.003, 0.0);
}
