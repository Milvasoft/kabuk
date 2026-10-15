// Wireframe galaxy: a spiral galaxy drawn as a fine mesh of rings and logarithmic
// spirals, tilted towards us and turning slowly; the mesh is brightest along its two
// arms, stars wheel round it (the inner ones faster) and its warm core drifts with it.

static const float TAU = 6.2831853;

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// A family of evenly spaced lines along `phase` (one line per unit), about a pixel wide.
float lines(float phase, float weight)
{
    float d = abs(frac(phase + 0.5) - 0.5);
    float w = max(fwidth(phase), 1e-5);
    // Lines closer than a few pixels melt away instead of turning to noise.
    return saturate(1.0 - d / (w * weight)) * saturate(0.25 / w - 0.25);
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * spin;

    // The galaxy's place, tilt and turn on the screen, all slowly changing.
    float2 centre = float2(0.18 * (aspect - 1.0) * sin(time * 0.011 + 1.0) + 0.05 * sin(time * 0.023),
                           0.06 * sin(time * 0.017 + 2.0));
    float scale = 0.46;
    float cosi = 0.42 + 0.12 * sin(time * 0.013);            // how far it leans away
    float pa = 0.45 + 0.3 * sin(time * 0.007);               // where its long axis points
    float cp = cos(pa), sp = sin(pa);
    float2 p = uv - centre;
    p = float2(cp * p.x + sp * p.y, -sp * p.x + cp * p.y);
    float2 disc = float2(p.x, p.y / cosi) / scale;
    float r = length(disc);
    float th = atan2(disc.y, disc.x);

    // The arms: a two-armed density wave that turns as one.
    float wind = 2.6;                                         // how tightly the arms wind
    float turn = t * 0.035;
    float armPhase = th - wind * log(max(r, 1e-3)) - turn;
    float arm = pow(0.5 + 0.5 * cos(2.0 * armPhase), 3.0);
    float edge = smoothstep(0.05, 0.22, r) * (1.0 - smoothstep(0.7, 1.02, r));

    // The mesh: spirals along the arms and rings across them.
    float spiralLines = lines(armPhase * (14.0 / TAU), 1.1);
    float ringLines = lines(r * 11.0, 1.1);
    float mesh = (spiralLines * (0.12 + 0.88 * arm) + ringLines * (0.08 + 0.5 * arm)) * edge;
    float3 cool = float3(0.42, 0.62, 1.0);
    float3 warm = float3(1.0, 0.78, 0.55);
    float3 meshCol = lerp(warm, cool, smoothstep(0.15, 0.55, r));
    float3 col = meshCol * mesh * 1.3;

    // A faint haze along the arms, and the warm core.
    col += cool * arm * edge * 0.05 * (1.0 - r);
    col += warm * (0.55 * exp(-r / 0.035) + 0.12 * exp(-r / 0.14));

    // Stars: one place per cell of a polar grid, wheeling round at their own pace.
    float rings = 36.0;
    float n = floor(r * rings);
    float rc0 = (n + 0.5) / rings;
    float omega = 0.11 / (rc0 + 0.12);
    float cells = max(6.0, n * 6.0);
    float a = (th - omega * t * 0.35) / TAU * cells;
    float cell = floor(a);
    float h = hash12(float2(n, cell));
    float h2 = hash12(float2(cell, n) + 7.0);
    float rc = (n + 0.35 + 0.3 * h2) / rings;
    float thc = (cell + 0.35 + 0.3 * h) / cells * TAU + omega * t * 0.35;
    float2 sd = float2(rc * cos(thc), rc * sin(thc) * cosi) * scale;
    float2 ps = float2(cp * sd.x - sp * sd.y, sp * sd.x + cp * sd.y);
    float sdist = length(uv - centre - ps) * resolution.y;    // in pixels
    float density = stars * (0.04 + 0.7 * arm * arm) * (1.0 - smoothstep(0.55, 1.0, r)) * step(r, 1.0);
    float present = step(frac(h * 7.13 + h2 * 3.7), density);
    float glint = saturate(1.3 - sdist / (0.7 + 1.2 * resolution.y / 2160.0));
    col += float3(0.85, 0.9, 1.0) * present * glint * (0.35 + 0.5 * h);

    return col;
}
