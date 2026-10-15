// Obsidian seams: a slab of black volcanic glass split into plates, molten
// rock glowing slowly in the seams between them. Each plate is tilted a
// little, so a soft light glides across the glass; the seams cool and fade
// toward the left, where the icons sit.

float2 os_hash2(float2 p)
{
    float3 p3 = frac(p.xyx * float3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.xx + p3.yz) * p3.zy);
}

// Cells around p: returns (gap to the nearest edge, distance to the own
// centre) and the own cell's id in cell.
float2 os_cells(float2 p, out float2 cell)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float d1 = 8.0;
    float d2 = 8.0;
    cell = i;
    [unroll] for (int y = -1; y <= 1; y++)
    {
        [unroll] for (int x = -1; x <= 1; x++)
        {
            float2 g = float2(x, y);
            float2 o = os_hash2(i + g) * 0.8 + 0.1;
            float d = length(g + o - f);
            if (d < d1)
            {
                d2 = d1;
                d1 = d;
                cell = i + g;
            }
            else
            {
                d2 = min(d2, d);
            }
        }
    }
    return float2(d2 - d1, d1);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.04;
    float sx = uv.x / aspect;
    float pxs = 1.0 / resolution.y;

    // Gently wandering seams: the plane is bent a little before the cells.
    float2 p = uv * plates * 3.2;
    p += 0.30 * float2(sin(p.y * 1.3 + 0.7), sin(p.x * 1.1 + 2.1)) + 0.08 * float2(sin(p.y * 4.1 + 1.0), sin(p.x * 3.7));
    float2 cell;
    float2 big = os_cells(p, cell);
    float2 fine;
    float2 small = os_cells(p * 2.7 + 5.3, fine);

    // Glass: each plate tilts its own way and catches a soft band of light.
    float2 tilt = os_hash2(cell + 17.0) - 0.5;
    float band = uv.y * 0.9 + (sx - 0.5) * 0.45 + tilt.x * 0.35 + tilt.y * 0.12 - 0.45 - 0.08 * sin(t * 0.6);
    // Shell-like ripples of the fracture bend the reflection a little.
    band += 0.012 * sin(big.y * 9.0 - tilt.x * 20.0);
    float sheen = 0.55 * exp(-band * band * 70.0) + 0.45 * exp(-band * band * 9.0);
    float3 glass = float3(0.022, 0.020, 0.032) + float3(0.17, 0.18, 0.24) * sheen;
    // A faint sky colour in the glass, deepest at each plate's heart.
    glass += float3(0.035, 0.030, 0.055) * smoothstep(0.1, 0.9, big.y);

    // Heat moves slowly along the seams, swelling and cooling in turn.
    float2 hc = uv * 1.3;
    float pulse = 0.55 + 0.25 * sin(hc.x * 2.1 + hc.y * 1.4 - t * 2.0) + 0.20 * sin(hc.x * 3.7 - hc.y * 2.9 + t * 1.3);
    float cool = smoothstep(0.04, 0.42, sx);
    float hot = heat * pulse * cool;

    // Width of a seam in cell units, kept at least about a pixel wide.
    float aaw = 0.6 * pxs * plates * 3.2;
    float w = 0.012 + 0.030 * pulse + aaw;
    float seam = smoothstep(w, 0.0, big.x);
    float halo = exp(-big.x * 9.0);
    // Fine cracks show only in places, as if the glass had just split there.
    float some = smoothstep(0.55, 0.85, sin(p.x * 0.9 + 1.0) * sin(p.y * 1.2 - 0.5) * 0.5 + 0.5 + 0.2 * pulse);
    float crack = smoothstep(0.010 + aaw * 2.7, 0.0, small.x) * 0.45 * some;
    float crackHalo = exp(-small.x * 30.0) * 0.15 * some;

    // Molten colour: yellow at the core, through orange to deep red.
    float core = saturate(seam * hot);
    float3 lava = lerp(float3(0.55, 0.06, 0.02), float3(1.00, 0.45, 0.10), core);
    lava = lerp(lava, float3(1.00, 0.85, 0.50), core * core * 0.6);

    float3 col = glass;
    col += float3(0.60, 0.10, 0.03) * halo * hot * 0.45;
    col += float3(0.50, 0.12, 0.04) * (crack + crackHalo) * hot;
    col = lerp(col, lava * (0.25 + 0.75 * hot), seam * saturate(0.25 + hot));
    return col;
}
