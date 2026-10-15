// Leaf shallows: looking down through clear shallow water at pale sand.
// Sunlight gathers into a slow web of light while a few leaves drift
// across; the water deepens toward the left, where the desktop icons sit.

// The water surface's bend at p: a few slow crossing swells.
float2 pc_bend(float2 p, float t)
{
    float2 b = float2(sin(p.y * 1.7 + t * 1.1) + sin(p.x * 0.9 - p.y * 1.3 + t * 0.7),
                      sin(p.x * 1.5 - t * 0.9) + sin(p.x * 1.1 + p.y * 0.8 + t * 0.6));
    return b;
}

// A web of focused light: each step bends the point by the last, and
// the light gathers where the bent grid lines pass close.
float pc_web(float2 p, float t)
{
    float2 i = p;
    float c = 1.0;
    [unroll] for (int n = 0; n < 5; n++)
    {
        float tt = t * (1.0 - 3.5 / (n + 1.0));
        i = p + float2(cos(tt - i.x) + sin(tt + i.y), sin(tt - i.y) + cos(tt + i.x));
        c += 1.0 / length(float2(p.x / (sin(i.x + tt) * 200.0), p.y / (cos(i.y + tt) * 200.0)));
    }
    c = 1.17 - pow(c * 0.2, 1.4);
    return pow(abs(c), 8.0);
}

// Soft shadow of a petal floating at c, with the bright ring its
// meniscus throws around it.
float2 pc_petal(float2 uv, float2 c, float r, float ang)
{
    float2 d = uv - c;
    float ca = cos(ang);
    float sa = sin(ang);
    d = float2(ca * d.x - sa * d.y, sa * d.x + ca * d.y);
    // A leaf: its half-width narrows to points at both ends.
    float u = d.x / (r * 2.4);
    float l = abs(d.y) / r + u * u;
    float shade = 1.0 - smoothstep(0.8, 1.2, l);
    float le = length(float2(d.y / r, u));
    float ring = exp(-(le - 1.12) * (le - 1.12) * 60.0) * 0.5 * (1.0 - shade);
    return float2(shade, ring);
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed * 0.18;

    float2 bend = pc_bend(uv * 3.0, t * 0.8);
    float2 p = uv * 4.2 + bend * 0.04 - 250.0;

    // Pale sand under clear water, deepening to the left.
    float shallow = smoothstep(0.0, 0.85, uv.x / aspect);
    float3 deepC = lerp(float3(0.10, 0.36, 0.42), float3(0.04, 0.20, 0.30), depth);
    float3 shallowC = lerp(float3(0.70, 0.84, 0.78), float3(0.46, 0.70, 0.70), depth);
    float3 floorC = lerp(deepC, shallowC, shallow);
    floorC *= 1.0 + 0.04 * sin(uv.x * 7.0 + sin(uv.y * 5.0) * 1.5) * sin(uv.y * 6.0 - uv.x * 2.0);

    float cw = pc_web(p, t);

    // Three petals drift across, wrapping around the screen, very slowly.
    float drift = time * speed * 0.006;
    float shade = 0.0;
    float ring = 0.0;
    [unroll] for (int k = 0; k < 3; k++)
    {
        float fk = k;
        float x = frac(0.25 + fk * 0.37 + drift * (0.8 + fk * 0.25)) * (aspect + 0.4) - 0.2;
        float y = 0.25 + fk * 0.27 + 0.05 * sin(drift * 9.0 + fk * 2.0);
        float2 pr = pc_petal(uv, float2(x, y), 0.026 + 0.006 * fk, drift * 6.0 + fk * 1.7);
        float fade = smoothstep(0.25, 0.5, x / aspect);
        shade = max(shade, pr.x * fade);
        ring += pr.y * fade;
    }

    float reach = lerp(0.3, 1.0, shallow) * sparkle;
    float3 col = floorC + float3(0.85, 1.0, 0.95) * cw * reach * 0.5;
    col += float3(0.05, 0.06, 0.05) * (bend.x + bend.y) * shallow * 0.5;
    col *= 1.0 - 0.28 * shade;
    col += float3(0.16, 0.18, 0.15) * ring * reach;
    return col;
}
