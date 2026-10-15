// Riso planet: a ringed planet printed in two inks, fluorescent pink and blue, on warm
// paper. Every tone is a halftone screen at its own angle; where the inks overlap they
// multiply into violet, and the pink sits a hair off the blue, as on a stencil press.

uint pcg(uint v)
{
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

float grain01(float2 px)
{
    return float(pcg(uint(px.x) + pcg(uint(px.y)))) * (1.0 / 4294967295.0);
}

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float noise2(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = hash12(i);
    float b = hash12(i + float2(1.0, 0.0));
    float c = hash12(i + float2(0.0, 1.0));
    float d = hash12(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

// A halftone screen: how much of this point the ink covers for a tone of `dens` (0-1).
float screen(float2 p, float dens, float angle, float cell, float pix)
{
    float ca = cos(angle);
    float sa = sin(angle);
    float2 q = float2(ca * p.x + sa * p.y, -sa * p.x + ca * p.y) / cell;
    float2 g = frac(q) - 0.5;
    float r = sqrt(saturate(dens)) * 0.72;
    float aa = pix / cell;
    float dotc = 1.0 - smoothstep(r - aa, r + aa, length(g));
    // Full tones close up into solid ink.
    return max(dotc, smoothstep(0.92, 1.0, dens));
}

// The scene's two tones (pink, blue) at a point, in screen heights.
float2 tones(float2 p, float aspect)
{
    float2 pc = float2(aspect * 0.66, 0.50);
    float R = 0.24;
    float2 d = (p - pc) / R;
    float r2 = dot(d, d);
    float pink = 0.0;
    float blue = 0.0;

    // Background: a pink haze rising from the lower edge, a blue one from the top right.
    pink += 0.38 * smoothstep(0.55, 1.0, p.y) * smoothstep(0.0, aspect * 0.5, p.x);
    blue += 0.22 * smoothstep(0.6, 0.0, length(p - float2(aspect, 0.0)) / aspect);

    // The ring, in the planet's tilted frame.
    float tilt = -0.32;
    float2 rq = float2(cos(tilt) * d.x - sin(tilt) * d.y, sin(tilt) * d.x + cos(tilt) * d.y);
    float2 re = float2(rq.x, rq.y / 0.24);
    float rr = length(re);
    float band = smoothstep(1.32, 1.36, rr) * (1.0 - smoothstep(2.15, 2.19, rr));
    float gap = 1.0 - (smoothstep(1.70, 1.72, rr) * (1.0 - smoothstep(1.78, 1.80, rr)));
    float ringt = band * gap * (0.55 + 0.25 * sin(rr * 40.0) + 0.2 * sin(rr * 13.0));
    bool front = rq.y > 0.0;

    // The planet: a lit sphere with soft latitude bands.
    float inside = 1.0 - smoothstep(0.995, 1.005, r2);
    float z = sqrt(saturate(1.0 - r2));
    float3 n = float3(d.x, -d.y, z);
    float lit = saturate(dot(n, normalize(float3(-0.55, 0.55, 0.62))));
    float lat = rq.y;
    float bands = 0.5 + 0.5 * sin(lat * 9.0 + 0.6 * sin(rq.x * 3.0));
    float pb = lerp(0.95, 0.12, lit) * (0.75 + 0.25 * bands);
    float pp = (0.15 + 0.35 * (1.0 - bands)) * (0.4 + 0.6 * lit);
    // The ring's shadow lies across the planet below the ring plane.
    float rshadow = smoothstep(0.02, 0.10, -lat + 0.10) * (1.0 - smoothstep(0.10, 0.26, -lat + 0.10));
    pb = lerp(pb, 0.95, rshadow * 0.6);

    // Behind the planet: background, then the ring's far side, then the planet, then
    // the ring's near side.
    pink = lerp(pink, ringt, (front ? 0.0 : 1.0) * band * gap);
    blue = lerp(blue, 0.12 * ringt, (front ? 0.0 : 1.0) * band * gap);
    pink = lerp(pink, pp, inside);
    blue = lerp(blue, pb, inside);
    pink = lerp(pink, ringt, (front ? 1.0 : 0.0) * band * gap);
    blue = lerp(blue, 0.18 * ringt + 0.35 * (1.0 - smoothstep(1.3, 1.6, rr)) * band, (front ? 1.0 : 0.0) * band * gap);

    // Two moons.
    float2 m1 = (p - pc - float2(-0.52, -0.28)) / 0.045;
    float mi1 = 1.0 - smoothstep(0.97, 1.03, dot(m1, m1));
    float ml1 = saturate(0.5 - 0.6 * m1.x + 0.4 * m1.y);
    blue = lerp(blue, lerp(0.85, 0.15, ml1), mi1);
    pink = lerp(pink, 0.10, mi1);
    float2 m2 = (p - pc - float2(0.48, 0.30)) / 0.026;
    float mi2 = 1.0 - smoothstep(0.95, 1.05, dot(m2, m2));
    float ml2 = saturate(0.5 - 0.6 * m2.x + 0.4 * m2.y);
    pink = lerp(pink, lerp(0.95, 0.25, ml2), mi2);
    blue = lerp(blue, 0.05, mi2);
    return float2(pink, blue);
}

float3 wall(float2 px)
{
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float2 p = px / resolution.y;
    float cell = 0.0085 * dotsize;

    // Warm paper with a faint tooth.
    float tooth = noise2(p * 600.0) * 0.6 + noise2(p * 60.0) * 0.4;
    float3 paper = float3(0.965, 0.945, 0.900) * (0.98 + 0.03 * tooth);

    // Each ink is printed a hair off the other.
    float2 tp = tones(p + float2(0.0011, -0.0007), aspect);
    float2 tb = tones(p, aspect);
    float pink = screen(p, tp.x, 1.309, cell, pix);
    float blue = screen(p, tb.y, 0.262, cell, pix);

    // Stars: small solid pink and blue dots scattered in the dark corners.
    float2 sg = floor(p * 28.0);
    float sh = hash12(sg);
    float2 sp = (sg + 0.2 + 0.6 * float2(hash12(sg + 3.1), hash12(sg + 7.7))) / 28.0;
    float sr = 0.0012 + 0.0022 * hash12(sg + 1.3);
    float star = (1.0 - smoothstep(sr - pix, sr + pix, length(p - sp))) * step(0.72, sh);
    star *= smoothstep(aspect * 0.18, aspect * 0.40, p.x);
    pink = max(pink, star * step(0.5, frac(sh * 7.0)));
    blue = max(blue, star * step(frac(sh * 7.0), 0.5));

    // Ink density breathes across the sheet.
    float breathe = 0.86 + 0.14 * noise2(p * 18.0);
    float3 pinkInk = float3(1.00, 0.36, 0.60);
    float3 blueInk = float3(0.10, 0.42, 0.78);
    float3 c = paper;
    c *= lerp(float3(1.0, 1.0, 1.0), blueInk, blue * breathe);
    c *= lerp(float3(1.0, 1.0, 1.0), pinkInk, pink * breathe * 0.95);

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 6.0 / 255.0);
    return c;
}
