// Ink bloom: indigo ink unfurling through still water on warm paper.
// A layered sine-flow warp gives the curling plumes; the ink gathers
// on the right and leaves the left (desktop icons) as clear paper.

float ib_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float ib_noise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = ib_hash(i);
    float b = ib_hash(i + float2(1.0, 0.0));
    float c = ib_hash(i + float2(0.0, 1.0));
    float d = ib_hash(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float ib_fbm(float2 p)
{
    float s = 0.0;
    float a = 0.5;
    [unroll] for (int i = 0; i < 5; i++)
    {
        s += a * ib_noise(p);
        p = float2(1.6 * p.x + 1.2 * p.y, -1.2 * p.x + 1.6 * p.y) + 7.3;
        a *= 0.5;
    }
    return s * 1.032;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.026;

    // Turbulent flow: each layer bends the next, finer and weaker.
    float2 p = uv * 2.4 + float2(1.3, 0.4);
    float2 w = p;
    float amp = 0.55;
    float fr = 1.0;
    [unroll] for (int i = 0; i < 7; i++)
    {
        w += amp * float2(sin(fr * w.y + t * (1.0 + 0.13 * i) + 1.7 * i),
                          sin(fr * w.x - t * (0.8 + 0.11 * i) + 2.9 * i));
        w = float2(0.8 * w.x - 0.6 * w.y, 0.6 * w.x + 0.8 * w.y);
        fr *= 1.45;
        amp *= 0.62;
    }
    float f = ib_fbm(w * 0.9 + float2(0.0, t * 0.5));

    // Where the ink may go: none at the left edge, growing to the right.
    float xr = uv.x / aspect;
    float reach = smoothstep(0.15, 0.8, xr);
    float g = f * (0.55 + 0.6 * reach) + 0.12 * (density - 1.0);
    float ink = smoothstep(0.55, 0.95, g);
    float veil = smoothstep(0.3, 0.7, g);

    float3 paper = lerp(float3(0.90, 0.91, 0.90), float3(0.95, 0.91, 0.84), warmth);
    paper *= 1.0 - 0.05 * length(uv - float2(aspect * 0.35, 0.45));
    float3 indigo = float3(0.16, 0.20, 0.42);
    float3 deep = float3(0.04, 0.05, 0.13);
    float3 second = lerp(float3(0.20, 0.45, 0.50), float3(0.62, 0.30, 0.24), warmth);

    // A second, thinner ink folded into the first where the flow turned.
    float turn = smoothstep(0.35, 0.95, sin(w.x * 0.7 - w.y * 0.5 + t));
    float3 tint = lerp(indigo, second, turn * 0.55);

    // Pigment piles up along the edge of a plume.
    float rim = saturate(1.0 - abs(g - 0.6) * 18.0) * smoothstep(0.1, 0.5, reach);

    float3 col = paper;
    col = lerp(col, lerp(paper, tint, 0.6), veil * 0.55);
    col = lerp(col, lerp(tint, deep, smoothstep(0.8, 1.1, g)), ink);
    col = lerp(col, tint, rim * 0.22);
    return col;
}
