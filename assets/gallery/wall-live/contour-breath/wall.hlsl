// Contour breath: the contour lines of a quiet landscape whose hills rise
// and settle like slow breathing. Every fifth line is an index line.

float cb_hill(float2 p, float2 c, float r)
{
    float2 d = p - c;
    return exp(-dot(d, d) / (r * r));
}

float cb_height(float2 p, float t, float aspect)
{
    float h = 0.0;
    h += 1.00 * cb_hill(p, float2(aspect * 0.70 + 0.10 * sin(t * 0.31), 0.42 + 0.06 * cos(t * 0.23)), 0.34) * (0.85 + 0.15 * sin(t * 0.50));
    h += 0.75 * cb_hill(p, float2(aspect * 0.86 + 0.07 * cos(t * 0.27), 0.80 + 0.05 * sin(t * 0.35)), 0.26) * (0.80 + 0.20 * sin(t * 0.41 + 1.0));
    h += 0.60 * cb_hill(p, float2(aspect * 0.52 + 0.08 * sin(t * 0.19 + 2.0), 0.18 + 0.05 * sin(t * 0.29)), 0.22) * (0.80 + 0.20 * sin(t * 0.37 + 2.0));
    h += 0.55 * cb_hill(p, float2(aspect * 0.97 + 0.05 * sin(t * 0.21), 0.12), 0.24);
    h -= 0.35 * cb_hill(p, float2(aspect * 0.60 + 0.06 * cos(t * 0.17), 0.78), 0.18) * (0.7 + 0.3 * sin(t * 0.33 + 4.0));
    h += 0.45 * cb_hill(p, float2(aspect * 0.36, 0.55 + 0.08 * sin(t * 0.21 + 1.0)), 0.30);
    // Gentle terrain ripples so the lines are never perfect circles.
    h += 0.06 * sin(p.x * 3.1 + 1.3 * sin(p.y * 2.3 + t * 0.2)) + 0.04 * sin(p.y * 4.7 - p.x * 1.9 + t * 0.13);
    h += 0.018 * sin(p.x * 9.3 + 2.0 * sin(p.y * 5.1 - t * 0.3)) * sin(p.y * 7.7 + p.x * 2.3);
    return h;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.1;

    float h = cb_height(uv, t, aspect);
    float hn = h * lines * 1.15;
    float w = fwidth(hn);

    // Distance to the nearest line, in pixels.
    float fl = abs(frac(hn - 0.5) - 0.5) / max(w, 1e-4);
    float px1 = resolution.y / 1080.0;
    float minor = 1.0 - smoothstep(0.35 * px1, 0.35 * px1 + 1.0, fl);
    // Index lines: every fifth level, a little thicker.
    float hi = hn / 5.0;
    float fi = abs(frac(hi - 0.5) - 0.5) / max(w / 5.0, 1e-4);
    float index = 1.0 - smoothstep(0.9 * px1, 0.9 * px1 + 1.2, fi);

    // Light from the upper left on the slopes.
    float2 g = float2(ddx(h), ddy(h)) * resolution.y;
    float shade = saturate(0.5 + dot(g, float2(-0.5, -0.6)) * 0.6);

    float lowT = saturate(h * 0.9);
    float3 low = float3(0.055, 0.075, 0.085);
    float3 high = lerp(float3(0.10, 0.12, 0.12), float3(0.17, 0.12, 0.09), warmth);
    float3 col = lerp(low, high, lowT) * (0.8 + 0.4 * shade);

    // Fewer lines where the icons sit.
    float calm = lerp(0.25, 1.0, smoothstep(0.08, 0.35, uv.x / aspect));
    float3 ink = lerp(float3(0.42, 0.55, 0.52), float3(0.62, 0.56, 0.44), warmth * lowT);
    float3 gold = lerp(float3(0.55, 0.75, 0.72), float3(0.92, 0.62, 0.36), warmth);
    col = lerp(col, ink, minor * 0.35 * calm);
    col = lerp(col, gold, index * 0.75 * calm);
    return col;
}
