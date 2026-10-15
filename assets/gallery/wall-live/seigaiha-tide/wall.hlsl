// Seigaiha tide: the Japanese "blue sea wave" pattern of overlapping
// fans in indigo and cream. Rings ripple slowly outward within each fan
// and a broad swell of light rolls across; the left edge fades to plain
// indigo for the desktop icons.

float sg_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;
    float r = 0.105 / scale;
    float rowH = r * 0.5;
    float pxw = 1.0 / resolution.y;

    // The whole cloth drifts slowly to the left.
    float2 p = uv + float2(t * 0.002, 0.0);

    // Find the front-most fan covering this point: rows lower on the screen
    // overlap the ones above them.
    float j0 = floor(p.y / rowH) + 2.0;
    float found = 0.0;
    float dist = 0.0;
    float2 cell = 0.0;
    float gap = 1.0;
    [unroll] for (int k = 0; k < 5; k++)
    {
        float j = j0 - k;
        float off = frac(j * 0.5) * 2.0 * r;
        float cx = round((p.x - off) / (2.0 * r)) * 2.0 * r + off;
        float cy = j * rowH;
        float d = length(p - float2(cx, cy));
        float hit = (1.0 - found) * step(d, r);
        // How close the fans in front come: they cast a soft shadow.
        gap = lerp(gap, min(gap, (d - r) / r), (1.0 - found) * (1.0 - hit));
        dist = lerp(dist, d, hit);
        cell = lerp(cell, float2(cx, j), hit);
        found = max(found, hit);
    }

    // Rings inside the fan, moving slowly outward.
    float h = sg_hash(cell * 3.17 + 1.0);
    float rings = 5.0;
    float phase = dist / r * rings - t * 0.04;
    float f = frac(phase);
    float lineW = 0.14 + 0.05 * scale;
    float aa = pxw / r * rings * 1.5;
    float stroke = 1.0 - smoothstep(lineW - aa, lineW + aa, min(f, 1.0 - f) * 2.0);
    // The rim of each fan is a little stronger, where it overlaps the next.
    float rim = smoothstep(r - pxw * 3.0, r - pxw, dist) * found;

    // A broad, slow swell of light across the pattern.
    float swell = 0.5 + 0.5 * sin(uv.x * 1.6 - uv.y * 1.1 - t * 0.04);
    swell = swell * swell;

    float3 indigo = float3(0.050, 0.085, 0.190);
    float3 deep = float3(0.030, 0.050, 0.120);
    float3 cream = float3(0.86, 0.83, 0.74);
    float3 goldC = float3(0.86, 0.66, 0.34);
    // A few fans are picked out in gold leaf.
    float isGold = step(0.93, h) * gold;
    float3 lineC = lerp(cream, goldC, isGold);

    float3 col = lerp(deep, indigo, 0.5 + 0.5 * swell);
    // Each fan is a little lighter toward its centre, like a raised scale.
    col *= 0.85 + 0.3 * (1.0 - dist / r);
    float side = smoothstep(0.03, 0.30, uv.x / aspect);
    float strength = (0.14 + 0.6 * swell) * side;
    col = lerp(col, lineC, stroke * strength * found);
    col = lerp(col, deep * 0.8, rim * 0.6 * side);
    col *= 1.0 - 0.35 * exp(-max(gap, 0.0) * 14.0) * found;
    return col;
}
