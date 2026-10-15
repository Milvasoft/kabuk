// Falling light petals: cherry petals made of soft light drift down through the dark,
// swaying and tumbling as they fall, in three depths; each glows for a while and fades.
// Every depth is a grid of columns falling at their own pace; a cell holds at most one
// petal, kept inside it, so a pixel only looks at its own cell in each depth.

static const int LAYERS = 3;

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// A petal in its own frame (unit length from base to tip, +y towards the tip): how far
// outside it the point is, in petal units (negative inside).
float petalShape(float2 p)
{
    float2 q = float2(p.x, p.y - 0.5);
    // A rounded drop: wide near the tip, narrow at the base.
    float wdt = 0.36 * (0.55 + 0.45 * saturate(p.y * 1.4));
    float body = length(float2(q.x / wdt, q.y / 0.5)) - 1.0;
    // The small V-shaped notch at the tip.
    float notch = (p.y - 0.88 - 1.7 * abs(p.x)) * 0.45;
    return max(body * 0.4, notch);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float pix = 1.0 / resolution.y;
    float3 col = 0;

    [unroll]
    for (int l = 0; l < LAYERS; l++)
    {
        float fl = l;
        float near = (fl + 1.0) / LAYERS;                 // 1/3, 2/3, 1
        float cellSize = 0.11 + 0.12 * near;
        float fall = 0.012 + 0.03 * near;
        float wind = breeze * (0.01 + 0.025 * near);
        float2 p = uv + float2(-wind * time, 0.0) + fl * 3.7;
        float column = floor(p.x / cellSize);
        float colSpeed = fall * (0.75 + 0.5 * hash12(float2(column, fl + 1.3)));
        float y = p.y - time * colSpeed + hash12(float2(column, fl + 5.1)) * 9.0;
        float2 cell = float2(column, floor(y / cellSize));
        float2 f = float2(frac(p.x / cellSize), frac(y / cellSize)) - 0.5;

        float h1 = hash12(cell + fl * 17.0);
        float h2 = hash12(cell + fl * 17.0 + 3.3);
        float h3 = hash12(cell + fl * 17.0 + 7.7);
        float present = step(h3, petals * (0.55 - 0.15 * near));
        // Swaying sideways, turning and tumbling as it falls.
        float sway = sin(time * (0.5 + 0.3 * h1) + 6.28 * h2);
        float2 c = float2(0.16 * sway, 0.08 * sin(time * 0.31 + 6.28 * h1));
        float ang = 0.9 * sway + 6.28 * h1;
        float tumble = cos(time * (0.35 + 0.4 * h2) + 6.28 * h3);
        float2 d = (f - c) / 0.3;                              // petal length: 0.3 cells, centred on c
        float cs = cos(ang), sn = sin(ang);
        float2 q = float2(cs * d.x - sn * d.y, sn * d.x + cs * d.y);
        q.x /= max(abs(tumble), 0.12);
        q.y += 0.5;
        float lw = pix / (0.3 * cellSize);                     // a pixel in petal units
        float s = petalShape(q);
        float soft = lw * (1.5 + 5.0 * near * near);           // near petals are a little out of focus
        float fill = saturate(0.5 - s / soft);
        float halo = exp(-max(s, 0.0) / (0.06 + 0.1 * near)) * 0.25;
        // Each petal glows for a while and fades, in its own rhythm.
        float life = sin(time * (0.06 + 0.05 * h2) + 6.28 * h1);
        life = saturate(life * 1.4) ;
        float face = 0.55 + 0.45 * abs(tumble);
        float3 tip = float3(1.0, 0.55, 0.72);
        float3 base = float3(1.0, 0.86, 0.9);
        float3 tint = lerp(base, tip, saturate(q.y));
        float bright = (0.25 + 0.6 * near) * face * life * present;
        col += tint * (fill * (0.55 + 0.45 * saturate(q.y)) + halo) * bright;
    }
    return col;
}
