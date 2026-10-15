// Quiet orrery: a minimal planetary model on deep ink. Thin tilted orbits,
// small flat planets drawing a fading arc behind them, and a lit sphere at
// the centre that hides whatever passes behind it.

static const float TAU = 6.2831853;

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;
    float pxw = 1.0 / resolution.y;

    float2 centre = float2(aspect * 0.64, 0.5);
    float2 p = uv - centre;
    // The system is seen from above at an angle and turns very slowly.
    float squash = lerp(0.95, 0.32, tilt);
    float phi = -0.32 + 0.05 * sin(t * 0.004);
    float cp = cos(phi);
    float sp = sin(phi);
    float2 r = float2(cp * p.x + sp * p.y, -sp * p.x + cp * p.y);
    float2 q = float2(r.x, r.y / squash);
    float qa = atan2(q.y, q.x);
    float ql = length(q);

    float3 bg = lerp(float3(0.045, 0.055, 0.095), float3(0.025, 0.030, 0.055), saturate(length(p) * 1.2));
    float3 col = bg;

    // The central body; its back half covers the orbits behind it.
    float sr = 0.105;
    float sd = length(p);
    float body = smoothstep(sr + pxw, sr - pxw, sd);
    float behind = step(0.0, -q.y) * body;

    float3 orbitC = float3(0.62, 0.66, 0.76);
    float3 planets[5] = {
        float3(0.96, 0.74, 0.32),
        float3(0.95, 0.47, 0.40),
        float3(0.56, 0.78, 0.68),
        float3(0.93, 0.90, 0.84),
        float3(0.50, 0.62, 0.95)
    };
    float radii[5] = { 0.17, 0.235, 0.31, 0.40, 0.50 };
    float sizes[5] = { 0.010, 0.016, 0.012, 0.021, 0.009 };

    float3 front = 0.0;
    float frontA = 0.0;
    [unroll] for (int i = 0; i < 5; i++)
    {
        float R = radii[i];
        // Slower further out, roughly as Kepler would have it.
        float w = 0.020 / pow(R / 0.17, 1.5);
        float a = t * w + i * 2.39;

        // The orbit line, about one pixel wide on screen.
        float dl = abs(ql - R) * squash / pxw;
        float ring = saturate(1.2 - dl) * 0.22;
        // The arc behind the planet glows and fades.
        float lag = frac((a - qa) / TAU);
        float arc = exp(-lag * 9.0) * trails;
        float3 lineC = lerp(orbitC, planets[i], saturate(arc * 1.5));
        float lineA = saturate(ring + saturate(1.6 - dl * 0.8) * arc * 0.55);
        lineA *= 1.0 - behind;
        col = lerp(col, lineC, lineA);

        // The planet itself: a flat disc, hidden when behind the body.
        float2 pq = R * float2(cos(a), sin(a));
        float2 pr = float2(pq.x, pq.y * squash);
        float2 pp = float2(cp * pr.x - sp * pr.y, sp * pr.x + cp * pr.y);
        float d = length(p - pp);
        float disc = smoothstep(sizes[i] + pxw, sizes[i] - pxw, d);
        float hidden = step(pq.y, 0.0) * body;
        disc *= 1.0 - hidden;
        // Planets in front of the body are drawn after it.
        float infront = step(0.0, pq.y);
        front = lerp(front, planets[i], disc * infront);
        frontA = max(frontA, disc * infront);
        col = lerp(col, planets[i], disc * (1.0 - infront));
    }

    // The body: lit from the upper left, with a soft terminator.
    float2 n2 = p / sr;
    float nz = sqrt(saturate(1.0 - dot(n2, n2)));
    float3 n = float3(n2, nz);
    float lit = saturate(dot(n, normalize(float3(-0.6, -0.5, 0.6))));
    float3 bodyC = lerp(float3(0.10, 0.11, 0.18), float3(0.96, 0.84, 0.70), smoothstep(0.0, 0.9, lit));
    bodyC += float3(0.10, 0.05, 0.02) * pow(1.0 - nz, 3.0) * (1.0 - lit);
    col = lerp(col, bodyC, body);
    // A faint halo around it.
    col += float3(0.05, 0.045, 0.04) * exp(-max(sd - sr, 0.0) * 30.0) * (1.0 - body);

    col = lerp(col, front, frontA);
    return col;
}
